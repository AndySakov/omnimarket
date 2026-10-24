//! Swap quotes (#83): every tracked pool's state, mirrored from `pool-updates.base`, and the
//! quotes `POST /v1/quotes` serves on it with `routing`'s exact math (D21). Fee (D28), slippage
//! defaults (D27) and gas are applied here; the route is the best net of all three.

use std::collections::{BTreeMap, VecDeque};

use alloy_primitives::{U256, address};
use proto::api::v1::{Fee, Quote, QuoteRequest, RouteLeg, Slippage, TokenRef};
use proto::lineage::v1::Lineage;
use proto::pool::v1::{PoolUpdate, pool_key, pool_state};
use proto::price::v1::PriceUpdate;
use routing::{Pool, PoolKind, Route};
use types::LineageId;
use types::chain::Address;

use crate::model::{decimal, hex, scaled_decimal};

const WETH: Address = address!("4200000000000000000000000000000000000006");
const USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
const USDT: Address = address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2");
/// The tokens our fee is taken in (D28), with their symbols and decimals. A trade is quoted
/// only if one side is one of them.
const QUOTE_ASSETS: [(Address, &str, u32); 3] =
    [(WETH, "WETH", 18), (USDC, "USDC", 6), (USDT, "USDT", 6)];
/// Two-hop routes go through these.
const HUBS: [Address; 2] = [WETH, USDC];
const FEE_BPS: u32 = 100;
/// A quote is good for about 2 blocks (D27: re-quoted at send time anyway).
const VALID_MS: u64 = 4_000;
/// Rough gas per swap, until #84 simulates each trade: a transaction's base and each hop's.
const GAS_BASE: u64 = 60_000;
const GAS_V2_HOP: u64 = 90_000;
const GAS_V3_HOP: u64 = 130_000;
/// Base's typical gas price, 0.01 gwei.
const GAS_PRICE_WEI: u64 = 10_000_000;
/// Quotes kept for repeats of their client request ID, until they expire.
const KEPT: usize = 10_000;
/// The longest client request ID taken.
const MAX_REQUEST_ID: usize = 64;

/// What a token's latest price update says.
#[derive(Clone, Debug, Default)]
struct Token {
    symbol: String,
    name: String,
    decimals: Option<u32>,
    price_usd: f64,
    thin: bool,
}

#[derive(Default)]
pub struct Quotes {
    pools: BTreeMap<Address, Pool>,
    /// Each pool's latest update: its block and lineage ID.
    updated: BTreeMap<Address, (u64, Vec<u8>)>,
    tokens: BTreeMap<Address, Token>,
    /// The latest price update's block time, ms.
    head_ms: u64,
    /// By client request ID: the request, and the quote it got.
    issued: BTreeMap<String, (QuoteRequest, Quote)>,
    issued_order: VecDeque<String>,
    /// Each kept quote's client request ID, by quote ID.
    by_quote_id: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuoteError {
    /// A field that doesn't parse, named.
    Bad(&'static str),
    /// Neither side is WETH, USDC or USDT, so there's nothing to take the fee in.
    NoQuoteAsset,
    /// The token's decimals aren't known yet.
    UnknownToken,
    /// No tracked pool, or pair of pools, can take the amount.
    NoRoute,
}

impl std::fmt::Display for QuoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bad(field) => write!(f, "{field} doesn't parse"),
            Self::NoQuoteAsset => write!(f, "one side must be WETH, USDC or USDT"),
            Self::UnknownToken => write!(f, "a token's decimals aren't known yet"),
            Self::NoRoute => write!(f, "no tracked pool can take this trade"),
        }
    }
}

