//! v2 pools on a simulated chain: discovery, the CREATE2 proof, state from `Sync`, the shadow
//! state check, and record/replay.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use alloy_primitives::{U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{
    InMemorySink, Recorder, RecordingClock, RecordingEventSource, RecordingRpc, Replay, SimClock,
    SimEventSource, SimRpc,
};
use engine::{Engine, EngineConfig, InMemoryOutbox, PoolState, Summary};
use types::Timestamp;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall, Log};
use venues::multicall;
use venues::v2::{self, BASE, PairCreated, Reserves, Sync};

const START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);
const TOKEN_A: Address = address!("1000000000000000000000000000000000000001");
const TOKEN_B: Address = address!("2000000000000000000000000000000000000002");
const TOKEN_C: Address = address!("3000000000000000000000000000000000000003");
const FORGED: Address = address!("f000000000000000000000000000000000000000");

fn genuine() -> Address {
    BASE.pair_address(TOKEN_A, TOKEN_B)
}

fn created() -> Address {
    BASE.pair_address(TOKEN_A, TOKEN_C)
}

fn sync(pair: Address, log_index: u64, reserve0: u64, reserve1: u64) -> Log {
    Log {
        address: pair,
        topics: vec![Sync::SIGNATURE_HASH],
        data: Bytes::from((U256::from(reserve0), U256::from(reserve1)).abi_encode()),
        log_index,
        transaction_hash: B256::ZERO,
    }
}

fn pair_created(token0: Address, token1: Address, pair: Address, log_index: u64) -> Log {
    Log {
        address: BASE.factory,
        topics: vec![
            PairCreated::SIGNATURE_HASH,
            token0.into_word(),
            token1.into_word(),
        ],
        data: Bytes::from((pair, U256::from(1u64)).abi_encode()),
        log_index,
        transaction_hash: B256::ZERO,
    }
}

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

/// Blocks from 100, one every 2s, each with the given logs.
fn chain(logs: Vec<Vec<Log>>) -> Vec<(Duration, Block)> {
    logs.into_iter()
        .enumerate()
        .map(|(i, logs)| {
            let number = 100 + i as u64;
            let block = Block {
                number,
                hash: hash(number),
                parent_hash: hash(number - 1),
                timestamp: 1_767_225_600 + 2 * i as u64,
                logs,
            };
            (Duration::from_secs(2 * (i as u64 + 1)), block)
        })
        .collect()
}

/// A node that knows each pair's tokens and, per block, its reserves. Counts its calls.
#[derive(Clone, Default)]
struct Node {
    tokens: BTreeMap<Address, (Address, Address)>,
    reserves: BTreeMap<(Address, u64), Reserves>,
    calls: Rc<Cell<u64>>,
}

impl Node {
    fn answer(&self, call: &EthCall) -> CallResult {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(call.to, multicall::ADDRESS);
        let results: Vec<Option<Vec<u8>>> = multicall::decode_calls(&call.data)
            .unwrap()
            .into_iter()
            .map(|(target, data)| {
                let tokens = self.tokens.get(&target);
                if data.as_ref() == v2::token0_call().as_slice() {
                    tokens.map(|t| v2::encode_address(t.0))
                } else if data.as_ref() == v2::token1_call().as_slice() {
                    tokens.map(|t| v2::encode_address(t.1))
                } else {
                    self.reserves
                        .get(&(target, call.block))
                        .map(|r| v2::encode_reserves(*r))
                }
            })
            .collect();
        CallResult::Returned(Bytes::from(multicall::encode_results(&results)))
    }
}

fn node() -> Node {
    let mut tokens = BTreeMap::new();
    tokens.insert(genuine(), (TOKEN_A, TOKEN_B));
    // A fake that claims the same tokens: its address isn't the factory's CREATE2 address.
    tokens.insert(FORGED, (TOKEN_A, TOKEN_B));
    Node {
        tokens,
        ..Node::default()
    }
}

struct Run {
    summary: Summary,
    updates: Vec<engine::PoolUpdate>,
    recording: Vec<det::InputRecord>,
}

