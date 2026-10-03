//! Trade history (#80): when a pool becomes tracked, the engine reads its Swaps from the window
//! before it, a batch of pools and a range of blocks at a time, newest first, splitting a read the
//! node refuses; and a replay of the recording publishes the same history.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use alloy_primitives::aliases::{I24, U160};
use alloy_primitives::{I256, U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{
    InMemorySink, Recorder, RecordingClock, RecordingEventSource, RecordingRpc, Replay, SimClock,
    SimEventSource, SimRpc, Source,
};
use engine::{Engine, EngineConfig, InMemoryOutbox, Side, Summary, Trade, Venue};
use types::Timestamp;
use types::chain::{
    Address, B256, Block, BlockLog, Bytes, CallResult, EthCall, Log, LogsQuery, LogsResult,
};
use venues::{multicall, v2, v3};

/// The session's first block, and its timestamp (2026-01-01T00:00:00Z).
const FIRST: u64 = 50_000;
const T0: u64 = 1_767_225_600;
const START: Timestamp = Timestamp::from_unix_nanos(T0 * 1_000_000_000);
const WETH: Address = address!("4200000000000000000000000000000000000006");
const USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
/// Sorts before WETH and USDC, so it's token0 in its pools.
const LOW: Address = address!("1000000000000000000000000000000000000001");
const OTHER: Address = address!("2000000000000000000000000000000000000002");
/// Sorts after WETH and USDC, so it's token1 in its pools.
const HIGH: Address = address!("f00000000000000000000000000000000000000f");
const TRADER: Address = address!("7000000000000000000000000000000000000007");
const ROUTER: Address = address!("8000000000000000000000000000000000000008");

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

/// Every 2s, before and after the session's first block.
fn time(number: u64) -> u64 {
    T0 + 2 * number - 2 * FIRST
}

fn log(address: Address, topics: Vec<B256>, data: Vec<u8>) -> Log {
    Log {
        address,
        topics,
        data: Bytes::from(data),
        log_index: 0,
        transaction_hash: B256::repeat_byte(0xaa),
    }
}

fn pair(token0: Address, token1: Address) -> Address {
    v2::BASE.pair_address(token0, token1)
}

fn pair_created(token0: Address, token1: Address) -> Log {
    log(
        v2::BASE.factory,
        vec![
            v2::PairCreated::SIGNATURE_HASH,
            token0.into_word(),
            token1.into_word(),
        ],
        (pair(token0, token1), U256::from(1u64)).abi_encode(),
    )
}

fn sync(pair: Address, reserve0: u64, reserve1: u64) -> Log {
    log(
        pair,
        vec![v2::Sync::SIGNATURE_HASH],
        (U256::from(reserve0), U256::from(reserve1)).abi_encode(),
    )
}

/// amount0In, amount1In, amount0Out, amount1Out.
fn v2_swap(pair: Address, amounts: [u64; 4]) -> Log {
    log(
        pair,
        vec![
            v2::Swap::SIGNATURE_HASH,
            ROUTER.into_word(),
            TRADER.into_word(),
        ],
        amounts.map(U256::from).abi_encode(),
    )
}

fn pool(token0: Address, token1: Address) -> Address {
    v3::BASE.pool_address(token0, token1, 500)
}

/// The pool's signed deltas: positive is what it took in.
fn v3_swap(pool: Address, amount0: i64, amount1: i64) -> Log {
    log(
        pool,
        vec![
            v3::Swap::SIGNATURE_HASH,
            ROUTER.into_word(),
            TRADER.into_word(),
        ],
        (
            I256::try_from(amount0).unwrap(),
            I256::try_from(amount1).unwrap(),
            U160::from(1u64) << 96,
            1_000u128,
            I24::try_from(0).unwrap(),
        )
            .abi_encode(),
    )
}

/// Blocks from `FIRST`, one every 2s, numbering each block's logs in order.
fn chain(blocks: Vec<Vec<Log>>) -> Vec<(Duration, Block)> {
    blocks
        .into_iter()
        .enumerate()
        .map(|(i, mut logs)| {
            for (index, log) in logs.iter_mut().enumerate() {
                log.log_index = index as u64;
            }
            let number = FIRST + i as u64;
            let block = Block {
                number,
                hash: hash(number),
                parent_hash: hash(number - 1),
                timestamp: time(number),
                logs,
                chain_head: None,
            };
            (Duration::from_secs(2 * (i as u64 + 1)), block)
        })
        .collect()
}

/// A node that knows each contract's tokens and, for v3 pools, a fixed price, no liquidity and
/// no ticks.
#[derive(Clone, Default)]
struct Node {
    tokens: BTreeMap<Address, (Address, Address)>,
}

impl Node {
    fn knowing(contracts: &[(Address, (Address, Address))]) -> Self {
        Self {
            tokens: contracts.iter().copied().collect(),
        }
    }

    fn answer(&self, call: &EthCall) -> CallResult {
        let results: Vec<Option<Vec<u8>>> = multicall::decode_calls(&call.data)
            .unwrap()
            .into_iter()
            .map(|(target, data)| {
                let (token0, token1) = *self.tokens.get(&target)?;
                let selector = &data[..4];
                Some(if selector == &v3::token0_call()[..4] {
                    v3::answers::address(token0)
                } else if selector == &v3::token1_call()[..4] {
                    v3::answers::address(token1)
                } else if selector == &v3::fee_call()[..4] {
                    v3::answers::fee(500)
                } else if selector == &v3::tick_spacing_call()[..4] {
                    v3::answers::tick_spacing(10)
                } else if selector == &v3::slot0_call()[..4] {
                    v3::answers::slot0(v3::Price {
                        sqrt_price_x96: U256::from(1u64) << 96,
                        tick: 0,
                    })
                } else if selector == &v3::liquidity_call()[..4] {
                    v3::answers::liquidity(0)
                } else if selector == &v3::tick_bitmap_call(0)[..4] {
                    v3::answers::tick_bitmap(U256::ZERO)
                } else {
                    return None;
                })
            })
            .collect();
        CallResult::Returned(Bytes::from(multicall::encode_results(&results)))
    }
}

/// The chain's logs as a ranged read sees them: `earlier`, from before the session, then the
/// session's own blocks. Every read takes `latency`, and one that `refuses` is refused.
struct LogsNode {
    logs: Vec<BlockLog>,
    latency: Duration,
    refuses: fn(&LogsQuery) -> bool,
}

impl LogsNode {
    fn new(earlier: Vec<(u64, Log)>, blocks: &[(Duration, Block)], latency: Duration) -> Self {
        let mut logs: Vec<BlockLog> = Vec::new();
        for (number, mut log) in earlier {
            log.log_index = logs.iter().filter(|l| l.block_number == number).count() as u64;
            logs.push(BlockLog {
                block_number: number,
                block_hash: hash(number),
                block_timestamp: time(number),
                log,
            });
        }
        for (_, block) in blocks {
            for log in &block.logs {
                logs.push(BlockLog {
                    block_number: block.number,
                    block_hash: block.hash,
                    block_timestamp: block.timestamp,
                    log: log.clone(),
                });
            }
        }
        logs.sort_by_key(|l| (l.block_number, l.log.log_index));
        Self {
            logs,
            latency,
            refuses: |_| false,
        }
    }

    fn answer(&self, query: &LogsQuery) -> LogsResult {
        if (self.refuses)(query) {
            return LogsResult::Failed("backend response too large".into());
        }
        LogsResult::Returned(
            self.logs
                .iter()
                .filter(|l| {
                    query.addresses.contains(&l.log.address)
                        && query.topics.contains(&l.log.topics[0])
                        && (query.from_block..=query.to_block).contains(&l.block_number)
                })
                .cloned()
                .collect(),
        )
    }
}

struct Run {
    summary: Summary,
    live: Vec<Trade>,
    history: Vec<Trade>,
    asked: Vec<LogsQuery>,
    recording: Vec<det::InputRecord>,
}

fn history_of(blocks: u64) -> EngineConfig {
    EngineConfig {
        history_blocks: Some(blocks),
        ..EngineConfig::base()
    }
}

fn run(config: EngineConfig, blocks: Vec<(Duration, Block)>, node: Node, logs: LogsNode) -> Run {
    let sink = InMemorySink::default();
    let outbox = InMemoryOutbox::default();
    let asked = Rc::new(RefCell::new(Vec::new()));
    let summary = det::run_simulated(async {
        let clock = SimClock::starting_at(START);
        let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock.clone()));
        recorder.record_config(config.encode());
        let rpc = SimRpc::new(move |call: EthCall| (Duration::from_millis(50), node.answer(&call)));
        let asking = asked.clone();
        let log_rpc = SimRpc::new(move |query: LogsQuery| {
            asking.borrow_mut().push(query.clone());
            (logs.latency, logs.answer(&query))
        });
        let engine = Engine::new(
            config,
            Box::new(RecordingClock::new(Box::new(clock), recorder.clone())),
            Box::new(RecordingEventSource::new(
                Box::new(SimEventSource::new(blocks)),
                recorder.clone(),
            )),
            Box::new(RecordingRpc::new(Box::new(rpc), recorder.clone())),
            Box::new(outbox.clone()),
        )
        .with_logs(Box::new(RecordingRpc::logs(Box::new(log_rpc), recorder)));
        engine.run().await.unwrap()
    });
    Run {
        summary,
        live: outbox.trades(),
        history: outbox.history(),
        asked: asked.take(),
        recording: sink.records(),
    }
}

