//! The engine's I/O boundary with chains (D10, D16, D77): reads canonical blocks and the
//! logs the engine follows, and hands them to the core in order.

mod calls;
mod follower;
mod http;

use futures::future::LocalBoxFuture;
use types::chain::{B256, Log};

pub use calls::{CallRequest, spawn_call_worker};
pub use follower::{FollowerConfig, Start, follow_head, spawn_head_follower};
pub use http::HttpChain;

/// Base's free public RPC (D17): HTTP only, `eth_getLogs` limited to 2,000 blocks a call.
pub const BASE_PUBLIC_RPC: &str = "https://mainnet.base.org";

/// The block header fields the engine needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
}

#[derive(Debug)]
pub enum ChainError {
    Rpc(String),
    /// The node answered with something the engine can't use, such as a log with no index.
    Malformed(String),
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rpc(e) => write!(f, "rpc: {e}"),
            Self::Malformed(e) => write!(f, "malformed response: {e}"),
        }
    }
}

impl std::error::Error for ChainError {}

/// What the head follower needs from a node. `HttpChain` is the real one; tests use a fake.
pub trait ChainReader {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>>;

    /// `None` when the node doesn't have the block yet.
    fn header(&self, number: u64) -> LocalBoxFuture<'_, Result<Option<Header>, ChainError>>;

    /// The block's logs whose first topic is one of `topics`. By hash, so the logs always
    /// belong to the header just read, even if the chain reorgs in between.
    fn logs<'a>(
        &'a self,
        block_hash: B256,
        topics: &'a [B256],
    ) -> LocalBoxFuture<'a, Result<Vec<Log>, ChainError>>;
}
