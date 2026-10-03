//! The status read model: the engine's latest status (#79), as `GET /v1/status` and the `status`
//! topic serve it. It passes the engine's numbers through; nothing here reads a clock.

use proto::api::v1::EngineStatus;
use proto::api::v1::engine_status::{Mode, VenuePools};
use proto::status::v1::engine_status::Mode as RecordMode;

use crate::model::{ModelError, hex};

/// The contract's view of one `status.base` record. A record without the chain head shows a lag
/// of 0 blocks.
pub fn engine_status(record: &proto::status::v1::EngineStatus) -> Result<EngineStatus, ModelError> {
    if record.block_hash.len() != 32 {
        return Err(ModelError::BadHash {
            len: record.block_hash.len(),
        });
    }
    let mode = match record.mode() {
        RecordMode::Unspecified => Mode::Unspecified,
        RecordMode::Live => Mode::Live,
        RecordMode::Replay => Mode::Replay,
    };
    let venue = |venue: &str, pools: u64| VenuePools {
        venue: venue.to_string(),
        // Every pool the engine tracks has state until pools are tiered (#41).
        known: pools,
        active: pools,
    };
    Ok(EngineStatus {
        lineage: record.lineage.clone(),
        chain_id: record.chain_id,
        mode: mode.into(),
        head_block_number: record.block_number,
        head_block_hash: hex(&record.block_hash),
        head_block_time_ms: record.block_timestamp.saturating_mul(1000),
        lag_blocks: record.lag_blocks.unwrap_or_default(),
        lag_ms: record.lag_ms,
        pools: vec![
            venue("uniswap-v2", record.v2_pools),
            venue("uniswap-v3", record.v3_pools),
        ],
        shadow_checks: record.shadow_checks,
        shadow_check_mismatches: record.shadow_check_mismatches,
        recording: record.recording,
        core_instance: record.core_instance.clone(),
        uptime_ms: record.uptime_ms,
    })
}