impl Quotes {
    /// Takes one `pool-updates.base` record: a discovery creates the pool, a later update
    /// replaces its state and the ticks it touched. An update for a pool never discovered, or
    /// one that doesn't decode, is skipped.
    pub fn apply_pool_update(&mut self, update: &PoolUpdate) {
        let Ok(address) = Address::try_from(update.pool.as_slice()) else {
            return;
        };
        let Some(state) = update.after.as_ref().and_then(|s| s.state.as_ref()) else {
            return;
        };
        if let Some(key) = &update.key {
            let (Ok(token0), Ok(token1)) = (
                Address::try_from(key.token0.as_slice()),
                Address::try_from(key.token1.as_slice()),
            ) else {
                return;
            };
            let kind = match key.venue() {
                pool_key::Venue::UniswapV2 => PoolKind::V2 {
                    reserve0: U256::ZERO,
                    reserve1: U256::ZERO,
                },
                pool_key::Venue::UniswapV3 => PoolKind::V3(venues::v3::Pool::new(
                    address,
                    token0,
                    token1,
                    key.fee_pips,
                    key.tick_spacing,
                )),
                pool_key::Venue::Unspecified => return,
            };
            self.pools.insert(
                address,
                Pool {
                    address,
                    token0,
                    token1,
                    kind,
                },
            );
        }
        let Some(pool) = self.pools.get_mut(&address) else {
            return;
        };
        match (&mut pool.kind, state) {
            (PoolKind::V2 { reserve0, reserve1 }, pool_state::State::V2(r)) => {
                *reserve0 = uint(&r.reserve0);
                *reserve1 = uint(&r.reserve1);
            }
            (PoolKind::V3(pool), pool_state::State::V3(s)) => {
                pool.price = s.initialized.then(|| venues::v3::Price {
                    sqrt_price_x96: uint(&s.sqrt_price_x96),
                    tick: s.tick,
                });
                pool.liquidity = uint(&s.liquidity).saturating_to();
                for t in &s.ticks {
                    let gross: u128 = uint(&t.liquidity_gross).saturating_to();
                    if gross == 0 {
                        pool.ticks.remove(&t.tick);
                    } else {
                        let net = <[u8; 16]>::try_from(t.liquidity_net.as_slice())
                            .map_or(0, i128::from_be_bytes);
                        pool.ticks
                            .insert(t.tick, venues::v3::TickLiquidity { gross, net });
                    }
                }
            }
            _ => return,
        }
        let id = update
            .lineage
            .as_ref()
            .map(|l| l.id.clone())
            .unwrap_or_default();
        self.updated.insert(address, (update.block_number, id));
    }

    /// Takes a token's symbol, decimals, USD price and thinness from its latest price update.
    pub fn apply_price(&mut self, update: &PriceUpdate) {
        let Ok(token) = Address::try_from(update.token.as_slice()) else {
            return;
        };
        self.head_ms = self
            .head_ms
            .max(update.block_timestamp.saturating_mul(1000));
        let metadata = update.metadata.clone().unwrap_or_default();
        self.tokens.insert(
            token,
            Token {
                symbol: metadata.symbol.unwrap_or_default(),
                name: metadata.name.unwrap_or_default(),
                decimals: metadata.decimals,
                price_usd: update.price_usd,
                thin: update.thin,
            },
        );
    }

    /// The pools quotes are priced on.
    pub fn pools(&self) -> &BTreeMap<Address, Pool> {
        &self.pools
    }

    /// Quotes `request` on the pools' latest state, or returns the quote the same request,
    /// with the same client request ID, already got while it's unexpired. A different request
    /// reusing the ID gets its own quote, never another's.
    pub fn quote(&mut self, request: &QuoteRequest) -> Result<Quote, QuoteError> {
        if request.client_request_id.len() > MAX_REQUEST_ID {
            return Err(QuoteError::Bad("client_request_id"));
        }
        while let Some(oldest) = self.issued_order.front() {
            let expired = self
                .issued
                .get(oldest)
                .is_none_or(|(_, q)| q.expires_at_ms <= self.head_ms);
            if !expired && self.issued_order.len() < KEPT {
                break;
            }
            if let Some(oldest) = self.issued_order.pop_front()
                && let Some((_, quote)) = self.issued.remove(&oldest)
            {
                self.by_quote_id.remove(&quote.quote_id);
            }
        }
        if let Some((asked, quote)) = self.issued.get(&request.client_request_id)
            && asked == request
        {
            return Ok(quote.clone());
        }
        let quote = self.price(request)?;
        if !request.client_request_id.is_empty() {
            self.by_quote_id
                .insert(quote.quote_id.clone(), request.client_request_id.clone());
            match self.issued.insert(
                request.client_request_id.clone(),
                (request.clone(), quote.clone()),
            ) {
                Some((_, replaced)) => {
                    self.by_quote_id.remove(&replaced.quote_id);
                    self.by_quote_id
                        .insert(quote.quote_id.clone(), request.client_request_id.clone());
                }
                None => self
                    .issued_order
                    .push_back(request.client_request_id.clone()),
            }
        }
        Ok(quote)
    }

