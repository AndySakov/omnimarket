//! The candle read model (#80): OHLCV per token and interval, in USD, built from trade records,
//! both live (`trades.base`) and read back from before each pool was tracked
//! (`trade-history.base`). Held in memory (D96).
//!
//! A candle is a function of the set of trades in it, never of the order they arrive in. Each
//! trade counts once, by its natural key (block hash, log index, D15); open and close are the
//! trades with the lowest and highest (block, log index); high and low are a max and a min; and
//! volume adds up exactly, in base units. So history that overlaps the live trades or arrives
//! after them, and a rebuild that reads the topics in another interleaving, give the same candles.
//!
//! Candles are kept per pool, in the pool's quote asset, and priced in USD when served. A token's
//! candle merges those of its pools counted in its latest price update (its main pool when none
//! is), each quoted in USDC or USDT at $1, or in WETH at WETH's own USD close for that candle.

use std::collections::{BTreeMap, BTreeSet};

use alloy_primitives::{U256, address};
use proto::api::v1::{Candle, CandleInterval, CandleSeries};
use proto::lineage::v1::Lineage;
use proto::price::v1::PriceUpdate;
use proto::trade::v1::Trade;
use types::LineageId;
use types::chain::{Address, B256};

use crate::model::{ModelError, decimal, hex, scaled_decimal};

/// Every interval the contract offers: its label in topics and `GET /v1/candles`, and its length
/// in ms. Candles open on multiples of their length since the Unix epoch, so days open at
/// midnight UTC.
pub const INTERVALS: [(CandleInterval, &str, u64); 7] = [
    (CandleInterval::CandleInterval1s, "1s", 1_000),
    (CandleInterval::CandleInterval1m, "1m", 60_000),
    (CandleInterval::CandleInterval5m, "5m", 300_000),
    (CandleInterval::CandleInterval15m, "15m", 900_000),
    (CandleInterval::CandleInterval1h, "1h", 3_600_000),
    (CandleInterval::CandleInterval4h, "4h", 14_400_000),
    (CandleInterval::CandleInterval1d, "1d", 86_400_000),
];

/// The most candles a series holds: a day of 1m candles. A longer range gets its newest.
pub const MAX_SERIES: usize = 1_440;

const WETH: Address = address!("4200000000000000000000000000000000000006");

/// How a quote asset is priced in USD.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Usd {
    /// A reference stablecoin: $1 (D19).
    Dollar,
    /// WETH: its own USD candles.
    Weth,
}

/// Base's quote assets (D102), with their decimals.
const QUOTES: [(Address, i32, Usd); 3] = [
    (
        address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
        6,
        Usd::Dollar,
    ),
    (
        address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2"),
        6,
        Usd::Dollar,
    ),
    (WETH, 18, Usd::Weth),
];

/// The interval a label names.
pub fn interval_named(label: &str) -> Option<CandleInterval> {
    INTERVALS
        .iter()
        .find(|(_, l, _)| *l == label)
        .map(|(interval, _, _)| *interval)
}

fn label(interval: CandleInterval) -> &'static str {
    INTERVALS
        .iter()
        .find(|(i, _, _)| *i == interval)
        .map_or("", |(_, l, _)| l)
}

fn length_ms(interval: CandleInterval) -> u64 {
    INTERVALS
        .iter()
        .find(|(i, _, _)| *i == interval)
        .map_or(1, |(_, _, ms)| *ms)
}

/// The `candles:<address>:<interval>` topic's name.
pub fn candles_topic(address: &str, interval: CandleInterval) -> String {
    format!("candles:{address}:{}", label(interval))
}

/// One pool's candle in its quote asset, exactly as the trades in it add up.
#[derive(Clone, Debug, Default)]
struct PoolCandle {
    /// Unset while no trade in it has a price.
    prices: Option<Prices>,
    /// Quote base units.
    volume: U256,
    trades: u64,
    /// The trade with the highest (block number, log index), and its lineage ID.
    last: Option<((u64, u64), Vec<u8>)>,
}

