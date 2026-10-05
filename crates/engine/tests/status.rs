//! The engine's status after each block (#79): head, lag, pools tracked and shadow checks, on a
//! simulated chain, and the same statuses again from a replay of its recording.

use std::time::Duration;

use alloy_primitives::{U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{
    InMemorySink, Recorder, RecordingClock, RecordingEventSource, RecordingRpc, Replay, SimClock,
    SimEventSource, SimRpc,
};
use engine::{EngineConfig, EngineStatus, InMemoryOutbox, Mode, Run};
use types::Timestamp;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall, Log};
use venues::multicall;
use venues::v2::{self, BASE, Reserves, Sync};

/// The first block's timestamp, 2026-01-01T00:00:00Z.
const T0: u64 = 1_767_225_600;
const START: Timestamp = Timestamp::from_unix_nanos(T0 * 1_000_000_000);
const TOKEN_A: Address = address!("1000000000000000000000000000000000000001");
const TOKEN_B: Address = address!("2000000000000000000000000000000000000002");

fn pair() -> Address {
    BASE.pair_address(TOKEN_A, TOKEN_B)
}

fn sync(log_index: u64, reserve0: u64, reserve1: u64) -> Log {
    Log {
        address: pair(),
        topics: vec![Sync::SIGNATURE_HASH],
        data: Bytes::from((U256::from(reserve0), U256::from(reserve1)).abi_encode()),
        log_index,
        transaction_hash: B256::ZERO,
    }
}

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

/// Blocks 100 to 103, made every 2s. Each arrives `arrive_ms` after the first block's timestamp
/// and carries the chain head the follower saw when it read it.
fn chain(head: [Option<u64>; 4]) -> Vec<(Duration, Block)> {
    let arrive_ms = [1_500, 4_200, 6_100, 9_000];
    (0..4)
        .map(|i| {
            let number = 100 + i as u64;
            let block = Block {
                number,
                hash: hash(number),
                parent_hash: hash(number - 1),
                timestamp: T0 + 2 * i as u64,
                logs: if i == 0 {
                    vec![sync(0, 10, 20)]
                } else {
                    vec![]
                },
                chain_head: head[i],
            };
            (Duration::from_millis(arrive_ms[i]), block)
        })
        .collect()
}

/// A node that knows the pair's tokens and holds its reserves at 10 and 20, except at block 102.
fn answer(call: &EthCall) -> CallResult {
    let results: Vec<Option<Vec<u8>>> = multicall::decode_calls(&call.data)
        .unwrap()
        .into_iter()
        .map(|(_, data)| {
            if data.as_ref() == v2::token0_call().as_slice() {
                Some(v2::encode_address(TOKEN_A))
            } else if data.as_ref() == v2::token1_call().as_slice() {
                Some(v2::encode_address(TOKEN_B))
            } else {
                let reserve1 = if call.block == 102 { 21 } else { 20 };
                Some(v2::encode_reserves(Reserves {
                    reserve0: 10,
                    reserve1,
                }))
            }
        })
        .collect();
    CallResult::Returned(Bytes::from(multicall::encode_results(&results)))
}

fn config() -> EngineConfig {
    EngineConfig {
        check_every: Some(1),
        ..EngineConfig::base()
    }
}

fn run(blocks: Vec<(Duration, Block)>) -> (Vec<EngineStatus>, Vec<det::InputRecord>) {
    let sink = InMemorySink::default();
    let outbox = InMemoryOutbox::default();
    det::run_simulated(async {
        let clock = SimClock::starting_at(START);
        let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock.clone()));
        recorder.record_config(config().encode());
        let rpc = SimRpc::new(|call: EthCall| (Duration::from_millis(50), answer(&call)));
        let engine = engine::Engine::new(
            config(),
            Box::new(RecordingClock::new(Box::new(clock), recorder.clone())),
            Box::new(RecordingEventSource::new(
                Box::new(SimEventSource::new(blocks)),
                recorder.clone(),
            )),
            Box::new(RecordingRpc::new(Box::new(rpc), recorder)),
            Box::new(outbox.clone()),
        );
        engine.run().await.unwrap()
    });
    (outbox.statuses(), sink.records())
}

