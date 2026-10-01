//! Trade records (#77, D97): one per Swap on a tracked pool, with the token, its quote asset,
//! side and execution price right whichever of the pool's tokens is the quote, and the same
//! stream on replay.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use alloy_primitives::aliases::{I24, U160};
use alloy_primitives::{I256, U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{
    InMemorySink, Recorder, RecordingClock, RecordingEventSource, RecordingRpc, Replay, SimClock,
    SimEventSource, SimRpc,
};
use engine::{Engine, EngineConfig, InMemoryOutbox, Side, Summary, Trade, Venue};
use types::Timestamp;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall, Log};
use venues::{multicall, v2, v3};

const START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);
const WETH: Address = address!("4200000000000000000000000000000000000006");
const USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
/// Sorts before WETH and USDC, so it's token0 in its pools.
const LOW: Address = address!("1000000000000000000000000000000000000001");
/// Sorts after WETH and USDC, so it's token1 in its pools.
const HIGH: Address = address!("f00000000000000000000000000000000000000f");
const OTHER: Address = address!("2000000000000000000000000000000000000002");
const TRADER: Address = address!("7000000000000000000000000000000000000007");
const ROUTER: Address = address!("8000000000000000000000000000000000000008");
const FORGED: Address = address!("fa00000000000000000000000000000000000000");

fn e36(quote: u64, token: u64) -> Option<U256> {
    Some(U256::from(quote) * U256::from(10u64).pow(U256::from(36u64)) / U256::from(token))
}

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

fn tx(n: u8) -> B256 {
    B256::repeat_byte(n)
}

fn log(address: Address, topics: Vec<B256>, data: Vec<u8>) -> Log {
    Log {
        address,
        topics,
        data: Bytes::from(data),
        log_index: 0,
        transaction_hash: tx(0xaa),
    }
}

// v2 logs.

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

// v3 logs.

fn pool(token0: Address, token1: Address) -> Address {
    v3::BASE.pool_address(token0, token1, 500)
}