/// Quote base units per token base unit, × 10^36 (D102). Open and close carry their trade's
/// (block number, log index).
#[derive(Clone, Debug)]
struct Prices {
    open: ((u64, u64), U256),
    close: ((u64, u64), U256),
    high: U256,
    low: U256,
}

impl PoolCandle {
    fn add(&mut self, at: (u64, u64), price: Option<U256>, volume: U256, id: &[u8]) {
        self.trades += 1;
        self.volume = self.volume.saturating_add(volume);
        if self.last.as_ref().is_none_or(|(last, _)| at > *last) {
            self.last = Some((at, id.to_vec()));
        }
        let Some(price) = price else {
            return;
        };
        match &mut self.prices {
            None => {
                self.prices = Some(Prices {
                    open: (at, price),
                    close: (at, price),
                    high: price,
                    low: price,
                })
            }
            Some(p) => {
                if at < p.open.0 {
                    p.open = (at, price);
                }
                if at > p.close.0 {
                    p.close = (at, price);
                }
                p.high = p.high.max(price);
                p.low = p.low.min(price);
            }
        }
    }
}

/// What a token's latest price update says about its candles.
#[derive(Clone, Debug)]
struct Token {
    chain_id: u64,
    decimals: Option<u32>,
    /// Its pools counted toward the display price, or its main pool if none is.
    pools: Vec<Address>,
}

/// A token's candle in USD, before it becomes a `Candle`.
#[derive(Clone, Debug, PartialEq)]
struct UsdCandle {
    /// Open, high, low and close; unset when no trade in it has a price.
    prices: Option<[f64; 4]>,
    volume: f64,
    trades: u64,
    /// Its latest trade's lineage ID.
    cause: Vec<u8>,
}

/// A trade the model hadn't counted before.
#[derive(Clone, Debug, PartialEq)]
pub struct Counted {
    pub token: Address,
    pub pool: Address,
    /// The block time, ms.
    pub time_ms: u64,
}

#[derive(Default)]
pub struct Candles {
    /// Every trade counted, by (block hash, log index).
    counted: BTreeSet<(B256, u64)>,
    /// Each pool's token and quote asset, from its trades.
    pools: BTreeMap<Address, (Address, Address)>,
    /// By pool, interval and open time.
    candles: BTreeMap<(Address, CandleInterval, u64), PoolCandle>,
    tokens: BTreeMap<Address, Token>,
    /// The latest block time of a trade counted, ms: a candle that ends by then is closed.
    head_ms: u64,
}

impl Candles {
    /// Counts one trade, live or from history, unless it was counted before.
    pub fn apply_trade(&mut self, trade: &Trade) -> Result<Option<Counted>, ModelError> {
        let pool = address_field("pool", &trade.pool)?;
        let token = address_field("token", &trade.token)?;
        let quote = address_field("quote", &trade.quote)?;
        let hash =
            B256::try_from(trade.block_hash.as_slice()).map_err(|_| ModelError::BadHash {
                len: trade.block_hash.len(),
            })?;
        let volume = amount_field("quote_amount", &trade.quote_amount)?;
        let price = trade
            .price_e36
            .as_ref()
            .map(|p| amount_field("price_e36", p))
            .transpose()?;
        if !self.counted.insert((hash, trade.log_index)) {
            return Ok(None);
        }
        self.pools.insert(pool, (token, quote));
        let time_ms = trade.block_timestamp.saturating_mul(1000);
        self.head_ms = self.head_ms.max(time_ms);
        let at = (trade.block_number, trade.log_index);
        let id = trade
            .lineage
            .as_ref()
            .map(|l| l.id.as_slice())
            .unwrap_or_default();
        for (interval, _, length) in INTERVALS {
            let open = time_ms / length * length;
            self.candles
                .entry((pool, interval, open))
                .or_default()
                .add(at, price, volume, id);
        }
        Ok(Some(Counted {
            token,
            pool,
            time_ms,
        }))
    }

