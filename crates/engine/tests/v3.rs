//! v3 pools on a simulated chain: discovery, the CREATE2 proof, the batched bootstrap of a pool
//! first seen mid-life, events after it, the shadow check, and record/replay.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use alloy_primitives::aliases::{I24, U160};
use alloy_primitives::{I256, U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use det::{
    InMemorySink, Recorder, RecordingClock, RecordingEventSource, RecordingRpc, Replay, SimClock,
    SimEventSource, SimRpc,
};
use engine::{Engine, EngineConfig, InMemoryOutbox, PoolState, PoolUpdate, Summary};
use types::Timestamp;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall, Log};
use venues::multicall;
use venues::v3::{self, BASE, Burn, Initialize, Mint, Pool, PoolCreated, Price, Swap};

const START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);
const TOKEN_A: Address = address!("1000000000000000000000000000000000000001");
const TOKEN_B: Address = address!("2000000000000000000000000000000000000002");
const TOKEN_C: Address = address!("3000000000000000000000000000000000000003");
const FORGED: Address = address!("f000000000000000000000000000000000000000");

fn old_pool() -> Address {
    BASE.pool_address(TOKEN_A, TOKEN_B, 3000)
}

fn new_pool() -> Address {
    BASE.pool_address(TOKEN_A, TOKEN_C, 500)
}

fn price(tick: i32) -> Price {
    Price {
        sqrt_price_x96: U256::from(1u64) << 96,
        tick,
    }
}

fn log(address: Address, topics: Vec<B256>, data: Vec<u8>, log_index: u64) -> Log {
    Log {
        address,
        topics,
        data: Bytes::from(data),
        log_index,
        transaction_hash: B256::ZERO,
    }
}

fn int24(v: i32) -> B256 {
    B256::from(I256::try_from(v).unwrap())
}

fn event_log(pool: Address, event: v3::Event, log_index: u64) -> Log {
    match event {
        v3::Event::Initialize(p) => log(
            pool,
            vec![Initialize::SIGNATURE_HASH],
            (U160::from(p.sqrt_price_x96), I24::try_from(p.tick).unwrap()).abi_encode(),
            log_index,
        ),
        v3::Event::Swap { price, liquidity } => log(
            pool,
            vec![Swap::SIGNATURE_HASH, B256::ZERO, B256::ZERO],
            (
                I256::ZERO,
                I256::ZERO,
                U160::from(price.sqrt_price_x96),
                liquidity,
                I24::try_from(price.tick).unwrap(),
            )
                .abi_encode(),
            log_index,
        ),
        v3::Event::Mint {
            lower,
            upper,
            amount,
        } => log(
            pool,
            vec![Mint::SIGNATURE_HASH, B256::ZERO, int24(lower), int24(upper)],
            (Address::ZERO, amount, U256::ZERO, U256::ZERO).abi_encode(),
            log_index,
        ),
        v3::Event::Burn {
            lower,
            upper,
            amount,
        } => log(
            pool,
            vec![Burn::SIGNATURE_HASH, B256::ZERO, int24(lower), int24(upper)],
            (amount, U256::ZERO, U256::ZERO).abi_encode(),
            log_index,
        ),
    }
}

fn pool_created(token0: Address, token1: Address, fee: u32, spacing: i32, pool: Address) -> Log {
    log(
        BASE.factory,
        vec![
            PoolCreated::SIGNATURE_HASH,
            token0.into_word(),
            token1.into_word(),
            B256::from(U256::from(fee)),
        ],
        (I24::try_from(spacing).unwrap(), pool).abi_encode(),
        0,
    )
}

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

/// A node holding every pool's true state after each block, built by applying the same events
/// with the venue code the engine uses. Counts its calls.
#[derive(Clone, Default)]
struct Node {
    truth: BTreeMap<u64, BTreeMap<Address, Pool>>,
    /// Identities of contracts that aren't in `truth`: fakes.
    fakes: BTreeMap<Address, (Address, Address, u32)>,
    calls: Rc<Cell<u64>>,
}

impl Node {
    fn answer(&self, call: &EthCall) -> CallResult {
        self.calls.set(self.calls.get() + 1);
        let pools = self.truth.get(&call.block).cloned().unwrap_or_default();
        let results: Vec<Option<Vec<u8>>> = multicall::decode_calls(&call.data)
            .unwrap()
            .into_iter()
            .map(|(target, data)| self.answer_one(&pools, target, &data))
            .collect();
        CallResult::Returned(Bytes::from(multicall::encode_results(&results)))
    }

