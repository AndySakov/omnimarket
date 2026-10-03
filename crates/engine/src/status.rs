//! The engine's status after each canonical block (#79): where it is, how far behind the chain,
//! what it tracks, and whether its shadow checks agree with the chain.

use types::LineageId;
use types::chain::B256;

/// What the core knows about itself after a block. Every field comes from its inputs, so a replay
/// of a recording gives the same statuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineStatus {
    /// From the natural key (chain, block hash).
    pub id: LineageId,
    pub chain_id: u64,
    pub block_number: u64,
    pub block_hash: B256,
    /// Unix seconds, as the block header states it.
    pub block_timestamp: u64,
    /// Blocks between this one and the chain's latest when the follower read it; `None` when the
    /// recording doesn't hold the head.
    pub lag_blocks: Option<u64>,
    /// From the block's timestamp to the core's clock reading when the block arrived.
    pub lag_ms: u64,
    pub v2_pools: u64,
    pub v3_pools: u64,
    pub shadow_checks: u64,
    pub shadow_check_mismatches: u64,
    /// From the core's first block to this one.
    pub uptime_ms: u64,
}

/// What a status says about the run rather than the core: set by whoever publishes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub mode: Mode,
    pub core_instance: String,
    /// Whether the session's inputs are recorded (D54). A replay plays a recording, so it's
    /// always recorded.
    pub recording: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Live,
    Replay,
}

impl EngineStatus {
    pub fn id_for(chain_id: u64, block_hash: B256) -> LineageId {
        let key = [&chain_id.to_le_bytes()[..], block_hash.as_slice()].concat();
        LineageId::from_natural_key("engine_status", &key)
    }

    pub fn to_proto(&self, run: &Run) -> proto::status::v1::EngineStatus {
        use proto::status::v1::engine_status;
        proto::status::v1::EngineStatus {
            lineage: Some(proto::lineage::v1::Lineage {
                id: self.id.as_bytes().to_vec(),
                caused_by: Vec::new(),
            }),
            chain_id: self.chain_id,
            block_number: self.block_number,
            block_hash: self.block_hash.to_vec(),
            block_timestamp: self.block_timestamp,
            lag_blocks: self.lag_blocks,
            lag_ms: self.lag_ms,
            v2_pools: self.v2_pools,
            v3_pools: self.v3_pools,
            shadow_checks: self.shadow_checks,
            shadow_check_mismatches: self.shadow_check_mismatches,
            uptime_ms: self.uptime_ms,
            mode: match run.mode {
                Mode::Live => engine_status::Mode::Live,
                Mode::Replay => engine_status::Mode::Replay,
            }
            .into(),
            recording: run.recording,
            core_instance: run.core_instance.clone(),
        }
    }
}
