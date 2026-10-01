//! The feed: the single-threaded core that applies records to the read models and throttles the
//! messages they make (rule 6). The server runs it on the Kafka consumer's task and fans its
//! output out to connections.

use proto::api::v1::delta;
use proto::pool::v1::PoolUpdate;
use proto::price::v1::PriceUpdate;
use proto::trade::v1::Trade;

use crate::discovery::{Discovery, DiscoveryConfig};
use crate::model::{ModelError, ReadModel};
use crate::throttle::Throttle;

/// The `discovery` topic's name.
pub const DISCOVERY_TOPIC: &str = "discovery";

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
}

impl Feed {
    pub fn new(discovery: DiscoveryConfig) -> Self {
        Self {
            discovery: Discovery::new(discovery),
            ..Self::default()
        }
    }

    /// Applies one `prices.base` record and returns what to publish: this record's tick, unless
    /// the throttle holds it, any held tick whose gap has passed, and the discovery rows a new
    /// block closes. Time is the record's block time, so a replay throttles exactly as the live
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
        out.extend(self.discovery_deltas(|d| d.apply_price(update)));
        Ok(out)
    }

    /// Applies one `trades.base` record to the discovery read model.
    pub fn apply_trade(&mut self, trade: &Trade) -> Vec<Published> {
        self.discovery_deltas(|d| d.apply_trade(trade))
    }

    /// Applies one `pool-updates.base` record to the discovery read model.
    pub fn apply_pool_update(&mut self, update: &PoolUpdate) -> Vec<Published> {
        self.discovery_deltas(|d| d.apply_pool_update(update))
    }

    pub fn model(&self) -> &ReadModel {
        &self.model
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
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