    /// A kept quote by its ID, with the request it answered, while it's unexpired.
    pub fn issued(&self, quote_id: &str) -> Option<(&QuoteRequest, &Quote)> {
        let (request, quote) = self.issued.get(self.by_quote_id.get(quote_id)?)?;
        (quote.expires_at_ms > self.head_ms).then_some((request, quote))
    }

    /// `request` priced again on the latest state, without keeping it: a trade's re-quote at
    /// send time (D27).
    pub fn price(&self, request: &QuoteRequest) -> Result<Quote, QuoteError> {
        let token_in = parse_address(&request.token_in).ok_or(QuoteError::Bad("token_in"))?;
        let token_out = parse_address(&request.token_out).ok_or(QuoteError::Bad("token_out"))?;
        if token_in == token_out {
            return Err(QuoteError::Bad("token_out"));
        }
        let (decimals_in, decimals_out) = (
            self.decimals(token_in).ok_or(QuoteError::UnknownToken)?,
            self.decimals(token_out).ok_or(QuoteError::UnknownToken)?,
        );
        let amount_in = parse_amount(&request.amount_in, decimals_in)
            .filter(|a| !a.is_zero())
            .ok_or(QuoteError::Bad("amount_in"))?;
        // D28: from the input on a buy (the input is a quote asset), else from the proceeds.
        let fee_from_input = is_quote_asset(token_in);
        if !fee_from_input && !is_quote_asset(token_out) {
            return Err(QuoteError::NoQuoteAsset);
        }
        let fee_of = |amount: U256| amount * U256::from(FEE_BPS) / U256::from(10_000u64);
        let routed_in = if fee_from_input {
            amount_in - fee_of(amount_in)
        } else {
            amount_in
        };
        let gas_out = |route: &Route| self.gas_in(route, token_out, decimals_out);
        let route = routing::routes(&self.pools, token_in, token_out, routed_in, &HUBS)
            .into_iter()
            .max_by_key(|route| route.amount_out().saturating_sub(gas_out(route)))
            .ok_or(QuoteError::NoRoute)?;
        let routed_out = route.amount_out();
        let (fee, amount_out) = if fee_from_input {
            (fee_of(amount_in), routed_out)
        } else {
            (fee_of(routed_out), routed_out - fee_of(routed_out))
        };
        let (fee_token, fee_decimals) = if fee_from_input {
            (token_in, decimals_in)
        } else {
            (token_out, decimals_out)
        };
        let slippage = self.slippage(request, token_in, token_out);
        let min_out =
            amount_out * U256::from(10_000 - slippage.bps.min(10_000)) / U256::from(10_000u64);
        let block_number = route
            .legs
            .iter()
            .filter_map(|l| self.updated.get(&l.pool).map(|(b, _)| *b))
            .max()
            .unwrap_or_default();
        let gas_units = gas_units(&self.pools, &route);
        let gas_native = U256::from(gas_units) * U256::from(GAS_PRICE_WEI);
        let whole = |amount: U256, decimals: u32| -> f64 {
            scaled_decimal(&amount.to_be_bytes::<32>(), decimals)
                .parse()
                .unwrap_or(f64::NAN)
        };
        let usd = |token: Address| self.usd(token);
        let price = whole(amount_out, decimals_out) / whole(amount_in, decimals_in);
        // Against the display price (D18), before our fee: what the pools cost.
        let impact = match (usd(token_in), usd(token_out)) {
            (Some(usd_in), Some(usd_out)) => {
                let executed = whole(routed_out, decimals_out) / whole(routed_in, decimals_in);
                decimal((1.0 - executed / (usd_in / usd_out)) * 100.0)
            }
            _ => String::new(),
        };
        let gas_usd = usd(WETH).map_or_else(String::new, |p| decimal(whole(gas_native, 18) * p));
        let mut key = request.client_request_id.as_bytes().to_vec();
        key.extend(request.chain_id.to_le_bytes());
        key.extend(block_number.to_le_bytes());
        key.extend(token_in.as_slice());
        key.extend(token_out.as_slice());
        key.extend(amount_in.to_be_bytes::<32>());
        let id = LineageId::from_natural_key("quote", &key);
        Ok(Quote {
            lineage: Some(Lineage {
                id: id.as_bytes().to_vec(),
                caused_by: route
                    .legs
                    .iter()
                    .filter_map(|l| self.updated.get(&l.pool).map(|(_, id)| id.clone()))
                    .collect(),
            }),
            quote_id: hex(id.as_bytes()),
            client_request_id: request.client_request_id.clone(),
            chain_id: request.chain_id,
            block_number,
            expires_at_ms: self.head_ms + VALID_MS,
            token_in: Some(self.token_ref(request.chain_id, token_in)),
            token_out: Some(self.token_ref(request.chain_id, token_out)),
            amount_in: exact(amount_in, decimals_in),
            amount_out: exact(amount_out, decimals_out),
            min_amount_out: exact(min_out, decimals_out),
            price_impact_pct: impact,
            price: decimal(price),
            fee: Some(Fee {
                fee_bps: FEE_BPS,
                amount: exact(fee, fee_decimals),
                token: Some(self.token_ref(request.chain_id, fee_token)),
                value_usd: usd(fee_token)
                    .map_or_else(String::new, |p| decimal(whole(fee, fee_decimals) * p)),
            }),
            slippage: Some(slippage),
            route: route
                .legs
                .iter()
                .map(|leg| {
                    let pool = &self.pools[&leg.pool];
                    let (venue, fee_bps) = match &pool.kind {
                        PoolKind::V2 { .. } => ("uniswap-v2", 30),
                        PoolKind::V3(p) => ("uniswap-v3", p.fee / 100),
                    };
                    let decimals = |t| self.decimals(t).unwrap_or_default();
                    RouteLeg {
                        pool: hex(leg.pool.as_slice()),
                        venue: venue.into(),
                        pool_fee_bps: fee_bps,
                        token_in: Some(self.token_ref(request.chain_id, leg.token_in)),
                        token_out: Some(self.token_ref(request.chain_id, leg.token_out)),
                        amount_in: exact(leg.amount_in, decimals(leg.token_in)),
                        amount_out: exact(leg.amount_out, decimals(leg.token_out)),
                    }
                })
                .collect(),
            gas_estimate_native: exact(gas_native, 18),
            gas_estimate_usd: gas_usd,
            intent_typed_data: String::new(),
        })
    }