    /// Takes a token's decimals and the pools its candles merge from its latest price update.
    /// A pool that isn't a 20-byte address is left out.
    pub fn apply_price(&mut self, update: &PriceUpdate) -> Result<(), ModelError> {
        let token = address_field("token", &update.token)?;
        let counted: Vec<Address> = update
            .pools
            .iter()
            .filter(|p| p.counted)
            .filter_map(|p| Address::try_from(p.pool.as_slice()).ok())
            .collect();
        let pools = if counted.is_empty() {
            Address::try_from(update.main_pool.as_slice())
                .ok()
                .into_iter()
                .collect()
        } else {
            counted
        };
        let decimals = update.metadata.as_ref().and_then(|m| m.decimals);
        self.tokens.insert(
            token,
            Token {
                chain_id: update.chain_id,
                decimals,
                pools,
            },
        );
        Ok(())
    }

    /// Whether `pool`'s trades count toward `token`'s candles.
    pub fn merges(&self, token: Address, pool: Address) -> bool {
        self.pools_of(token).iter().any(|(p, _, _)| *p == pool)
    }

    /// The token's candle opening at `open_ms`, if it has one.
    pub fn candle(&self, token: Address, interval: CandleInterval, open_ms: u64) -> Option<Candle> {
        let chain_id = self.tokens.get(&token)?.chain_id;
        let usd = self.usd(token, interval, open_ms)?;
        Some(self.to_candle(chain_id, token, interval, open_ms, usd))
    }

    /// The token's candles opening in `from_ms..=to_ms`, oldest first, at most `MAX_SERIES` of
    /// the newest. Empty for a token without candles, or not on `chain_id` when that's given.
    pub fn series(
        &self,
        chain_id: Option<u64>,
        token: Address,
        interval: CandleInterval,
        from_ms: u64,
        to_ms: u64,
    ) -> CandleSeries {
        let known = self
            .tokens
            .get(&token)
            .filter(|t| chain_id.is_none_or(|c| c == t.chain_id));
        let chain_id = known.map_or(chain_id.unwrap_or_default(), |t| t.chain_id);
        let mut opens = BTreeSet::new();
        if known.is_some() && from_ms <= to_ms {
            for (pool, _, _) in self.pools_of(token) {
                opens.extend(
                    self.candles
                        .range((pool, interval, from_ms)..=(pool, interval, to_ms))
                        .map(|((_, _, open), _)| *open),
                );
            }
        }
        let skip = opens.len().saturating_sub(MAX_SERIES);
        let candles: Vec<Candle> = opens
            .into_iter()
            .skip(skip)
            .filter_map(|open| self.candle(token, interval, open))
            .collect();
        let newest = candles
            .last()
            .and_then(|c| c.lineage.as_ref())
            .map(|l| l.id.clone());
        let mut key = chain_id.to_le_bytes().to_vec();
        key.extend(token.as_slice());
        key.extend(length_ms(interval).to_le_bytes());
        for candle in &candles {
            key.extend(
                candle
                    .lineage
                    .as_ref()
                    .map(|l| l.id.as_slice())
                    .unwrap_or_default(),
            );
        }
        CandleSeries {
            lineage: Some(Lineage {
                id: LineageId::from_natural_key("candle_series", &key)
                    .as_bytes()
                    .to_vec(),
                caused_by: newest.into_iter().collect(),
            }),
            chain_id,
            token: hex(token.as_slice()),
            interval: interval.into(),
            candles,
        }
    }

    /// How many trades have been counted: what a candle delta, or a snapshot, is as of.
    pub fn version(&self) -> u64 {
        self.counted.len() as u64
    }

