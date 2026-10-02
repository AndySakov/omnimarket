//! The engine's state machine: blocks and call answers in, calls and pool updates out. No
//! I/O and no waiting, so every step is a plain function call.

use std::collections::VecDeque;

use pricing::PricingConfig;
use prost::Message as _;
use types::Timestamp;
use types::chain::{Address, B256, Block, CallResult, EthCall};
use venues::v2::Deployment;

use crate::outbox::{PoolUpdate, PriceUpdate};
use crate::prices::{Coverage, Prices};
use crate::status::EngineStatus;
use crate::trades::Trade;
use crate::v2::V2Pools;
use crate::v3::V3Pools;

/// Base's quote assets (D18, D102): the reference stablecoins, then the native token, so a
/// WETH/USDC trade is WETH against USDC. Symbols read from each contract, 2026-10-01.
pub const BASE_QUOTE_ASSETS: [types::chain::Address; 3] = [
    // USDC
    alloy_primitives::address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
    // USDT
    alloy_primitives::address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2"),
    // WETH
    alloy_primitives::address!("4200000000000000000000000000000000000006"),
];

/// How many recent block hashes the engine keeps, to find where a reorg forked.
const RECENT_BLOCKS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub chain_id: u64,
    pub v2: Deployment,
    pub v3: venues::v3::Deployment,
    /// Every this many blocks, compare a sample of pools with the chain (shadow state check).
    pub check_every: Option<u64>,
    /// How many pools each check samples.
    pub check_sample: usize,
    /// The tokens trade records are quoted in, most preferred first (D102).
    pub quote_assets: Vec<types::chain::Address>,
    /// `None`: no pricing, as recordings made before it existed expect.
    pub pricing: Option<PricingConfig>,
}

impl EngineConfig {
    /// Proto bytes, recorded first in the input log.
    pub fn encode(&self) -> Vec<u8> {
        proto::engine::v1::EngineConfig {
            chain_id: self.chain_id,
            v2_factory: self.v2.factory.to_vec(),
            v2_init_code_hash: self.v2.init_code_hash.to_vec(),
            v3_factory: self.v3.factory.to_vec(),
            v3_init_code_hash: self.v3.init_code_hash.to_vec(),
            v3_tick_lens: self.v3.tick_lens.to_vec(),
            check_every: self.check_every,
            check_sample: self.check_sample as u64,
            quote_assets: self.quote_assets.iter().map(|a| a.to_vec()).collect(),
            pricing: self.pricing.as_ref().map(pricing_to_proto),
        }
        .encode_to_vec()
    }

    /// `None` if the bytes aren't a recorded config.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let config = proto::engine::v1::EngineConfig::decode(bytes).ok()?;
        Some(Self {
            chain_id: config.chain_id,
            v2: Deployment {
                factory: types::chain::Address::try_from(config.v2_factory.as_slice()).ok()?,
                init_code_hash: B256::try_from(config.v2_init_code_hash.as_slice()).ok()?,
            },
            v3: venues::v3::Deployment {
                factory: types::chain::Address::try_from(config.v3_factory.as_slice()).ok()?,
                init_code_hash: B256::try_from(config.v3_init_code_hash.as_slice()).ok()?,
                tick_lens: types::chain::Address::try_from(config.v3_tick_lens.as_slice()).ok()?,
            },
            check_every: config.check_every,
            check_sample: config.check_sample.try_into().ok()?,
            quote_assets: config
                .quote_assets
                .iter()
                .map(|a| types::chain::Address::try_from(a.as_slice()).ok())
                .collect::<Option<_>>()?,
            pricing: match config.pricing {
                Some(pricing) => Some(pricing_from_proto(pricing)?),
                None => None,
            },
        })
    }

    pub fn base() -> Self {
        Self {
            chain_id: 8453,
            v2: venues::v2::BASE,
            v3: venues::v3::BASE,
            check_every: None,
            check_sample: 20,
            quote_assets: BASE_QUOTE_ASSETS.to_vec(),
            pricing: Some(PricingConfig::base()),
        }
    }
}

