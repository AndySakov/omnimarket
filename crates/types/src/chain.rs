//! What the engine receives from a chain: canonical blocks with the logs it follows.

pub use alloy_primitives::{Address, B256, Bytes};

/// A canonical block and the logs in it that match the engine's topics, in log order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    /// Seconds since the Unix epoch, as the block header states it.
    pub timestamp: u64,
    pub logs: Vec<Log>,
    /// The chain's latest block number when the follower read this one. `None` in recordings
    /// made before the follower recorded it.
    pub chain_head: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log {
    pub address: Address,
    /// topics[0] is the event signature.
    pub topics: Vec<B256>,
    pub data: Bytes,
    /// Position among all logs in the block, not just the ones we follow. Part of the
    /// natural key (chain, block hash, log index), D71.
    pub log_index: u64,
    pub transaction_hash: B256,
}

/// A read-only call at a block: `eth_call`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EthCall {
    pub to: Address,
    pub data: Bytes,
    pub block: u64,
}

/// What an `eth_call` gives the core. Transport failures are retried below the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallResult {
    Returned(Bytes),
    /// The node answered with an error, such as a revert.
    Failed(String),
}

/// A ranged log read, `eth_getLogs` over `from_block..=to_block`: the logs of `addresses`
/// whose first topic is one of `topics`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogsQuery {
    pub addresses: Vec<Address>,
    pub topics: Vec<B256>,
    pub from_block: u64,
    pub to_block: u64,
}

/// What a log read gives the core. Transport failures and rate limits are retried below it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogsResult {
    /// In block and log order.
    Returned(Vec<BlockLog>),
    /// The node refused the read, e.g. as too large, or the answer was more logs than a read
    /// may return.
    Failed(String),
}

/// A log from a ranged read, with the block it's in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockLog {
    pub block_number: u64,
    pub block_hash: B256,
    /// Seconds since the Unix epoch.
    pub block_timestamp: u64,
    pub log: Log,
}
