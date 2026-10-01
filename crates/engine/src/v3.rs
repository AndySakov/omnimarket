//! Uniswap v3 pools in the engine: discovery, CREATE2 proof, a batched read of each pool's
//! state at the block it was first seen, the events after it, trades from `Swap`, and the
//! shadow check.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

use alloy_primitives::U256;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall};
use venues::multicall;
use venues::v3::{self, Deployment, Pool, Price, TickLiquidity};

use crate::outbox::{PoolState, PoolUpdate, V3State};
use crate::state::{Effects, Pending, Stats};
use crate::trades::{SwapSeen, Trade, Venue};

/// Pools per identity call: six view calls each.
const IDENTIFY_BATCH: usize = 50;
/// Bitmap words per call. A 1-tick-spacing pool has 6,932 words: 14 calls.
const WORDS_PER_CALL: usize = 500;
/// Non-empty words per TickLens call; each returns up to 256 ticks.
const TICK_WORDS_PER_CALL: usize = 50;

/// (block number, block hash, log index): the chain event a pool update comes from.
type EventKey = (u64, B256, u64);

/// A v3 call the engine is waiting on.
pub(crate) enum Call {
    /// token0, token1, fee, tickSpacing, slot0 and liquidity of each pool, at `block`. Each
    /// pool comes with its bootstrap attempt.
    Identify { block: u64, pools: Vec<Attempt> },
    /// A pool's bitmap words, at its bootstrap block.
    Words { attempt: Attempt, words: Vec<i16> },
    /// A pool's initialized ticks in some non-empty words, through TickLens.
    Ticks { attempt: Attempt },
    /// Each pool's slot0, liquidity and one of its initialized ticks at `block`, against what
    /// the engine held then.
    Check { block: u64, expected: Vec<Expected> },
}

/// One bootstrap read of one pool. Its number tells this read's answers from those of an
/// earlier, abandoned read of the same pool, which can still be in flight and arrive in any
/// order.
#[derive(Clone, Copy)]
pub(crate) struct Attempt {
    pool: Address,
    number: u64,
}

pub(crate) struct Expected {
    pool: Address,
    price: Price,
    liquidity: u128,
    tick: Option<(i32, TickLiquidity)>,
}

enum Stage {
    Identifying,
    /// Reading bitmap words; collecting the non-empty ones.
    Words {
        pool: Pool,
        calls_left: usize,
        nonzero: Vec<i16>,
    },
    /// Reading the initialized ticks in the non-empty words.
    Ticks {
        pool: Pool,
        calls_left: usize,
    },
}

/// A pool first seen mid-life (D80): its state is read at `block`, the block it was first
/// seen in, which already includes every event of that block.
struct Bootstrap {
    attempt: u64,
    block: u64,
    /// The event that made the engine read the pool; the discovery update carries its key.
    first: EventKey,
    stage: Stage,
    /// Events from blocks after `block`, in order, applied once the read completes.
    later: Vec<(EventKey, v3::Event)>,
    /// Every `Swap` since the pool was first seen, `block`'s included: the read already holds
    /// their state, but each is still a trade. Published once the read completes.
    trades: Vec<SwapSeen>,
}

pub(crate) struct V3Pools {
    deployment: Deployment,
    /// Trade records' quote assets, most preferred first (D97).
    quote_assets: Vec<Address>,
    pools: BTreeMap<Address, Pool>,
    bootstrapping: BTreeMap<Address, Bootstrap>,
    /// Addresses that emitted a v3 event but aren't this deployment's pools: forks or fakes.
    rejected: BTreeSet<Address>,
    check_cursor: Option<Address>,
    next_attempt: u64,
    /// Counts shadow checks, so each check reads a different tick of a pool than the last.
    checks: usize,
}