    fn answer_one(
        &self,
        pools: &BTreeMap<Address, Pool>,
        target: Address,
        data: &[u8],
    ) -> Option<Vec<u8>> {
        if target == BASE.tick_lens {
            let (pool, word) = <(Address, i16)>::abi_decode_params(&data[4..]).ok()?;
            let pool = pools.get(&pool)?;
            let ticks: Vec<_> = pool
                .ticks
                .iter()
                .filter(|(t, _)| (t.div_euclid(pool.tick_spacing) >> 8) as i16 == word)
                .map(|(t, l)| (*t, *l))
                .collect();
            return Some(v3::answers::populated_ticks(&ticks));
        }
        if let Some(&(token0, token1, fee)) = self.fakes.get(&target) {
            let pool = Pool::new(target, token0, token1, fee, 60);
            return identity_answer(&pool, data);
        }
        let pool = pools.get(&target)?;
        if let Some(answer) = identity_answer(pool, data) {
            return Some(answer);
        }
        let selector = &data[..4];
        if selector == &v3::slot0_call()[..4] {
            return Some(v3::answers::slot0(pool.price.unwrap_or(Price {
                sqrt_price_x96: U256::ZERO,
                tick: 0,
            })));
        }
        if selector == &v3::liquidity_call()[..4] {
            return Some(v3::answers::liquidity(pool.liquidity));
        }
        if selector == &v3::tick_bitmap_call(0)[..4] {
            let word = i16::abi_decode(&data[4..]).ok()?;
            let mut bits = U256::ZERO;
            for tick in pool.ticks.keys() {
                let compressed = tick.div_euclid(pool.tick_spacing);
                if (compressed >> 8) as i16 == word {
                    bits |= U256::from(1u64) << (compressed.rem_euclid(256) as usize);
                }
            }
            return Some(v3::answers::tick_bitmap(bits));
        }
        if selector == &v3::ticks_call(0)[..4] {
            let tick = I24::abi_decode(&data[4..]).ok()?.as_i32();
            return Some(v3::answers::tick(
                pool.ticks.get(&tick).copied().unwrap_or_default(),
            ));
        }
        None
    }
}

fn identity_answer(pool: &Pool, data: &[u8]) -> Option<Vec<u8>> {
    let selector = &data[..4];
    if selector == &v3::token0_call()[..4] {
        Some(v3::answers::address(pool.token0))
    } else if selector == &v3::token1_call()[..4] {
        Some(v3::answers::address(pool.token1))
    } else if selector == &v3::fee_call()[..4] {
        Some(v3::answers::fee(pool.fee))
    } else if selector == &v3::tick_spacing_call()[..4] {
        Some(v3::answers::tick_spacing(pool.tick_spacing))
    } else {
        None
    }
}

/// A chain that starts at block 100 with `old_pool` already alive (created long before the
/// engine started), plus the events in `blocks`. Returns the blocks the engine sees and a node
/// that knows the true state after each.
fn world(
    blocks: Vec<Vec<(Address, v3::Event)>>,
    created_in_block: Option<usize>,
) -> (Vec<(Duration, Block)>, Node) {
    let mut pools = BTreeMap::new();
    let mut old = Pool::new(old_pool(), TOKEN_A, TOKEN_B, 3000, 60);
    old.apply(v3::Event::Initialize(price(0)));
    old.apply(v3::Event::Mint {
        lower: -600,
        upper: 600,
        amount: 1_000,
    });
    old.apply(v3::Event::Mint {
        lower: -120_000,
        upper: 60_000,
        amount: 7,
    });
    pools.insert(old_pool(), old);

    let mut node = Node::default();
    node.fakes.insert(FORGED, (TOKEN_A, TOKEN_B, 3000));
    let mut chain = Vec::new();
    for (i, events) in blocks.into_iter().enumerate() {
        let number = 100 + i as u64;
        let mut logs = Vec::new();
        if created_in_block == Some(i) {
            logs.push(pool_created(TOKEN_A, TOKEN_C, 500, 10, new_pool()));
            pools.insert(new_pool(), Pool::new(new_pool(), TOKEN_A, TOKEN_C, 500, 10));
        }
        for (pool, event) in events {
            if let Some(p) = pools.get_mut(&pool) {
                p.apply(event);
            }
            logs.push(event_log(pool, event, logs.len() as u64));
        }
        node.truth.insert(number, pools.clone());
        let block = Block {
            number,
            hash: hash(number),
            parent_hash: hash(number - 1),
            timestamp: 1_767_225_600 + 2 * i as u64,
            logs,
        };
        chain.push((Duration::from_secs(2 * (i as u64 + 1)), block));
    }
    (chain, node)
}