fn pool_created(token0: Address, token1: Address) -> Log {
    log(
        v3::BASE.factory,
        vec![
            v3::PoolCreated::SIGNATURE_HASH,
            token0.into_word(),
            token1.into_word(),
            B256::from(U256::from(500u64)),
        ],
        (I24::try_from(10).unwrap(), pool(token0, token1)).abi_encode(),
    )
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

fn v3_mint(pool: Address) -> Log {
    let tick = |t: i32| B256::from(I256::try_from(t).unwrap());
    log(
        pool,
        vec![v3::Mint::SIGNATURE_HASH, B256::ZERO, tick(-10), tick(10)],
        (Address::ZERO, 5u128, U256::ZERO, U256::ZERO).abi_encode(),
    )
}

/// Blocks from 100, one every 2s, numbering each block's logs in order.
fn chain(blocks: Vec<Vec<Log>>) -> Vec<(Duration, Block)> {
    blocks
        .into_iter()
        .enumerate()
        .map(|(i, mut logs)| {
            for (index, log) in logs.iter_mut().enumerate() {
                log.log_index = index as u64;
            }
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

/// A node that knows each contract's tokens and, for v3 pools, a fixed price, no liquidity and
/// no ticks. It refuses every call at the blocks in `refuse`.
#[derive(Clone, Default)]
struct Node {
    tokens: BTreeMap<Address, (Address, Address)>,
    refuse: BTreeSet<u64>,
}

impl Node {
    fn knowing(contracts: &[(Address, (Address, Address))]) -> Self {
        Self {
            tokens: contracts.iter().copied().collect(),
            refuse: BTreeSet::new(),
        }
    }

    fn answer(&self, call: &EthCall) -> CallResult {
        if self.refuse.contains(&call.block) {
            return CallResult::Failed("refused".into());
        }
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

struct Run {
    summary: Summary,
    trades: Vec<Trade>,
    recording: Vec<det::InputRecord>,
}

fn run(blocks: Vec<(Duration, Block)>, node: Node) -> Run {
    let config = EngineConfig::base();
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
        trades: outbox.trades(),
        recording: sink.records(),
    }
}

/// What a trade says, without its lineage and log position.
#[derive(Debug, PartialEq, Eq)]
struct Says {
    pool: Address,
    venue: Venue,
    token: Address,
    quote: Address,
    side: Side,
    token_amount: U256,
    quote_amount: U256,
    price_e36: Option<U256>,
}

fn says(trade: &Trade) -> Says {
    Says {
        pool: trade.pool,
        venue: trade.venue,
        token: trade.token,
        quote: trade.quote,
        side: trade.side,
        token_amount: trade.token_amount,
        quote_amount: trade.quote_amount,
        price_e36: trade.price_e36,
    }
}

#[allow(clippy::too_many_arguments)]
fn expect(
    pool: Address,
    venue: Venue,
    token: Address,
    quote: Address,
    side: Side,
    token_amount: u64,
    quote_amount: u64,
) -> Says {
    Says {
        pool,
        venue,
        token,
        quote,
        side,
        token_amount: U256::from(token_amount),
        quote_amount: U256::from(quote_amount),
        price_e36: e36(quote_amount, token_amount),
    }
}

/// (block number, log index) of each trade.
fn positions(trades: &[Trade]) -> Vec<(u64, u64)> {
    trades
        .iter()
        .map(|t| (t.block_number, t.log_index))
        .collect()
}

#[test]
fn v2_side_and_price_are_right_whichever_token_is_the_quote() {
    // LOW/WETH: WETH, token1, is the quote. WETH/HIGH: WETH, token0, is the quote.
    let quote1 = pair(LOW, WETH);
    let quote0 = pair(WETH, HIGH);
    let blocks = chain(vec![
        vec![pair_created(LOW, WETH), pair_created(WETH, HIGH)],
        vec![
            // Buys 1,000 LOW with 2,000 WETH: the pair takes token1 in and pays token0 out.
            sync(quote1, 9_000, 12_000),
            v2_swap(quote1, [0, 2_000, 1_000, 0]),
            // Sells 400 LOW for 1,000 WETH.
            sync(quote1, 9_400, 11_000),
            v2_swap(quote1, [400, 0, 0, 1_000]),
            // Buys 300 HIGH with 900 WETH: the pair takes token0 in and pays token1 out.
            sync(quote0, 900, 700),
            v2_swap(quote0, [900, 0, 0, 300]),
            // Sells 500 HIGH for 250 WETH.
            sync(quote0, 650, 1_200),
            v2_swap(quote0, [0, 500, 250, 0]),
        ],
    ]);
    let run = run(blocks, Node::default());
    let got: Vec<Says> = run.trades.iter().map(says).collect();
    let v2 = Venue::UniswapV2;
    assert_eq!(
        got,
        vec![
            expect(quote1, v2, LOW, WETH, Side::Buy, 1_000, 2_000),
            expect(quote1, v2, LOW, WETH, Side::Sell, 400, 1_000),
            expect(quote0, v2, HIGH, WETH, Side::Buy, 300, 900),
            expect(quote0, v2, HIGH, WETH, Side::Sell, 500, 250),
        ]
    );
    assert_eq!(
        positions(&run.trades),
        vec![(101, 1), (101, 3), (101, 5), (101, 7)]
    );
    for trade in &run.trades {
        assert_eq!((trade.sender, trade.recipient), (ROUTER, TRADER));
        assert_eq!(trade.tx_hash, tx(0xaa));
        assert_eq!(trade.block_hash, hash(101));
        assert_eq!(trade.block_timestamp, 1_767_225_602);
    }
}

#[test]
fn v3_side_and_price_are_right_whichever_token_is_the_quote() {
    // LOW/USDC: USDC, token1, is the quote. USDC/HIGH: USDC, token0, is the quote.
    // WETH/USDC: both are quote assets; USDC comes first in Base's list, so WETH is the token.
    // LOW/OTHER: neither is; the token is token0.
    let quote1 = pool(LOW, USDC);
    let quote0 = pool(USDC, HIGH);
    let both = pool(WETH, USDC);
    let neither = pool(LOW, OTHER);
    let blocks = chain(vec![
        vec![
            pool_created(LOW, USDC),
            pool_created(USDC, HIGH),
            pool_created(WETH, USDC),
            pool_created(LOW, OTHER),
        ],
        vec![
            // Buys 1,000 LOW with 3,000 USDC.
            v3_swap(quote1, -1_000, 3_000),
            // Sells 1,000 LOW for 2,500 USDC.
            v3_swap(quote1, 1_000, -2_500),
            // Buys 40 HIGH with 10 USDC.
            v3_swap(quote0, 10, -40),
            // Sells 80 HIGH for 15 USDC.
            v3_swap(quote0, -15, 80),
            // Sells 2 WETH for 5,000 USDC.
            v3_swap(both, 2, -5_000),
            // Buys 7 LOW with 21 OTHER.
            v3_swap(neither, -7, 21),
        ],
    ]);
    let run = run(blocks, Node::default());
    let got: Vec<Says> = run.trades.iter().map(says).collect();
    let v3 = Venue::UniswapV3;
    assert_eq!(
        got,
        vec![
            expect(quote1, v3, LOW, USDC, Side::Buy, 1_000, 3_000),
            expect(quote1, v3, LOW, USDC, Side::Sell, 1_000, 2_500),
            expect(quote0, v3, HIGH, USDC, Side::Buy, 40, 10),
            expect(quote0, v3, HIGH, USDC, Side::Sell, 80, 15),
            expect(both, v3, WETH, USDC, Side::Sell, 2, 5_000),
            expect(neither, v3, LOW, OTHER, Side::Buy, 7, 21),
        ]
    );
}

#[test]
fn a_swap_that_moves_none_of_the_token_has_no_price_and_its_side_from_the_quote() {
    let p = pool(LOW, USDC);
    let blocks = chain(vec![
        vec![pool_created(LOW, USDC)],
        vec![v3_swap(p, 0, 3), v3_swap(p, 0, -3)],
    ]);
    let run = run(blocks, Node::default());
    let got: Vec<(Side, Option<U256>)> = run.trades.iter().map(|t| (t.side, t.price_e36)).collect();
    assert_eq!(got, vec![(Side::Buy, None), (Side::Sell, None)]);
}

#[test]
fn every_swap_on_a_tracked_pool_is_exactly_one_trade() {
    // Pools seen trading before they were known: proven (v2) or read (v3) at block 100, so
    // their swaps at 100 and 101 wait for the answer. Fakes trade too, and Sync, Mint and
    // creation logs are not trades.
    let v2_old = pair(LOW, WETH);
    let v3_old = pool(USDC, HIGH);
    let v2_known = pair(WETH, HIGH);
    let v3_known = pool(LOW, USDC);
    let node = Node::knowing(&[
        (v2_old, (LOW, WETH)),
        (v3_old, (USDC, HIGH)),
        (FORGED, (LOW, WETH)),
    ]);
    let blocks = chain(vec![
        vec![
            sync(v2_old, 10, 10),
            v2_swap(v2_old, [0, 1, 1, 0]),
            v3_mint(v3_old),
            v3_swap(v3_old, 1, -1),
            v3_swap(v3_old, 2, -2),
            sync(FORGED, 1, 1),
            v2_swap(FORGED, [1, 0, 0, 1]),
            v3_swap(FORGED, 1, -1),
            pair_created(WETH, HIGH),
            pool_created(LOW, USDC),
        ],
        vec![
            v3_swap(v3_old, 3, -3),
            sync(v2_old, 11, 9),
            v2_swap(v2_old, [1, 0, 0, 1]),
            sync(v2_known, 5, 5),
            v2_swap(v2_known, [1, 0, 0, 1]),
            v3_swap(v3_known, 4, -4),
            v3_swap(FORGED, 1, -1),
        ],
        vec![
            v3_swap(v3_old, 5, -5),
            sync(v2_old, 12, 8),
            v2_swap(v2_old, [1, 0, 0, 1]),
        ],
    ]);
    let run = run(blocks, node);

    let mut got: Vec<(Address, u64, u64)> = run
        .trades
        .iter()
        .map(|t| (t.pool, t.block_number, t.log_index))
        .collect();
    got.sort();
    let mut expected = vec![
        (v2_old, 100, 1),
        (v2_old, 101, 2),
        (v2_old, 102, 2),
        (v3_old, 100, 3),
        (v3_old, 100, 4),
        (v3_old, 101, 0),
        (v3_old, 102, 0),
        (v2_known, 101, 4),
        (v3_known, 101, 5),
    ];
    expected.sort();
    assert_eq!(got, expected);
    assert_eq!(run.summary.stats.trades, 9);
    assert_eq!(run.summary.stats.trades_dropped, 0);
    assert_eq!(run.summary.stats.pairs_rejected, 1);
    assert_eq!(run.summary.stats.pools_rejected, 1);
}

#[test]
fn swaps_waiting_on_a_read_that_fails_are_counted_as_dropped() {
    let v2_old = pair(LOW, WETH);
    let v3_old = pool(USDC, HIGH);
    let mut node = Node::knowing(&[(v2_old, (LOW, WETH)), (v3_old, (USDC, HIGH))]);
    node.refuse.insert(100);
    let blocks = chain(vec![
        vec![
            sync(v2_old, 10, 10),
            v2_swap(v2_old, [0, 1, 1, 0]),
            v3_swap(v3_old, 1, -1),
        ],
        vec![],
        // Seen again: proven and read at 102, and these trades are published.
        vec![
            sync(v2_old, 11, 9),
            v2_swap(v2_old, [1, 0, 0, 1]),
            v3_swap(v3_old, 2, -2),
        ],
        vec![],
    ]);
    let run = run(blocks, node);
    let mut got = positions(&run.trades);
    got.sort();
    assert_eq!(got, vec![(102, 1), (102, 2)]);
    assert_eq!(run.summary.stats.trades_dropped, 2);
}

#[test]
fn a_recorded_run_replays_to_the_same_trades() {
    let v2_old = pair(LOW, WETH);
    let v3_old = pool(USDC, HIGH);
    let node = Node::knowing(&[(v2_old, (LOW, WETH)), (v3_old, (USDC, HIGH))]);
    let blocks = chain(vec![
        vec![
            sync(v2_old, 10, 10),
            v2_swap(v2_old, [0, 1, 1, 0]),
            v3_swap(v3_old, 1, -1),
        ],
        vec![
            v3_swap(v3_old, 2, -2),
            sync(v2_old, 11, 9),
            v2_swap(v2_old, [1, 0, 0, 1]),
        ],
    ]);
    let live = run(blocks, node);
    assert_eq!(live.trades.len(), 4);

    let replay = Replay::new(live.recording);
    let config = EngineConfig::decode(&replay.config().unwrap()).unwrap();
    assert_eq!(config, EngineConfig::base());
    let outbox = InMemoryOutbox::default();
    let engine = Engine::new(
        config,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(outbox.clone()),
    );
    let replayed = replay.run(engine.run()).unwrap();
    assert_eq!(replayed, live.summary);
    assert_eq!(replayed.trades_digest, live.summary.trades_digest);
    assert_eq!(outbox.trades(), live.trades);
}

/// BLAKE3 over the length-prefixed kind and the key fields, cut to 128 bits: data.md's recipe,
/// written out independently of `LineageId`.
fn documented_id(kind: &str, fields: &[&[u8]]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(kind.len() as u64).to_le_bytes());
    hasher.update(kind.as_bytes());
    for field in fields {
        hasher.update(field);
    }
    let mut id = [0; 16];
    id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    id
}

#[test]
fn trades_carry_data_md_lineage_and_go_on_the_wire_as_trade_proto_says() {
    use proto::trade::v1::trade;

    let p = pool(LOW, USDC);
    let blocks = chain(vec![
        vec![pool_created(LOW, USDC)],
        vec![v3_mint(p), v3_swap(p, -0x0100, 0x0300)],
    ]);
    let run = run(blocks, Node::default());
    let [trade] = run.trades.as_slice() else {
        panic!("one trade")
    };
    let block_hash = hash(101);
    let key: [&[u8]; 3] = [
        &8453u64.to_le_bytes(),
        block_hash.as_slice(),
        &1u64.to_le_bytes(),
    ];
    assert_eq!(trade.id.as_bytes(), &documented_id("trade", &key));
    assert_eq!(
        trade.caused_by.as_bytes(),
        &documented_id("chain_event", &key)
    );

    let wire = trade.to_proto();
    assert_eq!(wire.chain_id, 8453);
    assert_eq!(wire.pool, p.to_vec());
    assert_eq!(wire.venue, trade::Venue::UniswapV3 as i32);
    assert_eq!(wire.token, LOW.to_vec());
    assert_eq!(wire.quote, USDC.to_vec());
    assert_eq!(wire.side, trade::Side::Buy as i32);
    assert_eq!(wire.token_amount, vec![0x01, 0x00]);
    assert_eq!(wire.quote_amount, vec![0x03, 0x00]);
    // 3 × 10^36.
    assert_eq!(
        wire.price_e36,
        Some(e36(3, 1).unwrap().to_be_bytes_trimmed_vec())
    );
    assert_eq!(wire.sender, ROUTER.to_vec());
    assert_eq!(wire.recipient, TRADER.to_vec());
    assert_eq!(wire.tx_hash, tx(0xaa).to_vec());
    assert_eq!(
        (wire.block_number, wire.log_index, wire.block_timestamp),
        (101, 1, 1_767_225_602)
    );
    assert_eq!(wire.block_hash, hash(101).to_vec());
    let lineage = wire.lineage.unwrap();
    assert_eq!(lineage.id, trade.id.as_bytes().to_vec());
    assert_eq!(lineage.caused_by, vec![trade.caused_by.as_bytes().to_vec()]);
}