/// (pool, block number, log index) of each trade.
fn positions(trades: &[Trade]) -> Vec<(Address, u64, u64)> {
    trades
        .iter()
        .map(|t| (t.pool, t.block_number, t.log_index))
        .collect()
}

/// (addresses, first block, last block) of each read.
fn reads(asked: &[LogsQuery]) -> Vec<(Vec<Address>, u64, u64)> {
    asked
        .iter()
        .map(|q| (q.addresses.clone(), q.from_block, q.to_block))
        .collect()
}

fn sorted(mut addresses: Vec<Address>) -> Vec<Address> {
    addresses.sort();
    addresses
}

fn p1() -> Address {
    pair(LOW, WETH)
}

fn p2() -> Address {
    pair(OTHER, WETH)
}

fn v() -> Address {
    pool(LOW, USDC)
}

/// Created during the session.
fn new_pair() -> Address {
    pair(WETH, HIGH)
}

/// p1 is first seen at `FIRST`, p2 a block later and the v3 pool v a block after that; the new
/// pair is created during the session. Each read takes 5s, so p2 and v wait for p1's read and are
/// read together, over the range both want.
fn mid_life_pools() -> (Vec<(Duration, Block)>, Node, LogsNode) {
    let mut blocks = vec![
        vec![
            sync(p1(), 1_000, 2_000),
            v2_swap(p1(), [0, 200, 100, 0]),
            pair_created(WETH, HIGH),
        ],
        vec![sync(p2(), 5_000, 1_000), v2_swap(p2(), [500, 0, 0, 100])],
        vec![
            v3_swap(v(), -10, 30),
            sync(new_pair(), 10, 20),
            v2_swap(new_pair(), [10, 0, 0, 20]),
            sync(p1(), 1_100, 1_800),
            v2_swap(p1(), [100, 0, 0, 200]),
        ],
    ];
    // Long enough for every read to start before the blocks end.
    blocks.resize(9, vec![]);
    let blocks = chain(blocks);
    let earlier = vec![
        (48_999, v2_swap(p1(), [0, 10, 5, 0])),
        (49_000, v2_swap(p1(), [0, 20, 10, 0])),
        (49_001, v2_swap(p2(), [50, 0, 0, 10])),
        (49_001, v3_swap(v(), -1, 3)),
        (49_500, v3_swap(v(), -2, 6)),
        (49_800, v2_swap(p2(), [100, 0, 0, 20])),
        (49_999, v2_swap(p1(), [40, 0, 0, 80])),
    ];
    let logs = LogsNode::new(earlier, &blocks, Duration::from_secs(5));
    let node = Node::knowing(&[
        (p1(), (LOW, WETH)),
        (p2(), (OTHER, WETH)),
        (v(), (LOW, USDC)),
    ]);
    (blocks, node, logs)
}