fn run(config: EngineConfig, blocks: Vec<(Duration, Block)>, node: Node) -> Run {
    let sink = InMemorySink::default();
    let outbox = InMemoryOutbox::default();
    let summary = det::run_simulated(async {
        let clock = SimClock::starting_at(START);
        let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock.clone()));
        recorder.record_config(config.encode());
        let rpc = SimRpc::new(move |call: EthCall| (Duration::from_millis(50), node.answer(&call)));
        let engine = Engine::new(
            config,
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
    Run {
        summary,
        updates: outbox.updates(),
        recording: sink.records(),
    }
}

fn reserves(reserve0: u128, reserve1: u128) -> PoolState {
    PoolState::V2(Reserves { reserve0, reserve1 })
}

#[test]
fn tracks_genuine_pairs_and_ignores_forged_ones() {
    let blocks = chain(vec![
        vec![sync(genuine(), 0, 10, 20), sync(FORGED, 1, 1, 1)],
        vec![sync(genuine(), 0, 11, 19), sync(FORGED, 1, 2, 2)],
        vec![
            pair_created(TOKEN_A, TOKEN_C, created(), 3),
            sync(created(), 4, 5, 5),
        ],
    ]);
    let run = run(EngineConfig::base(), blocks, node());

    let seen: Vec<(Address, u64, Option<PoolState>, PoolState)> = run
        .updates
        .iter()
        .map(|u| (u.pool, u.block_number, u.before.clone(), u.after.clone()))
        .collect();
    assert_eq!(
        seen,
        vec![
            (genuine(), 100, None, reserves(10, 20)),
            (genuine(), 101, Some(reserves(10, 20)), reserves(11, 19)),
            (created(), 102, Some(reserves(0, 0)), reserves(5, 5)),
        ]
    );
    assert_eq!(run.summary.stats.pairs_tracked, 2);
    assert_eq!(run.summary.stats.pairs_rejected, 1);
}

#[test]
fn verification_reads_are_batched() {
    let pairs: Vec<Address> = (0..250u64)
        .map(|i| Address::left_padding_from(&(i + 1).to_be_bytes()))
        .collect();
    let logs: Vec<Log> = pairs
        .iter()
        .enumerate()
        .map(|(i, &pair)| sync(pair, i as u64, 1, 1))
        .collect();
    let node = Node::default();
    let calls = node.calls.clone();
    let run = run(EngineConfig::base(), chain(vec![logs]), node);
    // 250 new pairs at 100 per call.
    assert_eq!(calls.get(), 3);
    assert_eq!(run.summary.stats.verify_calls, 3);
    assert_eq!(run.summary.stats.pairs_rejected, 250);
}

#[test]
fn the_shadow_check_compares_with_the_chain_at_the_same_block() {
    let blocks = chain(vec![
        vec![sync(genuine(), 0, 10, 20)],
        vec![sync(genuine(), 0, 11, 19)],
        vec![],
    ]);
    let mut node = node();
    // The chain agrees at block 101 and disagrees at 102.
    node.reserves.insert(
        (genuine(), 101),
        Reserves {
            reserve0: 11,
            reserve1: 19,
        },
    );
    node.reserves.insert(
        (genuine(), 102),
        Reserves {
            reserve0: 12,
            reserve1: 18,
        },
    );
    let config = EngineConfig {
        check_every: Some(1),
        ..EngineConfig::base()
    };
    let run = run(config, blocks, node);
    // Block 100's check finds no pair yet: the proof is still in flight.
    assert_eq!(run.summary.stats.checks_passed, 1);
    assert_eq!(run.summary.stats.checks_failed, 1);
}

#[test]
fn a_recorded_run_replays_to_the_same_updates() {
    let blocks = chain(vec![
        vec![sync(genuine(), 0, 10, 20), sync(FORGED, 1, 1, 1)],
        vec![sync(genuine(), 0, 11, 19)],
    ]);
    // With shadow checks on, so the replay only matches if it rebuilds the same config.
    let checked = EngineConfig {
        check_every: Some(1),
        ..EngineConfig::base()
    };
    let live = run(checked.clone(), blocks, node());
    assert!(live.summary.stats.checks_passed + live.summary.stats.checks_failed > 0);

    let replay = Replay::new(live.recording);
    let config = EngineConfig::decode(&replay.config().unwrap()).unwrap();
    assert_eq!(config, checked);
    let outbox = InMemoryOutbox::default();
    let engine = Engine::new(
        config,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(outbox.clone()),
    );
    assert_eq!(replay.run(engine.run()).unwrap(), live.summary);
    assert_eq!(outbox.updates(), live.updates);
}

#[test]
fn a_gap_stops_the_engine() {
    let mut blocks = chain(vec![vec![], vec![], vec![]]);
    blocks.remove(1);
    let result = det::run_simulated(async {
        let engine = Engine::new(
            EngineConfig::base(),
            Box::new(SimClock::starting_at(START)),
            Box::new(SimEventSource::new(blocks)),
            Box::new(SimRpc::new(|_: EthCall| {
                (Duration::ZERO, CallResult::Failed("unused".into()))
            })),
            Box::new(InMemoryOutbox::default()),
        );
        engine.run().await
    });
    assert_eq!(
        result,
        Err(engine::EngineError::Gap {
            expected: 101,
            received: 102
        })
    );
}

#[test]
fn a_changed_parent_is_counted_as_a_reorg() {
    let mut blocks = chain(vec![vec![], vec![], vec![]]);
    blocks[2].1.parent_hash = B256::repeat_byte(0xee);
    let run = run(EngineConfig::base(), blocks, Node::default());
    assert_eq!(run.summary.stats.reorgs_detected, 1);
    assert_eq!(run.summary.head, Some((102, hash(102))));
}