    /// The token's pools whose trades count toward its candles: those its latest price update
    /// names that trade the token against a quote asset, with the quote's decimals and USD.
    fn pools_of(&self, token: Address) -> Vec<(Address, i32, Usd)> {
        let Some(info) = self.tokens.get(&token) else {
            return Vec::new();
        };
        let mut pools: Vec<(Address, i32, Usd)> = info
            .pools
            .iter()
            .filter_map(|pool| {
                let (traded, quote) = self.pools.get(pool)?;
                let (_, decimals, usd) = QUOTES.iter().find(|(q, _, _)| q == quote)?;
                (*traded == token).then_some((*pool, *decimals, *usd))
            })
            .collect();
        pools.sort_by_key(|(pool, _, _)| *pool);
        pools.dedup_by_key(|(pool, _, _)| *pool);
        pools
    }

    /// The token's candle at `open_ms` in USD: its pools' candles merged, in pool order.
    fn usd(&self, token: Address, interval: CandleInterval, open_ms: u64) -> Option<UsdCandle> {
        let decimals = i32::try_from(self.tokens.get(&token)?.decimals?).ok()?;
        // Each pool's candle, the power of ten its raw prices are over, its quote's decimals, and
        // its quote's USD.
        let mut parts: Vec<(&PoolCandle, i32, i32, f64)> = Vec::new();
        for (pool, quote_decimals, usd) in self.pools_of(token) {
            let Some(candle) = self.candles.get(&(pool, interval, open_ms)) else {
                continue;
            };
            let dollars = match usd {
                Usd::Dollar => 1.0,
                // A pool's token is never its quote, so WETH's own candles never get here.
                Usd::Weth => match self.weth_close(interval, open_ms) {
                    Some(close) => close,
                    None => continue,
                },
            };
            // A raw price is quote base units per token base unit, × 10^36.
            let price_scale = 36 + quote_decimals - decimals;
            parts.push((candle, price_scale, quote_decimals, dollars));
        }
        let priced: Vec<(&Prices, i32, f64)> = parts
            .iter()
            .filter_map(|(c, scale, _, dollars)| Some((c.prices.as_ref()?, *scale, *dollars)))
            .collect();
        let prices = match (
            priced.iter().min_by_key(|(p, _, _)| p.open.0),
            priced.iter().max_by_key(|(p, _, _)| p.close.0),
        ) {
            (Some((first, a, x)), Some((last, b, y))) => Some([
                scaled(first.open.1, *a) * x,
                priced
                    .iter()
                    .map(|(p, scale, dollars)| scaled(p.high, *scale) * dollars)
                    .fold(f64::MIN, f64::max),
                priced
                    .iter()
                    .map(|(p, scale, dollars)| scaled(p.low, *scale) * dollars)
                    .fold(f64::MAX, f64::min),
                scaled(last.close.1, *b) * y,
            ]),
            _ => None,
        };
        let latest = parts
            .iter()
            .filter_map(|(c, _, _, _)| c.last.as_ref())
            .max_by_key(|(at, _)| *at)?;
        Some(UsdCandle {
            prices,
            volume: parts
                .iter()
                .map(|(c, _, quote_decimals, dollars)| scaled(c.volume, *quote_decimals) * dollars)
                .sum(),
            trades: parts.iter().map(|(c, _, _, _)| c.trades).sum(),
            cause: latest.1.clone(),
        })
    }