    /// A token's display price in USD: its latest price update's, or $1 for a reference
    /// stablecoin, which is pinned and publishes none (D19).
    pub fn usd(&self, token: Address) -> Option<f64> {
        if token == USDC || token == USDT {
            return Some(1.0);
        }
        self.tokens
            .get(&token)
            .map(|t| t.price_usd)
            .filter(|p| *p > 0.0)
    }

    fn decimals(&self, token: Address) -> Option<u32> {
        QUOTE_ASSETS
            .iter()
            .find(|(t, _, _)| *t == token)
            .map(|(_, _, d)| *d)
            .or_else(|| self.tokens.get(&token)?.decimals)
    }

    fn token_ref(&self, chain_id: u64, token: Address) -> TokenRef {
        let known = self.tokens.get(&token).cloned().unwrap_or_default();
        let symbol = QUOTE_ASSETS
            .iter()
            .find(|(t, _, _)| *t == token)
            .map_or(known.symbol, |(_, s, _)| s.to_string());
        TokenRef {
            chain_id,
            address: hex(token.as_slice()),
            symbol,
            name: known.name,
            decimals: self.decimals(token).unwrap_or_default(),
        }
    }

    /// The route's gas in `token_out`'s base units, at the display prices; nothing when they
    /// aren't known.
    fn gas_in(&self, route: &Route, token_out: Address, decimals_out: u32) -> U256 {
        let wei = U256::from(gas_units(&self.pools, route)) * U256::from(GAS_PRICE_WEI);
        if token_out == WETH {
            return wei;
        }
        let (Some(weth), Some(out)) = (self.usd(WETH), self.usd(token_out)) else {
            return U256::ZERO;
        };
        let whole_eth = f64::from(wei) / 1e18;
        let units = whole_eth * weth / out * 10f64.powi(decimals_out as i32);
        U256::from(units.max(0.0) as u128)
    }

