//! Routes from one token to another: every direct pool, and two hops through a hub (WETH or
//! USDC), each priced exactly against the pools' state.

use std::collections::BTreeMap;

use alloy_primitives::{Address, U256};

use crate::{v2, v3};

#[derive(Clone, Debug, PartialEq)]
pub enum PoolKind {
    /// Reserves of token0 and token1.
    V2 {
        reserve0: U256,
        reserve1: U256,
    },
    V3(venues::v3::Pool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pool {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
    pub kind: PoolKind,
}

impl Pool {
    /// The other token, if `token` is one of this pool's.
    fn other(&self, token: Address) -> Option<Address> {
        if token == self.token0 {
            Some(self.token1)
        } else if token == self.token1 {
            Some(self.token0)
        } else {
            None
        }
    }

    /// What swapping `amount_in` of `token_in` pays out. `None` if the pool doesn't trade it,
    /// can't take the whole amount, or pays nothing.
    pub fn amount_out(&self, token_in: Address, amount_in: U256) -> Option<U256> {
        self.other(token_in)?;
        let zero_for_one = token_in == self.token0;
        let out = match &self.kind {
            PoolKind::V2 { reserve0, reserve1 } => {
                let (r_in, r_out) = if zero_for_one {
                    (*reserve0, *reserve1)
                } else {
                    (*reserve1, *reserve0)
                };
                v2::amount_out(amount_in, r_in, r_out)?
            }
            PoolKind::V3(pool) => {
                let swapped = v3::amount_out(pool, zero_for_one, amount_in)?;
                if !swapped.unspent.is_zero() {
                    return None;
                }
                swapped.amount_out
            }
        };
        (!out.is_zero()).then_some(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leg {
    pub pool: Address,
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub amount_out: U256,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub legs: Vec<Leg>,
}

impl Route {
    pub fn amount_out(&self) -> U256 {
        self.legs.last().map_or(U256::ZERO, |l| l.amount_out)
    }
}

/// Every route from `token_in` to `token_out` for `amount_in`: each pool trading the pair, and
/// each two-hop route through a hub in `hubs` that isn't either end. A route whose pools can't
/// take the amount is left out. In pool address order, direct routes first.
pub fn routes(
    pools: &BTreeMap<Address, Pool>,
    token_in: Address,
    token_out: Address,
    amount_in: U256,
    hubs: &[Address],
) -> Vec<Route> {
    let leg = |pool: &Pool, token_in: Address, amount_in: U256| {
        let token_out = pool.other(token_in)?;
        Some(Leg {
            pool: pool.address,
            token_in,
            token_out,
            amount_in,
            amount_out: pool.amount_out(token_in, amount_in)?,
        })
    };
    let trading = |a: Address, b: Address| pools.values().filter(move |p| p.other(a) == Some(b));
    let mut found: Vec<Route> = trading(token_in, token_out)
        .filter_map(|pool| leg(pool, token_in, amount_in))
        .map(|leg| Route { legs: vec![leg] })
        .collect();
    for hub in hubs {
        if *hub == token_in || *hub == token_out {
            continue;
        }
        for first in trading(token_in, *hub) {
            let Some(first) = leg(first, token_in, amount_in) else {
                continue;
            };
            for second in trading(*hub, token_out) {
                if let Some(second) = leg(second, *hub, first.amount_out) {
                    found.push(Route {
                        legs: vec![first.clone(), second],
                    });
                }
            }
        }
    }
    found
}