#[test]
fn a_pool_first_seen_mid_life_gets_the_window_before_it_as_history() {
    let (blocks, node, logs) = mid_life_pools();
    let run = run(history_of(1_000), blocks, node, logs);

    // p1 wants 49,000 to 49,999; p2 49,001 to 50,000; v 49,002 to 50,001. p2's and v's read
    // covers both their ranges, newest 1,000 blocks first. The new pair has no history to read.
    assert_eq!(
        reads(&run.asked),
        vec![
            (vec![p1()], 49_000, 49_999),
            (sorted(vec![p2(), v()]), 49_002, 50_001),
            (sorted(vec![p2(), v()]), 49_001, 49_001),
        ]
    );
    // Each pool's history stops where its live trades start, and starts with its window: p2's
    // swap at 50,001 is live, v's at 49,001 is outside v's window, and p1's at 48,999 is outside
    // its own.
    assert_eq!(
        positions(&run.history),
        vec![
            (p1(), 49_000, 0),
            (p1(), 49_999, 0),
            (v(), 49_500, 0),
            (p2(), 49_800, 0),
            (p2(), 49_001, 0),
        ]
    );
    let mut live = positions(&run.live);
    live.sort();
    let mut expected_live = vec![
        (p1(), 50_000, 1),
        (p2(), 50_001, 1),
        (v(), 50_002, 0),
        (new_pair(), 50_002, 2),
        (p1(), 50_002, 4),
    ];
    expected_live.sort();
    assert_eq!(live, expected_live);

    let s = &run.summary.stats;
    assert_eq!(
        (
            s.history_pools,
            s.history_reads,
            s.history_read_failures,
            s.history_blocks_dropped,
            s.history_trades
        ),
        (3, 3, 0, 0, 5)
    );
    let text = run.summary.to_string();
    let line = text.lines().find(|l| l.starts_with("history:")).unwrap();
    assert!(
        line.starts_with(
            "history: 3 pools, 3 reads (0 failed, 0 blocks dropped), 5 trades, digest "
        ),
        "{line}"
    );
}

