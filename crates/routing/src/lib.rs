//! Swap quotes from in-memory pool state (#83, D21): exact Uniswap v2 and v3 math over the
//! state `venues` keeps, and a search over direct pools and two hops through a hub token.

pub mod route;
pub mod v2;
pub mod v3;

pub use route::{Leg, Pool, PoolKind, Route, routes};
pub use v3::Swapped;
