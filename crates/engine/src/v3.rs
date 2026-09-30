//! Uniswap v3 pools in the engine: discovery, CREATE2 proof, a batched read of each pool's
//! state at the block it was first seen, the events after it, and the shadow check.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

use alloy_primitives::U256;
use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall};
use venues::multicall;
use venues::v3::{self, Deployment, Pool, Price, TickLiquidity};

use crate::outbox::{PoolState, PoolUpdate, V3State};
use crate::state::{Effects, Pending, Stats};

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
    /// token0, token1, fee, tickSpacing, slot0 and liquidity of each pool, at `block`.
    Identify { block: u64, pools: Vec<Address> },
    /// A pool's bitmap words, at its bootstrap block.
    Words { pool: Address, words: Vec<i16> },
    /// A pool's initialized ticks in some non-empty words, through TickLens.
    Ticks { pool: Address },
    /// Each pool's slot0, liquidity and one initialized tick at `block`, against what the
    /// engine held then.
    Check { block: u64, expected: Vec<Expected> },
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
    block: u64,
    /// The event that made the engine read the pool; the discovery update carries its key.
    first: EventKey,
    stage: Stage,
    /// Events from blocks after `block`, in order, applied once the read completes.
    later: Vec<(EventKey, v3::Event)>,
}

pub(crate) struct V3Pools {
    deployment: Deployment,
    pools: BTreeMap<Address, Pool>,
    bootstrapping: BTreeMap<Address, Bootstrap>,
    /// Addresses that emitted a v3 event but aren't this deployment's pools: forks or fakes.
    rejected: BTreeSet<Address>,
    check_cursor: Option<Address>,
}

impl V3Pools {
    pub fn new(deployment: Deployment) -> Self {
        Self {
            deployment,
            pools: BTreeMap::new(),
            bootstrapping: BTreeMap::new(),
            rejected: BTreeSet::new(),
            check_cursor: None,
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
            if let Some(pool) = self.pools.get_mut(&address) {
                effects.updates.push(apply(chain_id, pool, key, event));
                continue;
            }
            match self.bootstrapping.get_mut(&address) {
                Some(bootstrap) if block.number > bootstrap.block => {
                    bootstrap.later.push((key, event))
                }
                // Same block as the read: the read already includes it.
                Some(_) => {}
                None => {
                    self.bootstrapping.insert(
                        address,
                        Bootstrap {
                            block: block.number,
                            first: key,
                            stage: Stage::Identifying,
                            later: Vec::new(),
                        },
                    );
                    newly_seen.push(address);
                }
            }
        }
        for batch in newly_seen.chunks(IDENTIFY_BATCH) {
            let calls: Vec<(Address, Vec<u8>)> = batch
                .iter()
                .flat_map(|&pool| {
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
            Call::Words { pool, words } => {
                self.on_words(chain_id, pool, words, result, stats, effects)
            }
            Call::Ticks { pool } => self.on_ticks(chain_id, pool, result, stats, effects),
            Call::Check { block, expected } => self.on_checked(block, expected, result, stats),
        }
    }

    fn on_identified(
        &mut self,
        block: u64,
        pools: Vec<Address>,
        result: CallResult,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        let Some(answers) = answers(&result, pools.len() * 6) else {
            for pool in pools {
                self.fail(pool, stats);
            }
            return;
        };
        for (i, address) in pools.into_iter().enumerate() {
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
                        pool: address,
                        words: chunk.to_vec(),
                    }),
                    multicall_at(block, &calls),
                ));
            }
            if let Some(bootstrap) = self.bootstrapping.get_mut(&address) {
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
        address: Address,
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
            self.fail(address, stats);
            return;
        };
        let Some(bootstrap) = self.bootstrapping.get_mut(&address) else {
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
                .map(|&word| {
                    (
                        self.deployment.tick_lens,
                        v3::populated_ticks_call(address, word),
                    )
                })
                .collect();
            stats.bootstrap_calls += 1;
            effects.calls.push((
                Pending::V3(Call::Ticks { pool: address }),
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
        address: Address,
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
            self.fail(address, stats);
            return;
        };
        let Some(bootstrap) = self.bootstrapping.get_mut(&address) else {
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
        self.pools.insert(address, pool);
    }

    /// Gives up on a pool's bootstrap. It starts again the next time it's seen.
    fn fail(&mut self, address: Address, stats: &mut Stats) {
        if self.bootstrapping.remove(&address).is_some() {
            stats.bootstrap_failures += 1;
        }
    }

    /// Asks the chain for the next `sample` initialized pools' slot0, liquidity and one
    /// initialized tick at `block`, the block just applied.
    pub fn check(&mut self, block: u64, sample: usize, effects: &mut Effects) {
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
                let tick = pool
                    .ticks
                    .range(price.tick..)
                    .next()
                    .or_else(|| pool.ticks.iter().next_back())
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
