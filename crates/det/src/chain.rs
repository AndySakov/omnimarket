//! Recording encoding for chain blocks, the engine's main input.

use prost::Message as _;
use types::chain::{Address, B256, Block, Bytes, Log};

use crate::Recordable;

impl Recordable for Block {
    fn encode(&self) -> Vec<u8> {
        proto::chain::v1::Block {
            number: self.number,
            hash: self.hash.to_vec(),
            parent_hash: self.parent_hash.to_vec(),
            timestamp: self.timestamp,
            logs: self
                .logs
                .iter()
                .map(|log| proto::chain::v1::Log {
                    address: log.address.to_vec(),
                    topics: log.topics.iter().map(|t| t.to_vec()).collect(),
                    data: log.data.to_vec(),
                    log_index: log.log_index,
                    transaction_hash: log.transaction_hash.to_vec(),
                })
                .collect(),
        }
        .encode_to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let block = proto::chain::v1::Block::decode(bytes).ok()?;
        let logs = block
            .logs
            .into_iter()
            .map(|log| {
                Some(Log {
                    address: Address::try_from(log.address.as_slice()).ok()?,
                    topics: log
                        .topics
                        .iter()
                        .map(|t| B256::try_from(t.as_slice()).ok())
                        .collect::<Option<_>>()?,
                    data: Bytes::from(log.data),
                    log_index: log.log_index,
                    transaction_hash: B256::try_from(log.transaction_hash.as_slice()).ok()?,
                })
            })
            .collect::<Option<_>>()?;
        Some(Block {
            number: block.number,
            hash: B256::try_from(block.hash.as_slice()).ok()?,
            parent_hash: B256::try_from(block.parent_hash.as_slice()).ok()?,
            timestamp: block.timestamp,
            logs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_round_trip() {
        let block = Block {
            number: 7,
            hash: B256::repeat_byte(1),
            parent_hash: B256::repeat_byte(2),
            timestamp: 1_767_225_600,
            logs: vec![Log {
                address: Address::repeat_byte(3),
                topics: vec![B256::repeat_byte(4), B256::repeat_byte(5)],
                data: Bytes::from(vec![6, 7]),
                log_index: 12,
                transaction_hash: B256::repeat_byte(8),
            }],
        };
        assert_eq!(Block::decode(&block.encode()), Some(block));
    }

    #[test]
    fn a_short_hash_does_not_decode() {
        let bad = proto::chain::v1::Block {
            hash: vec![1, 2],
            ..Default::default()
        };
        assert_eq!(Block::decode(&bad.encode_to_vec()), None);
    }
}