    /// D27's default for the pair, unless the request set one.
    fn slippage(&self, request: &QuoteRequest, token_in: Address, token_out: Address) -> Slippage {
        if let Some(bps) = request.slippage_bps {
            return Slippage {
                bps,
                reason: format!("set by you: {}", percent(bps)),
                user_set: true,
            };
        }
        let (bps, situation) = if is_quote_asset(token_in) && is_quote_asset(token_out) {
            (50, "major pair")
        } else {
            let token = if is_quote_asset(token_in) {
                token_out
            } else {
                token_in
            };
            match self.tokens.get(&token) {
                Some(t) if !t.thin => (300, "established pair"),
                _ => (1500, "new or thin pair"),
            }
        };
        Slippage {
            bps,
            reason: format!("{situation}: {}", percent(bps)),
            user_set: false,
        }
    }
}

fn gas_units(pools: &BTreeMap<Address, Pool>, route: &Route) -> u64 {
    GAS_BASE
        + route
            .legs
            .iter()
            .map(|leg| match pools.get(&leg.pool).map(|p| &p.kind) {
                Some(PoolKind::V2 { .. }) => GAS_V2_HOP,
                _ => GAS_V3_HOP,
            })
            .sum::<u64>()
}

fn is_quote_asset(token: Address) -> bool {
    QUOTE_ASSETS.iter().any(|(t, _, _)| *t == token)
}

/// 1500 bps as "15%", 50 as "0.5%".
fn percent(bps: u32) -> String {
    format!("{}%", decimal(f64::from(bps) / 100.0))
}

fn uint(bytes: &[u8]) -> U256 {
    U256::try_from_be_slice(bytes).unwrap_or_default()
}

fn parse_address(text: &str) -> Option<Address> {
    text.strip_prefix("0x")?.parse().ok()
}

/// A whole-token decimal string in base units. `None` for more fractional digits than the
/// token has, or anything but digits and one point.
fn parse_amount(text: &str, decimals: u32) -> Option<U256> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && fraction.is_empty()
        || fraction.len() > decimals as usize
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction:0<width$}", width = decimals as usize);
    U256::from_str_radix(if digits.is_empty() { "0" } else { &digits }, 10).ok()
}

fn exact(amount: U256, decimals: u32) -> String {
    scaled_decimal(&amount.to_be_bytes::<32>(), decimals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_parse_in_whole_tokens_to_base_units() {
        assert_eq!(parse_amount("1.5", 6), Some(U256::from(1_500_000u64)));
        assert_eq!(
            parse_amount("2", 18),
            Some(U256::from(2u64) * U256::from(10u64).pow(U256::from(18u64)))
        );
        assert_eq!(parse_amount(".25", 2), Some(U256::from(25u64)));
        assert_eq!(parse_amount("0.0000001", 6), None);
        assert_eq!(parse_amount("1e3", 6), None);
        assert_eq!(parse_amount("-1", 6), None);
        assert_eq!(parse_amount("", 6), None);
    }

    #[test]
    fn slippage_reads_as_a_percentage() {
        assert_eq!(percent(1500), "15%");
        assert_eq!(percent(50), "0.5%");
    }
}
