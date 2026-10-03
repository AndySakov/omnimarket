//! The feed: the single-threaded core that applies records to the read models and throttles the
//! messages they make (rule 6). The server runs it on the Kafka consumer's task and fans its
//! output out to connections.

use std::collections::{BTreeSet, VecDeque};

use proto::api::v1::{EngineStatus, delta};
use proto::pool::v1::PoolUpdate;
use proto::price::v1::PriceUpdate;
use proto::trade::v1::Trade;
use types::chain::Address;

use crate::candles::{Candles, INTERVALS, candles_topic};
use crate::discovery::{Discovery, DiscoveryConfig};
use crate::model::{ModelError, ReadModel, hex};
use crate::status::engine_status;
use crate::throttle::Throttle;

/// The `discovery` topic's name.
pub const DISCOVERY_TOPIC: &str = "discovery";

/// The `status` topic's name.
pub const STATUS_TOPIC: &str = "status";

/// A delta on its topic, ready for every connection subscribed to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Published {
    pub topic: String,
    /// How far its read model had got: a block number, or for candles the count of trades
    /// counted. A connection skips a delta no newer than its snapshot.
    pub as_of: u64,
    /// `None` when the topic changed too much to send as deltas: each subscriber gets a fresh
    /// snapshot instead.
    pub payload: Option<delta::Payload>,
}

/// The `token:<address>` topic's name.
pub fn token_topic(address: &str) -> String {
    format!("token:{address}")
}

#[derive(Default)]
pub struct Feed {
    model: ReadModel,
    throttle: Throttle,
    discovery: Discovery,
    pending: Pending,
    /// The latest `status.base` record's.
    status: Option<EngineStatus>,
    candles: Candles,
    /// Tokens whose candles took history since the last live trade. Their candle topics get a
    /// fresh snapshot with the next live trade, rather than a delta per trade of history.
    history_changed: BTreeSet<Address>,
}

/// The discovery read model's input, merged from the three topics in a fixed order, so its
/// output doesn't depend on how the consumer interleaves them. Each topic has one partition, so
/// its own order is fixed, but not quite block order: the engine publishes a pool, and its
/// trades, once it has proven or read the pool, up to tens of blocks after the block it was seen
/// in. So each record is placed by its topic's position when it arrives: the highest block the
/// topic has reached. Once every topic has reached past position N, nothing more can arrive at
/// N: its records are applied, pool updates first, then trades, then prices, each in its topic's
/// order. A topic that goes quiet holds the feed until it moves again.
#[derive(Default)]
struct Pending {
    /// Each with its position.
    pools: VecDeque<(u64, PoolUpdate)>,
    trades: VecDeque<(u64, Trade)>,
    prices: VecDeque<(u64, PriceUpdate)>,
    /// Each topic's position: pool updates, trades, prices.
    seen: [u64; 3],
}

impl Feed {
    pub fn new(discovery: DiscoveryConfig) -> Self {
        Self {
            discovery: Discovery::new(discovery),
            ..Self::default()
        }
    }

    /// Applies one `prices.base` record and returns what to publish: this record's tick, unless
    /// the throttle holds it, any held tick whose gap has passed, and the discovery rows the
    /// blocks it completes publish. Time is the record's block time, so a replay throttles exactly as the live
    /// run did.
    pub fn apply_price(&mut self, update: &PriceUpdate) -> Result<Vec<Published>, ModelError> {
        let now_ms = update.block_timestamp.saturating_mul(1000);
        let tick = self.model.apply_price(update)?;
        self.candles.apply_price(update)?;
        let ticks = self
            .throttle
            .offer(tick, now_ms)
            .into_iter()
            .chain(self.throttle.due(now_ms));
        let mut out: Vec<Published> = ticks
            .map(|tick| Published {
                topic: token_topic(&tick.token),
                as_of: tick.block_number,
                payload: Some(delta::Payload::TokenTick(tick)),
            })
            .collect();
        self.pending.seen[2] = self.pending.seen[2].max(update.block_number);
        let at = self.pending.seen[2];
        self.pending.prices.push_back((at, update.clone()));
        out.extend(self.release());
        Ok(out)
    }

    /// Applies one `status.base` record: it replaces the last, and goes out on `status`.
    pub fn apply_status(
        &mut self,
        record: &proto::status::v1::EngineStatus,
    ) -> Result<Vec<Published>, ModelError> {
        let status = engine_status(record)?;
        self.status = Some(status.clone());
        Ok(vec![Published {
            topic: STATUS_TOPIC.to_string(),
            as_of: status.head_block_number,
            payload: Some(delta::Payload::Status(status)),
        }])
    }