impl V3Pools {
    pub fn new(deployment: Deployment, quote_assets: Vec<Address>) -> Self {
        Self {
            deployment,
            quote_assets,
            pools: BTreeMap::new(),
            bootstrapping: BTreeMap::new(),
            rejected: BTreeSet::new(),
            check_cursor: None,
            next_attempt: 0,
            checks: 0,
        }
    }

    pub fn tracked(&self) -> usize {
        self.pools.len()
    }

    pub fn apply_block(
        &mut self,
        chain_id: u64,
        block: &Block,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        let mut newly_seen = Vec::new();
        for log in &block.logs {
            let key = (block.number, block.hash, log.log_index);
            if log.address == self.deployment.factory && v3::is_pool_created(log) {
                self.on_created(log);
                continue;
            }
            let Some(event) = v3::decode(log) else {
                continue;
            };
            let address = log.address;
            if self.rejected.contains(&address) {
                continue;
            }
            let swap = v3::decode_swap(log).map(|s| {
                SwapSeen::new(block, log, (s.sender, s.recipient), (s.amount0, s.amount1))
            });
            if let Some(pool) = self.pools.get_mut(&address) {
                effects.updates.push(apply(chain_id, pool, key, event));
                if let Some(swap) = swap {
                    effects
                        .trades
                        .push(trade(chain_id, pool, &self.quote_assets, &swap));
                }
                continue;
            }
            match self.bootstrapping.get_mut(&address) {
                Some(bootstrap) => {
                    // Same block as the read: the read already includes its state.
                    if block.number > bootstrap.block {
                        bootstrap.later.push((key, event));
                    }
                    bootstrap.trades.extend(swap);
                }
                None => {
                    let attempt = self.next_attempt;
                    self.next_attempt += 1;
                    self.bootstrapping.insert(
                        address,
                        Bootstrap {
                            attempt,
                            block: block.number,
                            first: key,
                            stage: Stage::Identifying,
                            later: Vec::new(),
                            trades: swap.into_iter().collect(),
                        },
                    );
                    newly_seen.push(Attempt {
                        pool: address,
                        number: attempt,
                    });
                }
            }
        }
        for batch in newly_seen.chunks(IDENTIFY_BATCH) {
            let calls: Vec<(Address, Vec<u8>)> = batch
                .iter()
                .flat_map(|&Attempt { pool, .. }| {
                    [
                        (pool, v3::token0_call()),
                        (pool, v3::token1_call()),
                        (pool, v3::fee_call()),
                        (pool, v3::tick_spacing_call()),
                        (pool, v3::slot0_call()),
                        (pool, v3::liquidity_call()),
                    ]
                })
                .collect();
            stats.bootstrap_calls += 1;
            effects.calls.push((
                Pending::V3(Call::Identify {
                    block: block.number,
                    pools: batch.to_vec(),
                }),
                multicall_at(block.number, &calls),
            ));
        }
    }

    /// The factory's own event: known at once, uninitialized and empty.
    fn on_created(&mut self, log: &types::chain::Log) {
        let Some(c) = v3::decode_pool_created(log) else {
            return;
        };
        if self.deployment.pool_address(c.token0, c.token1, c.fee) != c.pool {
            return;
        }
        self.pools
            .entry(c.pool)
            .or_insert_with(|| Pool::new(c.pool, c.token0, c.token1, c.fee, c.tick_spacing));
    }

    pub fn on_answer(
        &mut self,
        chain_id: u64,
        call: Call,
        result: CallResult,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        match call {
            Call::Identify { block, pools } => {
                self.on_identified(block, pools, result, stats, effects)
            }
            Call::Words { attempt, words } => {
                self.on_words(chain_id, attempt, words, result, stats, effects)
            }
            Call::Ticks { attempt } => self.on_ticks(chain_id, attempt, result, stats, effects),
            Call::Check { block, expected } => self.on_checked(block, expected, result, stats),
        }
    }

