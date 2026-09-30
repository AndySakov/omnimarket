//! Uniswap v2 pairs in the engine: discovery, CREATE2 proof, state from `Sync`, and the
//! shadow state check.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

use types::chain::{Address, B256, Block, Bytes, CallResult, EthCall, Log};
use venues::multicall;
use venues::v2::{self, Deployment, Pair, Reserves};

use crate::outbox::{PoolState, PoolUpdate};
use crate::state::{Effects, Pending, Stats};

/// Pairs per verification call: two view calls each, well inside a node's call limits.
const VERIFY_BATCH: usize = 100;

/// A v2 call the engine is waiting on.
pub(crate) enum Call {
    /// token0() and token1() of each pair, to prove it by its CREATE2 address.
    Verify { pairs: Vec<Address> },
    /// getReserves() of each pair at `block`, against the reserves the engine held then.
    Check {
        block: u64,
        expected: Vec<(Address, Reserves)>,
    },
}

/// A `Sync` seen for a pair still waiting for its proof.
struct Seen {
    event: (u64, B256, u64),
    reserves: Reserves,
}

pub(crate) struct V2Pools {
    deployment: Deployment,
    pairs: BTreeMap<Address, Pair>,
    /// Pairs seen trading before we knew them (D80), with their events in order, until the
    /// CREATE2 proof comes back.
    unproven: BTreeMap<Address, Vec<Seen>>,
    /// Addresses that emitted a v2 event but aren't this deployment's pairs: forks or fakes.
    rejected: BTreeSet<Address>,
    /// Where the last shadow check's sample ended, so checks walk every pair in turn.
    check_cursor: Option<Address>,
}

impl V2Pools {
    pub fn new(deployment: Deployment) -> Self {
        Self {
            deployment,
            pairs: BTreeMap::new(),
            unproven: BTreeMap::new(),
            rejected: BTreeSet::new(),
            check_cursor: None,
        }
    }

