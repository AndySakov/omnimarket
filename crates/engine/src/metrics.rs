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
