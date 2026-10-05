//! The feed: the single-threaded core that applies records to the read models and throttles the
//! messages they make (rule 6). The server runs it on the Kafka consumer's task and fans its
//! output out to connections.

use std::collections::VecDeque;

use proto::api::v1::{EngineStatus, delta};
use proto::pool::v1::PoolUpdate;
use proto::price::v1::PriceUpdate;
use proto::trade::v1::Trade;

use crate::discovery::{Discovery, DiscoveryConfig};
use crate::model::{ModelError, ReadModel};
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
    /// The block the delta is as of; a connection skips one no newer than its snapshot.
    pub block_number: u64,
    pub payload: delta::Payload,
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
        let ticks = self
            .throttle
            .offer(tick, now_ms)
            .into_iter()
            .chain(self.throttle.due(now_ms));
        let mut out: Vec<Published> = ticks
            .map(|tick| Published {
                topic: token_topic(&tick.token),
                block_number: tick.block_number,
                payload: delta::Payload::TokenTick(tick),
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
            block_number: status.head_block_number,
            payload: delta::Payload::Status(status),
        }])
    }

    /// Takes one `trades.base` record for the discovery read model.
    pub fn apply_trade(&mut self, trade: &Trade) -> Vec<Published> {
        self.pending.seen[1] = self.pending.seen[1].max(trade.block_number);
        let at = self.pending.seen[1];
        self.pending.trades.push_back((at, trade.clone()));
        self.release()
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

    /// The engine's latest status; `None` until its first.
    pub fn status(&self) -> Option<&EngineStatus> {
        self.status.as_ref()
    }

    fn discovery_deltas(
        &mut self,
        apply: impl FnOnce(&mut Discovery) -> Vec<delta::Payload>,
    ) -> Vec<Published> {
        let payloads = apply(&mut self.discovery);
        let block_number = self.discovery.published_block();
        payloads
            .into_iter()
            .map(|payload| Published {
                topic: DISCOVERY_TOPIC.to_string(),
                block_number,
                payload,
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