    /// WETH's USD close for the candle opening at `open_ms`: its own candle's, else the latest
    /// earlier one's, else the earliest later one's, passing over any candle whose trades have
    /// no price.
    fn weth_close(&self, interval: CandleInterval, open_ms: u64) -> Option<f64> {
        let pools = self.pools_of(WETH);
        // The latest WETH candle opening at or before `bound`, across its pools.
        let at_or_before = |bound: u64| {
            pools
                .iter()
                .filter_map(|(pool, _, _)| {
                    self.candles
                        .range((*pool, interval, 0)..=(*pool, interval, bound))
                        .next_back()
                        .map(|((_, _, open), _)| *open)
                })
                .max()
        };
        let at_or_after = |bound: u64| {
            pools
                .iter()
                .filter_map(|(pool, _, _)| {
                    self.candles
                        .range((*pool, interval, bound)..=(*pool, interval, u64::MAX))
                        .next()
                        .map(|((_, _, open), _)| *open)
                })
                .min()
        };
        let mut bound = Some(open_ms);
        while let Some(open) = bound.and_then(at_or_before) {
            if let Some(close) = self.close(interval, open) {
                return Some(close);
            }
            bound = open.checked_sub(1);
        }
        let mut bound = open_ms.checked_add(1);
        while let Some(open) = bound.and_then(at_or_after) {
            if let Some(close) = self.close(interval, open) {
                return Some(close);
            }
            bound = open.checked_add(1);
        }
        None
    }

    fn close(&self, interval: CandleInterval, open_ms: u64) -> Option<f64> {
        self.usd(WETH, interval, open_ms)?.prices.map(|p| p[3])
    }

    fn to_candle(
        &self,
        chain_id: u64,
        token: Address,
        interval: CandleInterval,
        open_ms: u64,
        usd: UsdCandle,
    ) -> Candle {
        let mut key = chain_id.to_le_bytes().to_vec();
        key.extend(token.as_slice());
        key.extend(length_ms(interval).to_le_bytes());
        key.extend(open_ms.to_le_bytes());
        key.extend(&usd.cause);
        let [open, high, low, close] = usd.prices.map_or_else(Default::default, |p| p.map(decimal));
        Candle {
            lineage: Some(Lineage {
                id: LineageId::from_natural_key("candle", &key)
                    .as_bytes()
                    .to_vec(),
                caused_by: vec![usd.cause],
            }),
            chain_id,
            token: hex(token.as_slice()),
            interval: interval.into(),
            open_time_ms: open_ms,
            open_usd: open,
            high_usd: high,
            low_usd: low,
            close_usd: close,
            volume_usd: decimal(usd.volume),
            trades: usd.trades,
            closed: open_ms.saturating_add(length_ms(interval)) <= self.head_ms,
        }
    }
}

/// `raw` / 10^`scale`, as the nearest `f64`: exact in decimal first, so a price of 0.5 is 0.5.
fn scaled(raw: U256, scale: i32) -> f64 {
    match u32::try_from(scale) {
        Ok(scale) => scaled_decimal(&raw.to_be_bytes::<32>(), scale)
            .parse()
            .unwrap_or(f64::NAN),
        Err(_) => f64::from(raw) * 10f64.powi(-scale),
    }
}

fn address_field(field: &'static str, bytes: &[u8]) -> Result<Address, ModelError> {
    Address::try_from(bytes).map_err(|_| ModelError::BadField {
        field,
        len: bytes.len(),
    })
}

/// A big-endian unsigned integer of at most 32 bytes.
fn amount_field(field: &'static str, bytes: &[u8]) -> Result<U256, ModelError> {
    U256::try_from_be_slice(bytes).ok_or(ModelError::BadField {
        field,
        len: bytes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quote_assets_are_the_engines() {
        let quotes: Vec<Address> = QUOTES.iter().map(|(q, _, _)| *q).collect();
        assert_eq!(quotes, engine::EngineConfig::base().quote_assets);
    }

    #[test]
    fn labels_name_the_contracts_intervals() {
        assert_eq!(
            interval_named("15m"),
            Some(CandleInterval::CandleInterval15m)
        );
        assert_eq!(interval_named("1d"), Some(CandleInterval::CandleInterval1d));
        assert_eq!(interval_named("2m"), None);
        assert_eq!(
            candles_topic("0xab", CandleInterval::CandleInterval4h),
            "candles:0xab:4h"
        );
        assert_eq!(length_ms(CandleInterval::CandleInterval4h), 14_400_000);
    }
}