fn pricing_to_proto(config: &PricingConfig) -> proto::engine::v1::PricingConfig {
    proto::engine::v1::PricingConfig {
        native: config.native.to_vec(),
        reference_pools: config.reference_pools.iter().map(|p| p.to_vec()).collect(),
        liquidity_floor_usd: config.liquidity_floor_usd,
        supply_refresh_blocks: config.supply_refresh_blocks,
    }
}

fn pricing_from_proto(config: proto::engine::v1::PricingConfig) -> Option<PricingConfig> {
    Some(PricingConfig {
        native: Address::try_from(config.native.as_slice()).ok()?,
        reference_pools: config
            .reference_pools
            .iter()
            .map(|p| Address::try_from(p.as_slice()).ok())
            .collect::<Option<Vec<_>>>()?,
        liquidity_floor_usd: config.liquidity_floor_usd,
        supply_refresh_blocks: config.supply_refresh_blocks,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum EngineError {
    /// The follower skipped a block. It fills gaps by number, so this is a bug upstream.
    Gap { expected: u64, received: u64 },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gap { expected, received } => {
                write!(f, "block gap: expected {expected}, received {received}")
            }
        }
    }
}

impl std::error::Error for EngineError {}

/// Counts kept while the engine runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub blocks: u64,
    pub logs: u64,
    /// Blocks whose parent wasn't the block before them.
    pub reorgs_detected: u64,
    pub pairs_tracked: u64,
    /// Pairs whose address isn't the factory's CREATE2 address for their tokens.
    pub pairs_rejected: u64,
    /// Verification calls that failed as a whole; their pairs are proven again on next sight.
    pub verify_failures: u64,
    pub verify_calls: u64,
    pub pools_tracked: u64,
    /// v3 pools whose address isn't the factory's CREATE2 address for their tokens and fee.
    pub pools_rejected: u64,
    /// Calls reading v3 pools' state at the block they were first seen.
    pub bootstrap_calls: u64,
    /// v3 bootstraps abandoned because a call failed; each pool starts again on next sight.
    pub bootstrap_failures: u64,
    pub updates: u64,
    /// Trade records published: one per Swap on a tracked pool.
    pub trades: u64,
    /// Swaps buffered on a pool whose proof or bootstrap read then failed. The pool is read
    /// again when it next trades, but these swaps are never published.
    pub trades_dropped: u64,
    pub checks_passed: u64,
    pub checks_failed: u64,
    /// Calls reading tokens' name, symbol, decimals and supply.
    pub metadata_calls: u64,
    /// Calls reading supplies again.
    pub supply_calls: u64,
    /// Metadata or supply calls that failed as a whole.
    pub metadata_failures: u64,
    pub price_updates: u64,
    /// Tokens with a published display price.
    pub tokens_priced: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub head: Option<(u64, B256)>,
    pub stats: Stats,
    /// Over every block applied, in order: number and hash.
    pub digest: blake3::Hash,
    /// Over every pool update published, in order: its proto bytes.
    pub updates_digest: blake3::Hash,
    /// Over every trade published, in order: its proto bytes.
    pub trades_digest: blake3::Hash,
    /// Over every price update published, in order: its proto bytes.
    pub prices_digest: blake3::Hash,
    /// `None` without pricing.
    pub coverage: Option<Coverage>,
}