    fn on_identified(
        &mut self,
        block: u64,
        pools: Vec<Attempt>,
        result: CallResult,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        let Some(answers) = answers(&result, pools.len() * 6) else {
            for attempt in pools {
                self.fail(attempt, stats);
            }
            return;
        };
        for (i, attempt) in pools.into_iter().enumerate() {
            let address = attempt.pool;
            if self.current(attempt).is_none() {
                continue;
            }
            let a = &answers[6 * i..6 * i + 6];
            let identity = (|| {
                Some((
                    v3::decode_address(a[0].as_ref()?)?,
                    v3::decode_address(a[1].as_ref()?)?,
                    v3::decode_fee(a[2].as_ref()?)?,
                    v3::decode_tick_spacing(a[3].as_ref()?)?,
                    v3::decode_slot0(a[4].as_ref()?)?,
                    v3::decode_liquidity(a[5].as_ref()?)?,
                ))
            })();
            let genuine = identity.filter(|&(token0, token1, fee, spacing, _, _)| {
                spacing > 0 && self.deployment.pool_address(token0, token1, fee) == address
            });
            let Some((token0, token1, fee, spacing, price, liquidity)) = genuine else {
                self.bootstrapping.remove(&address);
                self.rejected.insert(address);
                stats.pools_rejected += 1;
                continue;
            };
            let mut pool = Pool::new(address, token0, token1, fee, spacing);
            pool.price = (price.sqrt_price_x96 != U256::ZERO).then_some(price);
            pool.liquidity = liquidity;

            let words: Vec<i16> = v3::words(spacing).collect();
            let chunks: Vec<&[i16]> = words.chunks(WORDS_PER_CALL).collect();
            for chunk in &chunks {
                let calls: Vec<(Address, Vec<u8>)> = chunk
                    .iter()
                    .map(|&word| (address, v3::tick_bitmap_call(word)))
                    .collect();
                stats.bootstrap_calls += 1;
                effects.calls.push((
                    Pending::V3(Call::Words {
                        attempt,
                        words: chunk.to_vec(),
                    }),
                    multicall_at(block, &calls),
                ));
            }
            if let Some(bootstrap) = self.current(attempt) {
                bootstrap.stage = Stage::Words {
                    pool,
                    calls_left: chunks.len(),
                    nonzero: Vec::new(),
                };
            }
        }
    }

    fn on_words(
        &mut self,
        chain_id: u64,
        attempt: Attempt,
        words: Vec<i16>,
        result: CallResult,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        let bitmaps: Option<Vec<U256>> = answers(&result, words.len()).and_then(|a| {
            a.iter()
                .map(|r| r.as_ref().and_then(|r| v3::decode_tick_bitmap(r)))
                .collect()
        });
        let Some(bitmaps) = bitmaps else {
            self.fail(attempt, stats);
            return;
        };
        let address = attempt.pool;
        let tick_lens = self.deployment.tick_lens;
        let Some(bootstrap) = self.current(attempt) else {
            return;
        };
        let Stage::Words {
            calls_left,
            nonzero,
            ..
        } = &mut bootstrap.stage
        else {
            return;
        };
        for (word, bits) in words.into_iter().zip(bitmaps) {
            if bits != U256::ZERO {
                nonzero.push(word);
            }
        }
        *calls_left -= 1;
        if *calls_left > 0 {
            return;
        }
        let block = bootstrap.block;
        let Stage::Words {
            pool, mut nonzero, ..
        } = std::mem::replace(&mut bootstrap.stage, Stage::Identifying)
        else {
            return;
        };
        if nonzero.is_empty() {
            bootstrap.stage = Stage::Ticks {
                pool,
                calls_left: 0,
            };
            self.finish(chain_id, address, effects);
            return;
        }
        // Answers can arrive in any order; read the words in order.
        nonzero.sort_unstable();
        let chunks: Vec<&[i16]> = nonzero.chunks(TICK_WORDS_PER_CALL).collect();
        for chunk in &chunks {
            let calls: Vec<(Address, Vec<u8>)> = chunk
                .iter()
                .map(|&word| (tick_lens, v3::populated_ticks_call(address, word)))
                .collect();
            stats.bootstrap_calls += 1;
            effects.calls.push((
                Pending::V3(Call::Ticks { attempt }),
                multicall_at(block, &calls),
            ));
        }
        bootstrap.stage = Stage::Ticks {
            pool,
            calls_left: chunks.len(),
        };
    }

