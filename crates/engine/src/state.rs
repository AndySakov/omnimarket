//! The engine's state machine: blocks and call answers in, calls and pool updates out. No
//! I/O and no waiting, so every step is a plain function call.

use std::collections::VecDeque;

use prost::Message as _;
use types::Timestamp;
use types::chain::{B256, Block, CallResult, EthCall};
use venues::v2::{Deployment, Reserves};

use crate::outbox::PoolUpdate;
use crate::v2::V2Pools;

/// How many recent block hashes the engine keeps, to find where a reorg forked.
const RECENT_BLOCKS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub chain_id: u64,
    pub v2: Deployment,
    /// Every this many blocks, compare a sample of pools with the chain (shadow state check).
    pub check_every: Option<u64>,
    /// How many pools each check samples.
    pub check_sample: usize,
}

impl EngineConfig {
    /// Proto bytes, recorded first in the input log.
    pub fn encode(&self) -> Vec<u8> {
        proto::engine::v1::EngineConfig {
            chain_id: self.chain_id,
            v2_factory: self.v2.factory.to_vec(),
            v2_init_code_hash: self.v2.init_code_hash.to_vec(),
            check_every: self.check_every,
            check_sample: self.check_sample as u64,
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
            check_every: config.check_every,
            check_sample: config.check_sample.try_into().ok()?,
        })
    }

    pub fn base() -> Self {
        Self {
            chain_id: 8453,
            v2: venues::v2::BASE,
            check_every: None,
            check_sample: 20,
        }
    }
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
    pub updates: u64,
    pub checks_passed: u64,
    pub checks_failed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub head: Option<(u64, B256)>,
    pub stats: Stats,
    /// Over every block applied, in order: number and hash.
    pub digest: blake3::Hash,
    /// Over every pool update published, in order: its proto bytes.
    pub updates_digest: blake3::Hash,
}

/// A call the engine is waiting on, and what to do with its answer.
pub(crate) enum Pending {
    /// token0() and token1() of each pair, to prove it by its CREATE2 address.
    Verify { pairs: Vec<types::chain::Address> },
    /// getReserves() of each pair at `block`, against the reserves the engine held then.
    Check {
        block: u64,
        expected: Vec<(types::chain::Address, Reserves)>,
    },
}

/// What one step asks of the outside world.
#[derive(Default)]
pub(crate) struct Effects {
    pub calls: Vec<(Pending, EthCall)>,
    pub updates: Vec<PoolUpdate>,
}

pub(crate) struct State {
    config: EngineConfig,
    recent: VecDeque<(u64, B256)>,
    stats: Stats,
    digest: blake3::Hasher,
    updates_digest: blake3::Hasher,
    v2: V2Pools,
}

impl State {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            v2: V2Pools::new(config.v2),
            config,
            recent: VecDeque::with_capacity(RECENT_BLOCKS),
            stats: Stats::default(),
            digest: blake3::Hasher::new(),
            updates_digest: blake3::Hasher::new(),
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
        if let Some(every) = self.config.check_every
            && block.number.is_multiple_of(every)
        {
            self.v2
                .check(block.number, self.config.check_sample, effects);
        }
        self.finish(effects);

        // Seconds-resolution timestamps make this an upper bound, like the measured delay.
        let lag_ms = (now.unix_nanos / 1_000_000).saturating_sub(block.timestamp * 1_000);
        tracing::info!(
            block.number,
            logs = block.logs.len(),
            updates = effects.updates.len(),
            lag_ms,
            "block"
        );
        Ok(())
    }

    pub fn on_answer(&mut self, pending: Pending, result: CallResult, effects: &mut Effects) {
        match pending {
            Pending::Verify { pairs } => self.v2.on_verified(
                self.config.chain_id,
                pairs,
                result,
                &mut self.stats,
                effects,
            ),
            Pending::Check { block, expected } => {
                self.v2.on_checked(block, expected, result, &mut self.stats)
            }
        }
        self.finish(effects);
    }

    /// Folds this step's updates into the digest and counts its calls.
    fn finish(&mut self, effects: &Effects) {
        for update in &effects.updates {
            self.updates_digest
                .update(&update.to_proto().encode_length_delimited_to_vec());
        }
        self.stats.updates += effects.updates.len() as u64;
        self.stats.pairs_tracked = self.v2.tracked() as u64;
    }

    pub fn summary(&self) -> Summary {
        Summary {
            head: self.recent.back().copied(),
            stats: self.stats.clone(),
            digest: self.digest.finalize(),
            updates_digest: self.updates_digest.finalize(),
        }
    }
}
