//! The engine's metrics: what the outbox saw and the latest status, on a simulated chain.

use std::time::Duration;

use alloy_primitives::{U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{
    InMemorySink, Recorder, RecordingClock, RecordingEventSource, RecordingRpc, SimClock,
    SimEventSource, SimRpc,
};
use engine::{EngineConfig, EngineMetrics, InMemoryOutbox};
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

fn run(blocks: Vec<(Duration, Block)>) -> (EngineMetrics, InMemoryOutbox) {
    let sink = InMemorySink::default();
    let outbox = InMemoryOutbox::default();
    let metrics = EngineMetrics::default();
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
            metrics.outbox(Box::new(outbox.clone())),
        );
        engine.run().await.unwrap()
    });
    (metrics, outbox)
}

#[test]
fn before_the_first_block_there_are_only_counters() {
    let text = EngineMetrics::default().render();
    assert!(text.contains("omnimarket_engine_records_published_total{topic=\"trades.base\"} 0\n"));
    assert!(!text.contains("omnimarket_engine_head_block"));
}

#[test]
fn metrics_report_the_latest_status_and_what_was_published() {
    let (metrics, outbox) = run(chain([Some(101), Some(101), Some(104), Some(103)]));
    let text = metrics.render();
    for line in [
        "omnimarket_engine_head_block 103\n",
        "omnimarket_engine_lag_blocks 0\n",
        "omnimarket_engine_lag_ms 3000\n",
        "omnimarket_engine_pools{venue=\"uniswap-v2\"} 1\n",
        "omnimarket_engine_pools{venue=\"uniswap-v3\"} 0\n",
        "omnimarket_engine_shadow_checks_total 2\n",
        "omnimarket_engine_shadow_check_mismatches_total 1\n",
        "omnimarket_engine_uptime_ms 7500\n",
        "omnimarket_engine_records_published_total{topic=\"trades.base\"} 0\n",
    ] {
        assert!(text.contains(line), "{line:?} missing from\n{text}");
    }
    assert!(text.contains(&format!(
        "omnimarket_engine_records_published_total{{topic=\"pool-updates.base\"}} {}\n",
        outbox.updates().len()
    )));
    assert!(text.contains(&format!(
        "omnimarket_engine_records_published_total{{topic=\"prices.base\"}} {}\n",
        outbox.prices().len()
    )));
}

#[test]
fn a_status_without_the_chain_head_has_no_lag_in_blocks_metric() {
    let (metrics, _) = run(chain([None; 4]));
    assert!(!metrics.render().contains("omnimarket_engine_lag_blocks"));
}
