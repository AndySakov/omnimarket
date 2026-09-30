//! The engine's v2 tracking against what indexer.md, data.md and D71 say, on a simulated chain.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use alloy_primitives::{U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{SimClock, SimEventSource, SimRpc};
use engine::{Engine, EngineConfig, InMemoryOutbox, PoolState, PoolUpdate, Summary};
use types::Timestamp;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall, Log};
use venues::multicall;
use venues::v2::{self, BASE, Reserves, Sync};

const START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);
const TOKEN_A: Address = address!("1000000000000000000000000000000000000001");
const TOKEN_B: Address = address!("2000000000000000000000000000000000000002");
const TOKEN_C: Address = address!("3000000000000000000000000000000000000003");
const TOKEN_D: Address = address!("4000000000000000000000000000000000000004");
const FORGED: Address = address!("f000000000000000000000000000000000000000");

fn pair(token0: Address, token1: Address) -> Address {
    BASE.pair_address(token0, token1)
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

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

/// Blocks from 100, one every 2s.
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

/// What one `aggregate3` call asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Asked {
    Tokens { block: u64, pairs: Vec<Address> },
    Reserves { block: u64, pairs: Vec<Address> },
}

/// A node that knows pairs' tokens, answers reserves from `reserves`, fails its first
/// `failures` calls as a whole, and logs what it was asked.
struct Node {
    tokens: BTreeMap<Address, (Address, Address)>,
    reserves: Reserves,
    latency: Duration,
    failures: u32,
    asked: Rc<RefCell<Vec<Asked>>>,
}

impl Node {
    fn new(pairs: &[(Address, Address)]) -> Self {
        let mut tokens: BTreeMap<_, _> = pairs.iter().map(|&(a, b)| (pair(a, b), (a, b))).collect();
        tokens.insert(FORGED, (TOKEN_A, TOKEN_B));
        Self {
            tokens,
            reserves: Reserves {
                reserve0: 0,
                reserve1: 0,
            },
            latency: Duration::from_millis(50),
            failures: 0,
            asked: Rc::default(),
        }
    }

    fn answer(&mut self, call: &EthCall) -> CallResult {
        let calls = multicall::decode_calls(&call.data).unwrap();
        let reads_tokens = calls[0].1.as_ref() == v2::token0_call().as_slice();
        let mut pairs: Vec<Address> = calls.iter().map(|(target, _)| *target).collect();
        pairs.dedup();
        self.asked.borrow_mut().push(if reads_tokens {
            Asked::Tokens {
                block: call.block,
                pairs,
            }
        } else {
            Asked::Reserves {
                block: call.block,
                pairs,
            }
        });
        if self.failures > 0 {
            self.failures -= 1;
            return CallResult::Failed("block not found".into());
        }
        let results: Vec<Option<Vec<u8>>> = calls
            .into_iter()
            .map(|(target, data)| {
                let tokens = self.tokens.get(&target);
                if data.as_ref() == v2::token0_call().as_slice() {
                    tokens.map(|t| v2::encode_address(t.0))
                } else if data.as_ref() == v2::token1_call().as_slice() {
                    tokens.map(|t| v2::encode_address(t.1))
                } else {
                    Some(v2::encode_reserves(self.reserves))
                }
            })
            .collect();
        CallResult::Returned(Bytes::from(multicall::encode_results(&results)))
    }
}

fn run(
    config: EngineConfig,
    blocks: Vec<(Duration, Block)>,
    mut node: Node,
) -> (Summary, Vec<PoolUpdate>) {
    let outbox = InMemoryOutbox::default();
    let latency = node.latency;
    let summary = det::run_simulated(async {
        let engine = Engine::new(
            config,
            Box::new(SimClock::starting_at(START)),
            Box::new(SimEventSource::new(blocks)),
            Box::new(SimRpc::new(move |call: EthCall| {
                (latency, node.answer(&call))
            })),
            Box::new(outbox.clone()),
        );
        engine.run().await.unwrap()
    });
    (summary, outbox.updates())
}

fn v2(reserve0: u128, reserve1: u128) -> PoolState {
    PoolState::V2(Reserves { reserve0, reserve1 })
}

fn history(updates: &[PoolUpdate]) -> Vec<(Address, u64, Option<PoolState>, PoolState)> {
    updates
        .iter()
        .map(|u| (u.pool, u.block_number, u.before.clone(), u.after.clone()))
        .collect()
}

/// data.md: BLAKE3 over the kind (length-prefixed, u64 little-endian) and the key fields,
/// fixed-width little-endian in the order listed, cut to 128 bits. Written out here from the
/// doc, not from the code.
fn documented_id(kind: &str, fields: &[&[u8]]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(kind.len() as u64).to_le_bytes());
    hasher.update(kind.as_bytes());
    for field in fields {
        hasher.update(field);
    }
    hasher.finalize().as_bytes()[..16].try_into().unwrap()
}

// data.md: a pool update keys on (chain id, pool, block hash, log index) and is caused by the
// chain event keyed on (chain id, block hash, log index).
#[test]
fn pool_update_lineage_follows_data_md() {
    let ab = pair(TOKEN_A, TOKEN_B);
    let (_, updates) = run(
        EngineConfig::base(),
        chain(vec![vec![sync(ab, 7, 10, 20)]]),
        Node::new(&[(TOKEN_A, TOKEN_B)]),
    );
    let proto = updates[0].to_proto();
    let lineage = proto.lineage.unwrap();
    let chain_id = 8453u64.to_le_bytes();
    let log_index = 7u64.to_le_bytes();
    assert_eq!(
        lineage.id,
        documented_id(
            "pool_update",
            &[&chain_id, ab.as_slice(), hash(100).as_slice(), &log_index]
        )
    );
    assert_eq!(
        lineage.caused_by,
        vec![
            documented_id(
                "chain_event",
                &[&chain_id, hash(100).as_slice(), &log_index]
            )
            .to_vec()
        ]
    );
}

