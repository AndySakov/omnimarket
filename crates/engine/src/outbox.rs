//! What the engine publishes: pool updates, with the state before and after (D12).

use std::cell::RefCell;
use std::rc::Rc;

use det::kafka::KafkaPublisher;
use prost::Message as _;
use types::LineageId;
use types::chain::{Address, B256};
use venues::v2::Reserves;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolState {
    V2(Reserves),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolUpdate {
    /// From the natural key (chain, pool, block hash, log index), D71.
    pub id: LineageId,
    /// The chain event (chain, block hash, log index) that caused it.
    pub caused_by: LineageId,
    pub chain_id: u64,
    pub pool: Address,
    pub block_number: u64,
    pub block_hash: B256,
    pub log_index: u64,
    /// `None` when the pool was just discovered.
    pub before: Option<PoolState>,
    pub after: PoolState,
}

impl PoolUpdate {
    pub fn new(
        chain_id: u64,
        pool: Address,
        event: (u64, B256, u64),
        before: Option<PoolState>,
        after: PoolState,
    ) -> Self {
        let (block_number, block_hash, log_index) = event;
        let event_key = [
            &chain_id.to_le_bytes()[..],
            block_hash.as_slice(),
            &log_index.to_le_bytes(),
        ]
        .concat();
        let update_key = [
            &chain_id.to_le_bytes()[..],
            pool.as_slice(),
            block_hash.as_slice(),
            &log_index.to_le_bytes(),
        ]
        .concat();
        Self {
            id: LineageId::from_natural_key("pool_update", &update_key),
            caused_by: LineageId::from_natural_key("chain_event", &event_key),
            chain_id,
            pool,
            block_number,
            block_hash,
            log_index,
            before,
            after,
        }
    }

    pub fn to_proto(&self) -> proto::pool::v1::PoolUpdate {
        proto::pool::v1::PoolUpdate {
            lineage: Some(proto::lineage::v1::Lineage {
                id: self.id.as_bytes().to_vec(),
                caused_by: vec![self.caused_by.as_bytes().to_vec()],
            }),
            chain_id: self.chain_id,
            pool: self.pool.to_vec(),
            block_number: self.block_number,
            block_hash: self.block_hash.to_vec(),
            log_index: self.log_index,
            before: self.before.map(state_to_proto),
            after: Some(state_to_proto(self.after)),
        }
    }
}

fn state_to_proto(state: PoolState) -> proto::pool::v1::PoolState {
    use proto::pool::v1::pool_state::State;
    let state = match state {
        PoolState::V2(reserves) => State::V2(proto::pool::v1::V2Reserves {
            reserve0: big_endian(reserves.reserve0),
            reserve1: big_endian(reserves.reserve1),
        }),
    };
    proto::pool::v1::PoolState { state: Some(state) }
}

/// Big-endian without leading zeros; zero is empty.
fn big_endian(value: u128) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    bytes[first..].to_vec()
}

/// Where pool updates go. Publishing can't change the engine's decisions, so it isn't a
/// recorded input.
pub trait Outbox {
    fn publish(&self, update: &PoolUpdate);
}

/// Keeps updates in memory, for tests and replays. Clones share one list.
#[derive(Clone, Default)]
pub struct InMemoryOutbox {
    updates: Rc<RefCell<Vec<PoolUpdate>>>,
}

impl InMemoryOutbox {
    pub fn updates(&self) -> Vec<PoolUpdate> {
        self.updates.borrow().clone()
    }
}

impl Outbox for InMemoryOutbox {
    fn publish(&self, update: &PoolUpdate) {
        self.updates.borrow_mut().push(update.clone());
    }
}

/// Publishes to `pool-updates.<chain>`, keyed by pool.
pub struct KafkaOutbox {
    publisher: KafkaPublisher,
}

impl KafkaOutbox {
    pub fn new(publisher: KafkaPublisher) -> Self {
        Self { publisher }
    }
}

impl Outbox for KafkaOutbox {
    fn publish(&self, update: &PoolUpdate) {
        self.publisher
            .publish(update.pool.as_slice(), &update.to_proto().encode_to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_endian_drops_leading_zeros() {
        assert_eq!(big_endian(0), Vec::<u8>::new());
        assert_eq!(big_endian(0x01ff), vec![0x01, 0xff]);
    }

    #[test]
    fn the_same_event_gives_the_same_ids() {
        let event = (5, B256::repeat_byte(1), 3);
        let after = PoolState::V2(Reserves {
            reserve0: 1,
            reserve1: 2,
        });
        let a = PoolUpdate::new(8453, Address::repeat_byte(9), event, None, after);
        let b = PoolUpdate::new(8453, Address::repeat_byte(9), event, None, after);
        let other_pool = PoolUpdate::new(8453, Address::repeat_byte(8), event, None, after);
        assert_eq!((a.id, a.caused_by), (b.id, b.caused_by));
        assert_ne!(a.id, other_pool.id);
        assert_eq!(a.caused_by, other_pool.caused_by);
    }
}