/// The text `engine follow` and `engine replay` print, and a pinned replay fixture's summary
/// file holds (D99): two runs with the same text made the same decisions.
impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some((number, hash)) = self.head {
            writeln!(f, "head {number} {hash}")?;
        }
        let s = &self.stats;
        writeln!(
            f,
            "{} blocks, {} logs, {} reorgs detected, digest {}",
            s.blocks, s.logs, s.reorgs_detected, self.digest
        )?;
        writeln!(
            f,
            "v2: {} pairs tracked, {} rejected, {} verification calls ({} failed)",
            s.pairs_tracked, s.pairs_rejected, s.verify_calls, s.verify_failures
        )?;
        writeln!(
            f,
            "v3: {} pools tracked, {} rejected, {} bootstrap calls ({} bootstraps failed)",
            s.pools_tracked, s.pools_rejected, s.bootstrap_calls, s.bootstrap_failures
        )?;
        writeln!(
            f,
            "{} pool updates, digest {}",
            s.updates, self.updates_digest
        )?;
        writeln!(
            f,
            "{} trades ({} dropped with a failed read), digest {}",
            s.trades, s.trades_dropped, self.trades_digest
        )?;
        writeln!(
            f,
            "shadow checks: {} passed, {} failed",
            s.checks_passed, s.checks_failed
        )?;
        // Only with pricing on, so a recording from before pricing prints what it always did.
        if let Some(c) = &self.coverage {
            writeln!(
                f,
                "{} price updates, digest {}; {} metadata calls, {} supply calls ({} failed)",
                s.price_updates,
                self.prices_digest,
                s.metadata_calls,
                s.supply_calls,
                s.metadata_failures
            )?;
            writeln!(
                f,
                "coverage: {} tokens seen, {} with a quote-asset pool, {} priced ({} thin), {} without decimals",
                c.tokens_seen,
                c.tokens_quotable,
                c.tokens_priced,
                c.tokens_thin,
                c.tokens_without_decimals
            )?;
            for (token, count) in &c.unquoted_counterparts {
                writeln!(f, "  unquoted tokens trading against {token}: {count}")?;
            }
        }
        Ok(())
    }
}

/// A call the engine is waiting on, and what to do with its answer.
pub(crate) enum Pending {
    V2(crate::v2::Call),
    V3(crate::v3::Call),
    Prices(crate::prices::Call),
}

/// What one step asks of the outside world.
#[derive(Default)]
pub(crate) struct Effects {
    pub calls: Vec<(Pending, EthCall)>,
    pub updates: Vec<PoolUpdate>,
    pub trades: Vec<Trade>,
    pub prices: Vec<PriceUpdate>,
    /// Set once per block, after the block is applied.
    pub status: Option<EngineStatus>,
}

pub(crate) struct State {
    config: EngineConfig,
    recent: VecDeque<(u64, B256)>,
    stats: Stats,
    digest: blake3::Hasher,
    updates_digest: blake3::Hasher,
    trades_digest: blake3::Hasher,
    prices_digest: blake3::Hasher,
    v2: V2Pools,
    v3: V3Pools,
    prices: Option<Prices>,
    /// The clock reading when the first block arrived: uptime counts from it.
    started: Option<Timestamp>,
}