struct Run {
    summary: Summary,
    updates: Vec<PoolUpdate>,
    recording: Vec<det::InputRecord>,
}

fn run(config: EngineConfig, blocks: Vec<(Duration, Block)>, node: Node) -> Run {
    run_with(config, blocks, move |call: EthCall| {
        (Duration::from_millis(50), node.answer(&call))
    })
}

/// Like `run`, with each call's latency and answer decided by `answer`.
fn run_with(
    config: EngineConfig,
    blocks: Vec<(Duration, Block)>,
    answer: impl FnMut(EthCall) -> (Duration, CallResult) + 'static,
) -> Run {
    let sink = InMemorySink::default();
    let outbox = InMemoryOutbox::default();
    let summary = det::run_simulated(async {
        let clock = SimClock::starting_at(START);
        let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock.clone()));
        recorder.record_config(config.encode());
        let rpc = SimRpc::new(answer);
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

fn swap(tick: i32, liquidity: u128) -> v3::Event {
    v3::Event::Swap {
        price: price(tick),
        liquidity,
    }
}

fn scenario() -> Vec<Vec<(Address, v3::Event)>> {
    vec![
        // First sight of the old pool: its state is read at this block.
        vec![(old_pool(), swap(-30, 1_007)), (FORGED, swap(1, 1))],
        // After the read: applied on top of it.
        vec![(
            old_pool(),
            v3::Event::Mint {
                lower: -120,
                upper: 120,
                amount: 50,
            },
        )],
        vec![
            (new_pool(), v3::Event::Initialize(price(5))),
            (
                new_pool(),
                v3::Event::Mint {
                    lower: -10,
                    upper: 20,
                    amount: 9,
                },
            ),
            (
                old_pool(),
                v3::Event::Burn {
                    lower: -600,
                    upper: 600,
                    amount: 400,
                },
            ),
        ],
        vec![(old_pool(), swap(700, 7))],
        vec![],
    ]
}

fn final_state(updates: &[PoolUpdate], pool: Address) -> Option<(Option<Price>, u128)> {
    updates
        .iter()
        .rev()
        .find(|u| u.pool == pool)
        .map(|u| match &u.after {
            PoolState::V3(s) => (s.price, s.liquidity),
            PoolState::V2(_) => unreachable!(),
        })
}

#[test]
fn bootstraps_a_pool_seen_mid_life_and_follows_it() {
    let (blocks, node) = world(scenario(), Some(2));
    let truth = node.truth[&104].clone();
    let run = run(EngineConfig::base(), blocks, node);

    // Discovered with its full tick table as of block 100, then three events on top.
    let old: Vec<&PoolUpdate> = run
        .updates
        .iter()
        .filter(|u| u.pool == old_pool())
        .collect();
    assert_eq!(old.len(), 4);
    assert_eq!(old[0].before, None);
    let PoolState::V3(discovered) = &old[0].after else {
        panic!("v3 pool")
    };
    assert_eq!(discovered.ticks.len(), 4);
    assert_eq!(discovered.liquidity, 1_007);

    let held = final_state(&run.updates, old_pool()).unwrap();
    let chain = &truth[&old_pool()];
    assert_eq!(held, (chain.price, chain.liquidity));
    let held_new = final_state(&run.updates, new_pool()).unwrap();
    assert_eq!(held_new, (Some(price(5)), 9));

    assert_eq!(run.summary.stats.pools_tracked, 2);
    assert_eq!(run.summary.stats.pools_rejected, 1);
}

#[test]
fn the_shadow_check_agrees_with_the_chain_every_block() {
    let (blocks, node) = world(scenario(), Some(2));
    let config = EngineConfig {
        check_every: Some(1),
        ..EngineConfig::base()
    };
    let run = run(config, blocks, node);
    assert!(
        run.summary.stats.checks_passed >= 5,
        "{:?}",
        run.summary.stats
    );
    assert_eq!(run.summary.stats.checks_failed, 0);
}

#[test]
fn the_shadow_check_catches_a_wrong_tick() {
    let (blocks, mut node) = world(scenario(), Some(2));
    // The chain's truth at 104 has an extra position the engine never saw.
    node.truth
        .get_mut(&104)
        .unwrap()
        .get_mut(&old_pool())
        .unwrap()
        .apply(v3::Event::Mint {
            lower: 600,
            upper: 1_200,
            amount: 3,
        });
    let config = EngineConfig {
        check_every: Some(1),
        ..EngineConfig::base()
    };
    let run = run(config, blocks, node);
    assert!(
        run.summary.stats.checks_failed >= 1,
        "{:?}",
        run.summary.stats
    );
}

#[test]
fn a_recorded_run_replays_to_the_same_updates() {
    let (blocks, node) = world(scenario(), Some(2));
    let config = EngineConfig {
        check_every: Some(1),
        ..EngineConfig::base()
    };
    let live = run(config, blocks, node);

    let replay = Replay::new(live.recording);
    let config = EngineConfig::decode(&replay.config().unwrap()).unwrap();
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
fn a_one_tick_spacing_pool_reads_its_words_in_batches() {
    let spaced = BASE.pool_address(TOKEN_B, TOKEN_C, 100);
    let (mut blocks, mut node) = world(vec![vec![], vec![]], None);
    let mut pool = Pool::new(spaced, TOKEN_B, TOKEN_C, 100, 1);
    pool.apply(v3::Event::Initialize(price(0)));
    for truth in node.truth.values_mut() {
        truth.insert(spaced, pool.clone());
    }
    blocks[0].1.logs.push(event_log(spaced, swap(0, 0), 0));
    let calls = node.calls.clone();
    let run = run(EngineConfig::base(), blocks, node);
    // One identity call, then 6,932 words at 500 per call; no ticks to read.
    assert_eq!(calls.get(), 1 + 14);
    assert_eq!(run.summary.stats.pools_tracked, 1);
}

// A read that fails part-way is abandoned, and the pool is read again on its next event. The
// abandoned read's other answers are still in flight; one that lands during the new read must
// not count towards it.
#[test]
fn a_late_answer_from_an_abandoned_read_is_ignored() {
    let spaced = BASE.pool_address(TOKEN_B, TOKEN_C, 100);
    let mut pool = Pool::new(spaced, TOKEN_B, TOKEN_C, 100, 1);
    pool.apply(v3::Event::Initialize(price(0)));
    // One position in each of the 14 chunks of bitmap words, so a chunk read twice or missed
    // shows in the tick table.
    let first_word = *v3::words(1).start() as i32;
    for chunk in 0..14 {
        let lower = (first_word + 500 * chunk + 1) * 256;
        pool.apply(v3::Event::Mint {
            lower,
            upper: lower + 1,
            amount: 10 + chunk as u128,
        });
    }
    let (mut blocks, mut node) = world(vec![vec![], vec![], vec![], vec![]], None);
    for truth in node.truth.values_mut() {
        truth.insert(spaced, pool.clone());
    }
    let touch = swap(0, pool.liquidity);
    blocks[0].1.logs.push(event_log(spaced, touch, 0));
    blocks[1].1.logs.push(event_log(spaced, touch, 0));

    let chunk_of = move |call: &EthCall| {
        let (_, data) = multicall::decode_calls(&call.data)?.into_iter().next()?;
        (data[..4] == v3::tick_bitmap_call(0)[..4]).then(|| {
            let word = i16::abi_decode(&data[4..]).unwrap() as i32;
            (word - first_word) / 500
        })
    };
    let run = run_with(EngineConfig::base(), blocks, move |call: EthCall| {
        let answer = node.answer(&call);
        match (call.block, chunk_of(&call)) {
            // The first read, at block 100: one chunk is refused, which abandons the read...
            (100, Some(0)) => (
                Duration::from_millis(50),
                CallResult::Failed("Archive requests require a personal token".into()),
            ),
            // ...and another answers after the second read, at block 101, has started.
            (100, Some(1)) => (Duration::from_millis(2_100), answer),
            (101, Some(_)) => (Duration::from_millis(500), answer),
            _ => (Duration::from_millis(50), answer),
        }
    });

    let discovered = run
        .updates
        .iter()
        .find(|u| u.pool == spaced && u.before.is_none())
        .expect("the pool is discovered");
    let PoolState::V3(state) = &discovered.after else {
        panic!("v3 pool")
    };
    let chain: Vec<_> = pool.ticks.iter().map(|(t, l)| (*t, *l)).collect();
    assert_eq!(state.ticks, chain);
    assert_eq!(discovered.block_number, 101);
    assert_eq!(run.summary.stats.bootstrap_failures, 1);
    assert_eq!(run.summary.stats.pools_tracked, 1);
}
