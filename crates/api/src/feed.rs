//! The feed: the single-threaded core that applies records to the read model and throttles the
//! ticks it makes (rule 6). The server runs it on the Kafka consumer's task and fans its output
//! out to connections.

use proto::api::v1::TokenTick;
use proto::price::v1::PriceUpdate;

use crate::model::{ModelError, ReadModel};
use crate::throttle::Throttle;

/// A tick on its topic, ready for every connection subscribed to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Published {
    pub topic: String,
    pub tick: TokenTick,
}

/// The `token:<address>` topic's name.
pub fn token_topic(address: &str) -> String {
    format!("token:{address}")
}

#[derive(Default)]
pub struct Feed {
    model: ReadModel,
    throttle: Throttle,
}

impl Feed {
    /// Applies one `prices.base` record and returns the ticks to publish: this record's, unless
    /// the throttle holds it, and any held tick whose gap has passed. Time is the record's block
    /// time, so a replay throttles exactly as the live run did.
    pub fn apply_price(&mut self, update: &PriceUpdate) -> Result<Vec<Published>, ModelError> {
        let now_ms = update.block_timestamp.saturating_mul(1000);
        let tick = self.model.apply_price(update)?;
        let ticks = self
            .throttle
            .offer(tick, now_ms)
            .into_iter()
            .chain(self.throttle.due(now_ms));
        Ok(ticks
            .map(|tick| Published {
                topic: token_topic(&tick.token),
                tick,
            })
            .collect())
    }

    pub fn model(&self) -> &ReadModel {
        &self.model
    }
}