#[test]
fn a_history_trade_is_the_same_record_as_a_live_one() {
    let (blocks, node, logs) = mid_life_pools();
    let run = run(history_of(1_000), blocks, node, logs);
    // p1 at 49,000: the pair took in 20 WETH and paid out 10 LOW.
    let trade = &run.history[0];
    assert_eq!(
        (
            trade.venue,
            trade.token,
            trade.quote,
            trade.side,
            trade.token_amount,
            trade.quote_amount,
        ),
        (
            Venue::UniswapV2,
            LOW,
            WETH,
            Side::Buy,
            U256::from(10u64),
            U256::from(20u64)
        )
    );
    assert_eq!(
        trade.price_e36,
        Some(U256::from(2u64) * U256::from(10u64).pow(U256::from(36u64)))
    );
    assert_eq!(
        (trade.block_hash, trade.block_timestamp, trade.tx_hash),
        (hash(49_000), T0 - 2_000, B256::repeat_byte(0xaa))
    );
    assert_eq!((trade.sender, trade.recipient), (ROUTER, TRADER));
    assert_eq!(trade.chain_id, 8453);
}

#[test]
fn a_refused_read_is_split_newer_half_first_and_a_block_refused_alone_is_dropped() {
    let blocks = chain(vec![
        vec![sync(p1(), 1_000, 2_000), v2_swap(p1(), [0, 200, 100, 0])],
        vec![],
        vec![],
    ]);
    let earlier = (49_992..=49_999)
        .map(|n| (n, v2_swap(p1(), [0, 20, 10, 0])))
        .collect();
    let mut logs = LogsNode::new(earlier, &blocks, Duration::from_millis(100));
    // More than two blocks is too large, and so is any read with block 49,997 in it.
    logs.refuses =
        |q| q.to_block - q.from_block >= 2 || (q.from_block..=q.to_block).contains(&49_997);
    let run = run(
        history_of(8),
        blocks,
        Node::knowing(&[(p1(), (LOW, WETH))]),
        logs,
    );
    let ranges: Vec<(u64, u64)> = run
        .asked
        .iter()
        .map(|q| (q.from_block, q.to_block))
        .collect();
    assert_eq!(
        ranges,
        vec![
            (49_992, 49_999),
            (49_996, 49_999),
            (49_998, 49_999),
            (49_996, 49_997),
            (49_997, 49_997),
            (49_996, 49_996),
            (49_992, 49_995),
            (49_994, 49_995),
            (49_992, 49_993),
        ]
    );
    let blocks_read: Vec<u64> = run.history.iter().map(|t| t.block_number).collect();
    assert_eq!(
        blocks_read,
        vec![49_998, 49_999, 49_996, 49_994, 49_995, 49_992, 49_993]
    );
    let s = &run.summary.stats;
    assert_eq!(
        (
            s.history_reads,
            s.history_read_failures,
            s.history_blocks_dropped,
            s.history_trades
        ),
        (9, 5, 1, 7)
    );
}