    pub fn tracked(&self) -> usize {
        self.pairs.len()
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
            let event = (block.number, block.hash, log.log_index);
            if log.address == self.deployment.factory && v2::is_pair_created(log) {
                self.on_created(log);
            } else if v2::is_sync(log) {
                self.on_sync(chain_id, log, event, &mut newly_seen, effects);
            }
        }
        for batch in newly_seen.chunks(VERIFY_BATCH) {
            let calls: Vec<(Address, Vec<u8>)> = batch
                .iter()
                .flat_map(|&pair| [(pair, v2::token0_call()), (pair, v2::token1_call())])
                .collect();
            stats.verify_calls += 1;
            effects.calls.push((
                Pending::V2(Call::Verify {
                    pairs: batch.to_vec(),
                }),
                EthCall {
                    to: multicall::ADDRESS,
                    data: Bytes::from(multicall::encode(&calls)),
                    block: block.number,
                },
            ));
        }
    }

    /// The factory's own event: known at once, empty until its first `Sync`.
    fn on_created(&mut self, log: &Log) {
        let Some(created) = v2::decode_pair_created(log) else {
            return;
        };
        if self.deployment.pair_address(created.token0, created.token1) != created.pair {
            return;
        }
        self.pairs.entry(created.pair).or_insert(Pair {
            address: created.pair,
            token0: created.token0,
            token1: created.token1,
            reserves: Reserves {
                reserve0: 0,
                reserve1: 0,
            },
        });
    }

    fn on_sync(
        &mut self,
        chain_id: u64,
        log: &Log,
        event: (u64, B256, u64),
        newly_seen: &mut Vec<Address>,
        effects: &mut Effects,
    ) {
        let address = log.address;
        if self.rejected.contains(&address) {
            return;
        }
        let Some(reserves) = v2::decode_sync(log) else {
            return;
        };
        if let Some(pair) = self.pairs.get_mut(&address) {
            let before = PoolState::V2(pair.reserves);
            pair.reserves = reserves;
            effects.updates.push(PoolUpdate::new(
                chain_id,
                address,
                event,
                Some(before),
                PoolState::V2(reserves),
            ));
            return;
        }
        let seen = self.unproven.entry(address).or_default();
        if seen.is_empty() {
            newly_seen.push(address);
        }
        seen.push(Seen { event, reserves });
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
            Call::Verify { pairs } => self.on_verified(chain_id, pairs, result, stats, effects),
            Call::Check { block, expected } => self.on_checked(block, expected, result, stats),
        }
    }

    fn on_verified(
        &mut self,
        chain_id: u64,
        pairs: Vec<Address>,
        result: CallResult,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        let answers = match &result {
            CallResult::Returned(data) => multicall::decode(data),
            CallResult::Failed(_) => None,
        };
        let Some(answers) = answers.filter(|a| a.len() == pairs.len() * 2) else {
            // Forget them; each is proven again the next time it trades.
            stats.verify_failures += 1;
            for pair in &pairs {
                self.unproven.remove(pair);
            }
            return;
        };
        for (i, address) in pairs.into_iter().enumerate() {
            let seen = self.unproven.remove(&address).unwrap_or_default();
            let token =
                |answer: &Option<Bytes>| answer.as_ref().and_then(|a| v2::decode_address(a));
            let tokens = token(&answers[2 * i]).zip(token(&answers[2 * i + 1]));
            let genuine = tokens.filter(|&(token0, token1)| {
                self.deployment.pair_address(token0, token1) == address
            });
            let Some((token0, token1)) = genuine else {
                self.rejected.insert(address);
                stats.pairs_rejected += 1;
                continue;
            };
            // Each `Sync` carries the full reserves, so the buffered ones replay in order.
            let mut before: Option<Reserves> = None;
            for Seen { event, reserves } in seen {
                effects.updates.push(PoolUpdate::new(
                    chain_id,
                    address,
                    event,
                    before.map(PoolState::V2),
                    PoolState::V2(reserves),
                ));
                before = Some(reserves);
            }
            if let Some(reserves) = before {
                self.pairs.insert(
                    address,
                    Pair {
                        address,
                        token0,
                        token1,
                        reserves,
                    },
                );
            }
        }
    }

    /// Asks the chain for the reserves of the next `sample` pairs at `block`, the block just
    /// applied, to compare with what the engine holds now.
    pub fn check(&mut self, block: u64, sample: usize, effects: &mut Effects) {
        let after_cursor: Box<dyn Iterator<Item = (&Address, &Pair)>> = match self.check_cursor {
            Some(cursor) => Box::new(self.pairs.range((Excluded(cursor), Unbounded))),
            None => Box::new(self.pairs.iter()),
        };
        // Wrapping round to the start; taking at most every pair once.
        let expected: Vec<(Address, Reserves)> = after_cursor
            .chain(self.pairs.iter())
            .map(|(address, pair)| (*address, pair.reserves))
            .take(sample.min(self.pairs.len()))
            .collect();
        let Some(&(last, _)) = expected.last() else {
            return;
        };
        self.check_cursor = Some(last);
        let calls: Vec<(Address, Vec<u8>)> = expected
            .iter()
            .map(|(address, _)| (*address, v2::get_reserves_call()))
            .collect();
        effects.calls.push((
            Pending::V2(Call::Check { block, expected }),
            EthCall {
                to: multicall::ADDRESS,
                data: Bytes::from(multicall::encode(&calls)),
                block,
            },
        ));
    }

    fn on_checked(
        &mut self,
        block: u64,
        expected: Vec<(Address, Reserves)>,
        result: CallResult,
        stats: &mut Stats,
    ) {
        let answers = match &result {
            CallResult::Returned(data) => multicall::decode(data),
            CallResult::Failed(_) => None,
        };
        let Some(answers) = answers.filter(|a| a.len() == expected.len()) else {
            tracing::warn!(block, "shadow check call failed");
            return;
        };
        for ((address, held), answer) in expected.into_iter().zip(answers) {
            let chain = answer.as_ref().and_then(|a| v2::decode_reserves(a));
            if chain == Some(held) {
                stats.checks_passed += 1;
            } else {
                stats.checks_failed += 1;
                tracing::error!(block, %address, ?held, ?chain, "v2 reserves differ from the chain");
            }
        }
    }
}
