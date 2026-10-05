//! The engine's metrics: what it has published and where its latest status says it is. They're
//! read off the outbox, outside the core, so they can't change what the core decides.

use std::sync::{Arc, Mutex, MutexGuard};

use telemetry::metrics::{Metric, render};

use crate::outbox::{Outbox, PoolUpdate, PriceUpdate};
use crate::status::EngineStatus;
use crate::trades::Trade;

#[derive(Default)]
struct Counts {
    status: Option<EngineStatus>,
    pool_updates: u64,
    trades: u64,
    prices: u64,
}

/// Shared between the outbox the core publishes through and whatever scrapes it.
#[derive(Clone, Default)]
pub struct EngineMetrics {
    counts: Arc<Mutex<Counts>>,
}

impl EngineMetrics {
    fn counts(&self) -> MutexGuard<'_, Counts> {
        self.counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// An outbox that counts what passes through it, then hands it to `inner`.
    pub fn outbox(&self, inner: Box<dyn Outbox>) -> Box<dyn Outbox> {
        Box::new(Metered {
            inner,
            metrics: self.clone(),
        })
    }

    /// The text a scrape returns. Before the first block there are only the counters.
    pub fn render(&self) -> String {
        let counts = self.counts();
        let mut metrics = vec![
            Metric::counter(
                "omnimarket_engine_records_published_total",
                "Records the engine published, by topic.",
            )
            .labelled(&[("topic", crate::POOL_UPDATES_TOPIC)], counts.pool_updates)
            .labelled(&[("topic", crate::TRADES_TOPIC)], counts.trades)
            .labelled(&[("topic", crate::PRICES_TOPIC)], counts.prices),
        ];
        if let Some(s) = &counts.status {
            metrics.extend([
                Metric::gauge(
                    "omnimarket_engine_head_block",
                    "The block the engine last applied.",
                )
                .value(s.block_number),
                Metric::gauge(
                    "omnimarket_engine_lag_ms",
                    "From the head block's timestamp to its arrival at the core.",
                )
                .value(s.lag_ms),
                Metric::gauge(
                    "omnimarket_engine_pools",
                    "Pools the engine tracks with state, by venue.",
                )
                .labelled(&[("venue", "uniswap-v2")], s.v2_pools)
                .labelled(&[("venue", "uniswap-v3")], s.v3_pools),
                Metric::counter(
                    "omnimarket_engine_shadow_checks_total",
                    "Shadow state checks answered since the core started.",
                )
                .value(s.shadow_checks),
                Metric::counter(
                    "omnimarket_engine_shadow_check_mismatches_total",
                    "Shadow state checks that disagreed with the chain.",
                )
                .value(s.shadow_check_mismatches),
                Metric::gauge(
                    "omnimarket_engine_uptime_ms",
                    "From the core's first block to the latest.",
                )
                .value(s.uptime_ms),
            ]);
            // Absent when the recording doesn't hold the chain's head.
            if let Some(blocks) = s.lag_blocks {
                metrics.push(
                    Metric::gauge(
                        "omnimarket_engine_lag_blocks",
                        "The chain's latest block when the follower read the head block, minus the head block.",
                    )
                    .value(blocks),
                );
            }
        }
        render(&metrics)
    }
}

struct Metered {
    inner: Box<dyn Outbox>,
    metrics: EngineMetrics,
}

impl Outbox for Metered {
    fn publish(&self, update: &PoolUpdate) {
        self.inner.publish(update);
        self.metrics.counts().pool_updates += 1;
    }

    fn publish_trade(&self, trade: &Trade) {
        self.inner.publish_trade(trade);
        self.metrics.counts().trades += 1;
    }

    fn publish_price(&self, update: &PriceUpdate) {
        self.inner.publish_price(update);
        self.metrics.counts().prices += 1;
    }

    fn publish_status(&self, status: &EngineStatus) {
        self.inner.publish_status(status);
        self.metrics.counts().status = Some(status.clone());
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::U256;
    use pricing::TokenMetadata;
    use types::LineageId;
    use types::chain::{Address, B256};
    use venues::v2::Reserves;

    use super::*;
    use crate::outbox::{InMemoryOutbox, PoolState};
    use crate::trades::{Side, Venue};

    fn id() -> LineageId {
        LineageId::from_natural_key("test", b"")
    }

    fn trade() -> Trade {
        Trade {
            id: id(),
            caused_by: id(),
            chain_id: 8453,
            pool: Address::ZERO,
            venue: Venue::UniswapV2,
            token: Address::ZERO,
            quote: Address::ZERO,
            side: Side::Buy,
            token_amount: U256::ZERO,
            quote_amount: U256::ZERO,
            price_e36: None,
            sender: Address::ZERO,
            recipient: Address::ZERO,
            tx_hash: B256::ZERO,
            block_number: 1,
            block_hash: B256::ZERO,
            block_timestamp: 1,
            log_index: 0,
        }
    }

    fn price() -> PriceUpdate {
        PriceUpdate {
            id: id(),
            caused_by: vec![],
            chain_id: 8453,
            token: Address::ZERO,
            block_number: 1,
            block_hash: B256::ZERO,
            block_timestamp: 1,
            metadata: TokenMetadata::default(),
            price_usd: 1.0,
            depth_usd: 1.0,
            thin: false,
            main_pool: Address::ZERO,
            quote_token: Address::ZERO,
            price_in_quote: 1.0,
            fdv_usd: None,
            pools: vec![],
        }
    }

    fn update() -> PoolUpdate {
        PoolUpdate {
            id: id(),
            caused_by: id(),
            chain_id: 8453,
            pool: Address::ZERO,
            block_number: 1,
            block_hash: B256::ZERO,
            log_index: 0,
            before: None,
            after: PoolState::V2(Reserves {
                reserve0: 1,
                reserve1: 1,
            }),
        }
    }

    #[test]
    fn each_record_is_counted_by_topic_and_passed_on() {
        let inner = InMemoryOutbox::default();
        let metrics = EngineMetrics::default();
        let outbox = metrics.outbox(Box::new(inner.clone()));
        outbox.publish(&update());
        for _ in 0..2 {
            outbox.publish_trade(&trade());
        }
        for _ in 0..3 {
            outbox.publish_price(&price());
        }
        let text = metrics.render();
        for line in [
            "omnimarket_engine_records_published_total{topic=\"pool-updates.base\"} 1\n",
            "omnimarket_engine_records_published_total{topic=\"trades.base\"} 2\n",
            "omnimarket_engine_records_published_total{topic=\"prices.base\"} 3\n",
        ] {
            assert!(text.contains(line), "{line:?} missing from\n{text}");
        }
        assert_eq!(
            (
                inner.updates().len(),
                inner.trades().len(),
                inner.prices().len()
            ),
            (1, 2, 3)
        );
    }
}