#[test]
fn a_read_asks_for_at_most_fifty_pools() {
    // 51 pairs, all first seen in the same block and proven by the same call.
    let tokens: Vec<Address> = (1..=51u8)
        .map(|i| Address::left_padding_from(&[0x10, i]))
        .collect();
    let pairs: Vec<Address> = tokens.iter().map(|&t| pair(t, WETH)).collect();
    let blocks = chain(vec![
        pairs.iter().map(|&p| sync(p, 1_000, 2_000)).collect(),
        vec![],
        vec![],
    ]);
    let contracts: Vec<(Address, (Address, Address))> = tokens
        .iter()
        .zip(&pairs)
        .map(|(&t, &p)| (p, (t, WETH)))
        .collect();
    let logs = LogsNode::new(vec![], &blocks, Duration::from_millis(100));
    let run = run(history_of(1_000), blocks, Node::knowing(&contracts), logs);
    assert_eq!(
        reads(&run.asked),
        vec![
            (sorted(pairs[..50].to_vec()), 49_000, 49_999),
            (vec![pairs[50]], 49_000, 49_999),
        ]
    );
    assert_eq!(run.summary.stats.history_pools, 51);
}

#[test]
fn once_the_blocks_end_no_new_read_starts() {
    // p1 wants three ranges, but the blocks end at 4s, while its first read takes until 7.05s.
    let blocks = chain(vec![
        vec![sync(p1(), 1_000, 2_000), v2_swap(p1(), [0, 200, 100, 0])],
        vec![],
    ]);
    let earlier = vec![
        (47_500, v2_swap(p1(), [0, 20, 10, 0])),
        (48_500, v2_swap(p1(), [0, 20, 10, 0])),
        (49_500, v2_swap(p1(), [0, 20, 10, 0])),
    ];
    let logs = LogsNode::new(earlier, &blocks, Duration::from_secs(5));
    let run = run(
        history_of(3_000),
        blocks,
        Node::knowing(&[(p1(), (LOW, WETH))]),
        logs,
    );
    assert_eq!(reads(&run.asked), vec![(vec![p1()], 49_000, 49_999)]);
    // The read in flight still lands.
    assert_eq!(positions(&run.history), vec![(p1(), 49_500, 0)]);
}

#[test]
fn without_history_blocks_nothing_is_read() {
    let (blocks, node, logs) = mid_life_pools();
    let run = run(EngineConfig::base(), blocks, node, logs);
    assert!(run.asked.is_empty());
    assert!(run.history.is_empty());
    assert_eq!(run.summary.history_digest, None);
    assert!(!run.summary.to_string().contains("history"));
}

#[test]
fn a_recording_replays_to_the_same_history() {
    let (blocks, node, logs) = mid_life_pools();
    let live = run(history_of(1_000), blocks, node, logs);
    let log_reads = live
        .recording
        .iter()
        .filter(|r| r.source == Source::Logs)
        .count();
    assert_eq!(log_reads, 3);

    let replay = Replay::new(live.recording.clone());
    let config = EngineConfig::decode(&replay.config().unwrap()).unwrap();
    assert_eq!(config.history_blocks, Some(1_000));
    let outbox = InMemoryOutbox::default();
    let engine = Engine::new(
        config,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(outbox.clone()),
    )
    .with_logs(Box::new(replay.logs()));
    let summary = replay.run(engine.run()).unwrap();
    assert_eq!(outbox.history(), live.history);
    assert_eq!(summary, live.summary);
}