impl State {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            v2: V2Pools::new(config.v2, config.quote_assets.clone()),
            v3: V3Pools::new(config.v3, config.quote_assets.clone()),
            prices: config
                .pricing
                .clone()
                .map(|pricing| Prices::new(config.chain_id, config.quote_assets.clone(), pricing)),
            config,
            recent: VecDeque::with_capacity(RECENT_BLOCKS),
            stats: Stats::default(),
            digest: blake3::Hasher::new(),
            updates_digest: blake3::Hasher::new(),
            trades_digest: blake3::Hasher::new(),
            prices_digest: blake3::Hasher::new(),
            started: None,
        }
    }

    pub fn apply_block(
        &mut self,
        block: &Block,
        now: Timestamp,
        effects: &mut Effects,
    ) -> Result<(), EngineError> {
        if let Some(&(number, hash)) = self.recent.back() {
            if block.number != number + 1 {
                return Err(EngineError::Gap {
                    expected: number + 1,
                    received: block.number,
                });
            }
            if block.parent_hash != hash {
                // Undoing the replaced blocks' state arrives with tiered undo (D12). Until
                // then the engine re-anchors on the new chain.
                self.stats.reorgs_detected += 1;
                tracing::warn!(block.number, %block.parent_hash, held = %hash, "reorg detected");
                self.recent.clear();
            }
        }
        if self.recent.len() == RECENT_BLOCKS {
            self.recent.pop_front();
        }
        self.recent.push_back((block.number, block.hash));
        self.stats.blocks += 1;
        self.stats.logs += block.logs.len() as u64;
        self.digest.update(&block.number.to_le_bytes());
        self.digest.update(block.hash.as_slice());

        self.v2
            .apply_block(self.config.chain_id, block, &mut self.stats, effects);
        self.v3
            .apply_block(self.config.chain_id, block, &mut self.stats, effects);
        if let Some(every) = self.config.check_every
            && block.number.is_multiple_of(every)
        {
            self.v2
                .check(block.number, self.config.check_sample, effects);
            self.v3
                .check(block.number, self.config.check_sample, effects);
        }
        self.finish(effects);
        // Prices go out once the block's state is all applied (D77).
        if let Some(prices) = &mut self.prices {
            prices.on_block(block, &self.v2, &self.v3, &mut self.stats, effects);
            for price in &effects.prices {
                self.prices_digest
                    .update(&price.to_proto().encode_length_delimited_to_vec());
            }
            self.stats.price_updates += effects.prices.len() as u64;
        }

        // Seconds-resolution timestamps make this an upper bound, like the measured delay.
        let now_ms = now.unix_nanos / 1_000_000;
        let lag_ms = now_ms.saturating_sub(block.timestamp * 1_000);
        let started_ms = self.started.get_or_insert(now).unix_nanos / 1_000_000;
        effects.status = Some(EngineStatus {
            id: EngineStatus::id_for(self.config.chain_id, block.hash),
            chain_id: self.config.chain_id,
            block_number: block.number,
            block_hash: block.hash,
            block_timestamp: block.timestamp,
            lag_blocks: block
                .chain_head
                .map(|head| head.saturating_sub(block.number)),
            lag_ms,
            v2_pools: self.stats.pairs_tracked,
            v3_pools: self.stats.pools_tracked,
            shadow_checks: self.stats.checks_passed + self.stats.checks_failed,
            shadow_check_mismatches: self.stats.checks_failed,
            uptime_ms: now_ms.saturating_sub(started_ms),
        });
        tracing::info!(
            block.number,
            logs = block.logs.len(),
            updates = effects.updates.len(),
            trades = effects.trades.len(),
            prices = effects.prices.len(),
            lag_ms,
            "block"
        );
        Ok(())
    }

    pub fn on_answer(&mut self, pending: Pending, result: CallResult, effects: &mut Effects) {
        let chain_id = self.config.chain_id;
        if let CallResult::Failed(error) = &result {
            tracing::warn!(%error, "a call failed");
        }
        match pending {
            Pending::V2(call) => {
                self.v2
                    .on_answer(chain_id, call, result, &mut self.stats, effects)
            }
            Pending::V3(call) => {
                self.v3
                    .on_answer(chain_id, call, result, &mut self.stats, effects)
            }
            Pending::Prices(call) => {
                if let Some(prices) = &mut self.prices {
                    prices.on_answer(call, result, &mut self.stats);
                }
            }
        }
        self.finish(effects);
    }

    /// Folds this step's updates and trades into their digests, counts them, and notes the
    /// updates for pricing.
    fn finish(&mut self, effects: &Effects) {
        if let Some(prices) = &mut self.prices {
            prices.on_updates(&effects.updates, &self.v2, &self.v3);
        }
        for update in &effects.updates {
            self.updates_digest
                .update(&update.to_proto().encode_length_delimited_to_vec());
        }
        self.stats.updates += effects.updates.len() as u64;
        for trade in &effects.trades {
            self.trades_digest
                .update(&trade.to_proto().encode_length_delimited_to_vec());
        }
        self.stats.trades += effects.trades.len() as u64;
        self.stats.pairs_tracked = self.v2.tracked() as u64;
        self.stats.pools_tracked = self.v3.tracked() as u64;
    }

    pub fn summary(&self) -> Summary {
        Summary {
            head: self.recent.back().copied(),
            stats: self.stats.clone(),
            digest: self.digest.finalize(),
            updates_digest: self.updates_digest.finalize(),
            trades_digest: self.trades_digest.finalize(),
            prices_digest: self.prices_digest.finalize(),
            coverage: self.prices.as_ref().map(Prices::coverage),
        }
    }
}