/// (block, lag in blocks, lag in ms, v2 pools, shadow checks, mismatches, uptime in ms)
type Row = (u64, Option<u64>, u64, u64, u64, u64, u64);

fn rows(statuses: &[EngineStatus]) -> Vec<Row> {
    statuses
        .iter()
        .map(|s| {
            (
                s.block_number,
                s.lag_blocks,
                s.lag_ms,
                s.v2_pools,
                s.shadow_checks,
                s.shadow_check_mismatches,
                s.uptime_ms,
            )
        })
        .collect()
}

#[test]
fn a_status_follows_every_block() {
    let (statuses, _) = run(chain([Some(101), Some(101), Some(104), Some(103)]));
    // Lag in ms is arrival minus block time: 1,500 − 0, 4,200 − 2,000, 6,100 − 4,000 and
    // 9,000 − 6,000. Uptime counts from block 100's arrival at 1,500. The pair is proven 50ms
    // after block 100, so it's tracked from block 101. Each block from 101 checks it, and the
    // answer comes 50ms later: the check at 102 disagrees (reserve1 is 21 there).
    assert_eq!(
        rows(&statuses),
        vec![
            (100, Some(1), 1_500, 0, 0, 0, 0),
            (101, Some(0), 2_200, 1, 0, 0, 2_700),
            (102, Some(2), 2_100, 1, 1, 0, 4_600),
            (103, Some(0), 3_000, 1, 2, 1, 7_500),
        ]
    );
    let last = statuses.last().unwrap();
    assert_eq!(last.chain_id, 8453);
    assert_eq!(last.block_hash, hash(103));
    assert_eq!(last.block_timestamp, T0 + 6);
    assert_eq!(last.v3_pools, 0);
}

#[test]
fn a_block_without_the_chain_head_has_no_lag_in_blocks() {
    let (statuses, _) = run(chain([None; 4]));
    assert!(statuses.iter().all(|s| s.lag_blocks.is_none()));
    assert_eq!(statuses[1].lag_ms, 2_200);
}

#[test]
fn a_recording_replays_to_the_same_statuses() {
    let (live, recording) = run(chain([Some(101), Some(101), Some(104), Some(103)]));
    let replay = Replay::new(recording);
    let config = EngineConfig::decode(&replay.config().unwrap()).unwrap();
    let outbox = InMemoryOutbox::default();
    let engine = engine::Engine::new(
        config,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(outbox.clone()),
    );
    replay.run(engine.run()).unwrap();
    assert_eq!(outbox.statuses(), live);
    assert_eq!(live.len(), 4);
}

// data.md: kind `engine_status`, natural key chain id (u64) then block hash (32 bytes).
#[test]
fn status_lineage_follows_data_md() {
    let (statuses, _) = run(chain([None; 4]));
    let key = [&8453u64.to_le_bytes()[..], hash(100).as_slice()].concat();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&13u64.to_le_bytes());
    hasher.update(b"engine_status");
    hasher.update(&key);
    assert_eq!(
        statuses[0].id.as_bytes()[..],
        hasher.finalize().as_bytes()[..16]
    );
}

#[test]
fn the_publisher_stamps_the_run() {
    let (statuses, _) = run(chain([Some(101), Some(101), Some(104), Some(103)]));
    let run = Run {
        mode: Mode::Replay,
        core_instance: "base-00000000000000ff".into(),
        recording: true,
    };
    let wire = statuses[2].to_proto(&run);
    assert_eq!(
        wire,
        proto::status::v1::EngineStatus {
            lineage: Some(proto::lineage::v1::Lineage {
                id: statuses[2].id.as_bytes().to_vec(),
                caused_by: vec![],
            }),
            chain_id: 8453,
            block_number: 102,
            block_hash: hash(102).to_vec(),
            block_timestamp: T0 + 4,
            lag_blocks: Some(2),
            lag_ms: 2_100,
            v2_pools: 1,
            v3_pools: 0,
            shadow_checks: 1,
            shadow_check_mismatches: 0,
            uptime_ms: 4_600,
            mode: proto::status::v1::engine_status::Mode::Replay.into(),
            recording: true,
            core_instance: "base-00000000000000ff".into(),
        }
    );
}