// indexer.md: "Its token0() and token1() are read in one Multicall3 aggregate3 call per 100
// new pairs, at the block it was seen."
#[test]
fn a_pair_is_proven_at_the_block_it_was_first_seen() {
    let ab = pair(TOKEN_A, TOKEN_B);
    let node = Node::new(&[(TOKEN_A, TOKEN_B)]);
    let asked = node.asked.clone();
    run(
        EngineConfig::base(),
        chain(vec![
            vec![],
            vec![sync(ab, 0, 10, 20)],
            vec![sync(ab, 0, 11, 19)],
        ]),
        node,
    );
    assert_eq!(
        *asked.borrow(),
        [Asked::Tokens {
            block: 101,
            pairs: vec![ab]
        }]
    );
}

// indexer.md: "A pair first seen trading is held unproven, with its Syncs buffered in order."
// The proof takes 3s, so two more blocks trade the pair before it comes back.
#[test]
fn syncs_seen_while_the_proof_is_in_flight_apply_in_order() {
    let ab = pair(TOKEN_A, TOKEN_B);
    let mut node = Node::new(&[(TOKEN_A, TOKEN_B)]);
    node.latency = Duration::from_secs(3);
    let (summary, updates) = run(
        EngineConfig::base(),
        chain(vec![
            vec![sync(ab, 0, 10, 20)],
            vec![sync(ab, 0, 11, 19)],
            vec![sync(ab, 0, 12, 18)],
        ]),
        node,
    );
    assert_eq!(
        history(&updates),
        [
            (ab, 100, None, v2(10, 20)),
            (ab, 101, Some(v2(10, 20)), v2(11, 19)),
            (ab, 102, Some(v2(11, 19)), v2(12, 18)),
        ]
    );
    assert_eq!(summary.stats.verify_calls, 1);
}

// indexer.md: "A verification call that fails as a whole forgets its pairs, and each is
// proven again the next time it trades."
#[test]
fn a_failed_proof_is_retried_on_the_next_trade() {
    let ab = pair(TOKEN_A, TOKEN_B);
    let mut node = Node::new(&[(TOKEN_A, TOKEN_B)]);
    node.failures = 1;
    let asked = node.asked.clone();
    let (summary, updates) = run(
        EngineConfig::base(),
        chain(vec![
            vec![sync(ab, 0, 10, 20)],
            vec![],
            vec![sync(ab, 0, 11, 19)],
        ]),
        node,
    );
    assert_eq!(history(&updates), [(ab, 102, None, v2(11, 19))]);
    assert_eq!(summary.stats.verify_failures, 1);
    assert_eq!(summary.stats.pairs_tracked, 1);
    let blocks: Vec<u64> = asked
        .borrow()
        .iter()
        .map(|a| match a {
            Asked::Tokens { block, .. } | Asked::Reserves { block, .. } => *block,
        })
        .collect();
    assert_eq!(blocks, [100, 102]);
}

// indexer.md: "otherwise it is rejected for good (a fork or a fake)".
#[test]
fn a_rejected_pair_is_never_asked_about_again() {
    let node = Node::new(&[]);
    let asked = node.asked.clone();
    let (summary, updates) = run(
        EngineConfig::base(),
        chain(vec![
            vec![sync(FORGED, 0, 1, 1)],
            vec![sync(FORGED, 0, 2, 2)],
            vec![sync(FORGED, 0, 3, 3)],
        ]),
        node,
    );
    assert!(updates.is_empty());
    assert_eq!(summary.stats.pairs_rejected, 1);
    assert_eq!(asked.borrow().len(), 1);
}

// indexer.md: "every N blocks the engine takes the next 20 tracked pairs in address order,
// wrapping round, and reads their getReserves() at the block it just applied".
#[test]
fn the_shadow_check_walks_every_pair_in_turn() {
    let pairs = [(TOKEN_A, TOKEN_B), (TOKEN_A, TOKEN_C), (TOKEN_A, TOKEN_D)];
    let mut addresses: Vec<Address> = pairs.iter().map(|&(a, b)| pair(a, b)).collect();
    addresses.sort();
    let [p0, p1, p2] = addresses[..] else {
        unreachable!()
    };
    let node = Node::new(&pairs);
    let asked = node.asked.clone();
    let seeding: Vec<Log> = addresses
        .iter()
        .enumerate()
        .map(|(i, &a)| sync(a, i as u64, 0, 0))
        .collect();
    let config = EngineConfig {
        check_every: Some(1),
        check_sample: 2,
        ..EngineConfig::base()
    };
    run(config, chain(vec![seeding, vec![], vec![], vec![]]), node);
    let checks: Vec<Asked> = asked
        .borrow()
        .iter()
        .filter(|a| matches!(a, Asked::Reserves { .. }))
        .cloned()
        .collect();
    assert_eq!(
        checks,
        [
            Asked::Reserves {
                block: 101,
                pairs: vec![p0, p1]
            },
            Asked::Reserves {
                block: 102,
                pairs: vec![p2, p0]
            },
            Asked::Reserves {
                block: 103,
                pairs: vec![p1, p2]
            },
        ]
    );
}