    fn on_ticks(
        &mut self,
        chain_id: u64,
        attempt: Attempt,
        result: CallResult,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        let populated: Option<Vec<Vec<(i32, TickLiquidity)>>> = decode_all(&result).and_then(|a| {
            a.iter()
                .map(|r| r.as_ref().and_then(|r| v3::decode_populated_ticks(r)))
                .collect()
        });
        let Some(populated) = populated else {
            self.fail(attempt, stats);
            return;
        };
        let address = attempt.pool;
        let Some(bootstrap) = self.current(attempt) else {
            return;
        };
        let Stage::Ticks { pool, calls_left } = &mut bootstrap.stage else {
            return;
        };
        for (tick, liquidity) in populated.into_iter().flatten() {
            pool.ticks.insert(tick, liquidity);
        }
        *calls_left -= 1;
        if *calls_left == 0 {
            self.finish(chain_id, address, effects);
        }
    }

    /// The read is complete: publish the pool as discovered, then apply what happened since.
    fn finish(&mut self, chain_id: u64, address: Address, effects: &mut Effects) {
        let Some(bootstrap) = self.bootstrapping.remove(&address) else {
            return;
        };
        let Stage::Ticks { mut pool, .. } = bootstrap.stage else {
            return;
        };
        let discovered = V3State {
            price: pool.price,
            liquidity: pool.liquidity,
            ticks: pool.ticks.iter().map(|(t, l)| (*t, *l)).collect(),
        };
        effects.updates.push(PoolUpdate::new(
            chain_id,
            address,
            bootstrap.first,
            None,
            PoolState::V3(discovered),
        ));
        for (key, event) in bootstrap.later {
            effects.updates.push(apply(chain_id, &mut pool, key, event));
        }
        for swap in &bootstrap.trades {
            effects
                .trades
                .push(trade(chain_id, &pool, &self.quote_assets, swap));
        }
        self.pools.insert(address, pool);
    }

    /// The pool's bootstrap, if `attempt` is the one in progress. Answers to an abandoned
    /// attempt are ignored.
    fn current(&mut self, attempt: Attempt) -> Option<&mut Bootstrap> {
        self.bootstrapping
            .get_mut(&attempt.pool)
            .filter(|b| b.attempt == attempt.number)
    }

    /// Gives up on attempt `attempt` of a pool's bootstrap. It starts again the next time the
    /// pool is seen, and the swaps buffered so far are never published. A failure from an
    /// earlier attempt leaves the current one alone.
    fn fail(&mut self, attempt: Attempt, stats: &mut Stats) {
        if self.current(attempt).is_some()
            && let Some(bootstrap) = self.bootstrapping.remove(&attempt.pool)
        {
            stats.bootstrap_failures += 1;
            stats.trades_dropped += bootstrap.trades.len() as u64;
        }
    }