    /// Takes one `trades.base` record: the token's candles take it at once, and the discovery
    /// read model in its place in the merge. Candle topics that took history since the last
    /// live trade get their fresh snapshots first.
    pub fn apply_trade(&mut self, trade: &Trade) -> Vec<Published> {
        let mut out = self.history_snapshots();
        out.extend(self.candle_deltas(trade));
        self.pending.seen[1] = self.pending.seen[1].max(trade.block_number);
        let at = self.pending.seen[1];
        self.pending.trades.push_back((at, trade.clone()));
        out.extend(self.release());
        out
    }

    /// Takes one `trade-history.base` record: a trade from before its pool was tracked. It
    /// counts toward candles only, never discovery's live windows.
    pub fn apply_history(&mut self, trade: &Trade) -> Result<(), ModelError> {
        if let Some(counted) = self.candles.apply_trade(trade)? {
            self.history_changed.insert(counted.token);
        }
        Ok(())
    }

    /// The candles a live trade changed, one per interval, if its pool counts toward its
    /// token's candles. A trade counted before changes nothing.
    fn candle_deltas(&mut self, trade: &Trade) -> Vec<Published> {
        let counted = match self.candles.apply_trade(trade) {
            Ok(Some(counted)) => counted,
            Ok(None) => return Vec::new(),
            Err(error) => {
                tracing::warn!(?error, "a trade the candles can't take");
                return Vec::new();
            }
        };
        if !self.candles.merges(counted.token, counted.pool) {
            return Vec::new();
        }
        let address = hex(counted.token.as_slice());
        let as_of = self.candles.version();
        INTERVALS
            .iter()
            .filter_map(|(interval, _, length)| {
                let open = counted.time_ms / length * length;
                let candle = self.candles.candle(counted.token, *interval, open)?;
                Some(Published {
                    topic: candles_topic(&address, *interval),
                    as_of,
                    payload: Some(delta::Payload::Candle(candle)),
                })
            })
            .collect()
    }

    /// A fresh snapshot for every candle topic of each token that took history.
    fn history_snapshots(&mut self) -> Vec<Published> {
        let as_of = self.candles.version();
        std::mem::take(&mut self.history_changed)
            .into_iter()
            .flat_map(|token| {
                let address = hex(token.as_slice());
                INTERVALS.iter().map(move |(interval, _, _)| Published {
                    topic: candles_topic(&address, *interval),
                    as_of,
                    payload: None,
                })
            })
            .collect()
    }

    /// Takes one `pool-updates.base` record for the discovery read model.
    pub fn apply_pool_update(&mut self, update: &PoolUpdate) -> Vec<Published> {
        self.pending.seen[0] = self.pending.seen[0].max(update.block_number);
        let at = self.pending.seen[0];
        self.pending.pools.push_back((at, update.clone()));
        self.release()
    }

    /// Applies every whole position to the discovery read model: those before the lowest
    /// position of the three topics.
    fn release(&mut self) -> Vec<Published> {
        let watermark = self.pending.seen.iter().copied().min().unwrap_or_default();
        let mut out = Vec::new();
        loop {
            let pending = &mut self.pending;
            let next = [
                pending.pools.front().map(|r| r.0),
                pending.trades.front().map(|r| r.0),
                pending.prices.front().map(|r| r.0),
            ]
            .into_iter()
            .flatten()
            .min();
            let Some(block) = next.filter(|&b| b < watermark) else {
                // Every block before the watermark is whole: publish the last one now rather
                // than with the next block's first record.
                let complete = watermark.saturating_sub(1);
                out.extend(self.discovery_deltas(|d| d.complete(complete)));
                return out;
            };
            while let Some(update) = pop_at(&mut self.pending.pools, block) {
                out.extend(self.discovery_deltas(|d| d.apply_pool_update(&update)));
            }
            while let Some(trade) = pop_at(&mut self.pending.trades, block) {
                out.extend(self.discovery_deltas(|d| d.apply_trade(&trade)));
            }
            while let Some(price) = pop_at(&mut self.pending.prices, block) {
                out.extend(self.discovery_deltas(|d| d.apply_price(&price)));
            }
        }
    }

    pub fn model(&self) -> &ReadModel {
        &self.model
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    pub fn candles(&self) -> &Candles {
        &self.candles
    }

    /// The engine's latest status; `None` until its first.
    pub fn status(&self) -> Option<&EngineStatus> {
        self.status.as_ref()
    }

    fn discovery_deltas(
        &mut self,
        apply: impl FnOnce(&mut Discovery) -> Vec<delta::Payload>,
    ) -> Vec<Published> {
        let payloads = apply(&mut self.discovery);
        let as_of = self.discovery.published_block();
        payloads
            .into_iter()
            .map(|payload| Published {
                topic: DISCOVERY_TOPIC.to_string(),
                as_of,
                payload: Some(payload),
            })
            .collect()
    }
}

/// The queue's front record, if it's at `position`.
fn pop_at<T>(queue: &mut VecDeque<(u64, T)>, position: u64) -> Option<T> {
    if queue.front().map(|r| r.0) == Some(position) {
        queue.pop_front().map(|r| r.1)
    } else {
        None
    }
}