    /// Asks the chain for the next `sample` initialized pools' slot0, liquidity and one
    /// initialized tick at `block`, the block just applied. The tick rotates through the
    /// pool's table from one check to the next, so a missing or wrong tick is found in time.
    pub fn check(&mut self, block: u64, sample: usize, effects: &mut Effects) {
        let round = self.checks;
        self.checks += 1;
        let after_cursor: Box<dyn Iterator<Item = (&Address, &Pool)>> = match self.check_cursor {
            Some(cursor) => Box::new(self.pools.range((Excluded(cursor), Unbounded))),
            None => Box::new(self.pools.iter()),
        };
        // Wrapping round to the start; looking at each pool at most once.
        let expected: Vec<Expected> = after_cursor
            .chain(self.pools.iter())
            .take(self.pools.len())
            .filter_map(|(address, pool)| {
                let price = pool.price?;
                let tick = (!pool.ticks.is_empty())
                    .then(|| pool.ticks.iter().nth(round % pool.ticks.len()))
                    .flatten()
                    .map(|(t, l)| (*t, *l));
                Some(Expected {
                    pool: *address,
                    price,
                    liquidity: pool.liquidity,
                    tick,
                })
            })
            .take(sample)
            .collect();
        let Some(last) = expected.last() else {
            return;
        };
        self.check_cursor = Some(last.pool);
        let calls: Vec<(Address, Vec<u8>)> = expected
            .iter()
            .flat_map(|e| {
                let mut calls = vec![(e.pool, v3::slot0_call()), (e.pool, v3::liquidity_call())];
                if let Some((tick, _)) = e.tick {
                    calls.push((e.pool, v3::ticks_call(tick)));
                }
                calls
            })
            .collect();
        effects.calls.push((
            Pending::V3(Call::Check { block, expected }),
            multicall_at(block, &calls),
        ));
    }

    fn on_checked(
        &mut self,
        block: u64,
        expected: Vec<Expected>,
        result: CallResult,
        stats: &mut Stats,
    ) {
        let calls = expected
            .iter()
            .map(|e| 2 + usize::from(e.tick.is_some()))
            .sum();
        let Some(answers) = answers(&result, calls) else {
            tracing::warn!(block, "v3 shadow check call failed");
            return;
        };
        let mut next = answers.iter();
        for e in expected {
            let mut read = || next.next().and_then(|a| a.as_ref());
            let price = read().and_then(|a| v3::decode_slot0(a));
            let liquidity = read().and_then(|a| v3::decode_liquidity(a));
            let tick = e
                .tick
                .map(|(t, _)| (t, read().and_then(|a| v3::decode_tick(a))));
            let matches = price == Some(e.price)
                && liquidity == Some(e.liquidity)
                && tick.is_none_or(|(t, chain)| chain.is_some() && e.tick == chain.map(|l| (t, l)));
            if matches {
                stats.checks_passed += 1;
            } else {
                stats.checks_failed += 1;
                tracing::error!(
                    block,
                    pool = %e.pool,
                    held = ?(e.price, e.liquidity, e.tick),
                    chain = ?(price, liquidity, tick),
                    "v3 state differs from the chain"
                );
            }
        }
    }
}

fn apply(chain_id: u64, pool: &mut Pool, key: EventKey, event: v3::Event) -> PoolUpdate {
    let (before, after) = pool.apply(event);
    let state = |s: v3::Snapshot| {
        PoolState::V3(V3State {
            price: s.price,
            liquidity: s.liquidity,
            ticks: s.ticks,
        })
    };
    PoolUpdate::new(
        chain_id,
        pool.address,
        key,
        Some(state(before)),
        state(after),
    )
}

fn trade(chain_id: u64, pool: &Pool, quote_assets: &[Address], swap: &SwapSeen) -> Trade {
    Trade::new(
        chain_id,
        Venue::UniswapV3,
        pool.address,
        (pool.token0, pool.token1),
        quote_assets,
        swap,
    )
}

fn multicall_at(block: u64, calls: &[(Address, Vec<u8>)]) -> EthCall {
    EthCall {
        to: multicall::ADDRESS,
        data: Bytes::from(multicall::encode(calls)),
        block,
    }
}

fn decode_all(result: &CallResult) -> Option<Vec<Option<Bytes>>> {
    match result {
        CallResult::Returned(data) => multicall::decode(data),
        CallResult::Failed(_) => None,
    }
}

/// The multicall's per-call answers, if it returned exactly `count` of them.
fn answers(result: &CallResult, count: usize) -> Option<Vec<Option<Bytes>>> {
    decode_all(result).filter(|a| a.len() == count)
}
