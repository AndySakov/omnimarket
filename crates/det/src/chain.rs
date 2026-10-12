//! Recording encoding for chain blocks, the engine's main input.

use prost::Message as _;
use types::chain::{Address, B256, Block, BlockLog, Bytes, CallResult, Log, LogsResult};

use crate::Recordable;

impl Recordable for Block {
    fn encode(&self) -> Vec<u8> {
        proto::chain::v1::Block {
            number: self.number,
            hash: self.hash.to_vec(),
            parent_hash: self.parent_hash.to_vec(),
            timestamp: self.timestamp,
            chain_head: self.chain_head,
            logs: self.logs.iter().map(log_to_proto).collect(),
        }
        .encode_to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let block = proto::chain::v1::Block::decode(bytes).ok()?;
        let logs = block
            .logs
            .into_iter()
            .map(log_from_proto)
            .collect::<Option<_>>()?;
        Some(Block {
            number: block.number,
            hash: B256::try_from(block.hash.as_slice()).ok()?,
            parent_hash: B256::try_from(block.parent_hash.as_slice()).ok()?,
            timestamp: block.timestamp,
            logs,
            chain_head: block.chain_head,
        })
    }
}

fn log_to_proto(log: &Log) -> proto::chain::v1::Log {
    proto::chain::v1::Log {
        address: log.address.to_vec(),
        topics: log.topics.iter().map(|t| t.to_vec()).collect(),
        data: log.data.to_vec(),
        log_index: log.log_index,
        transaction_hash: log.transaction_hash.to_vec(),
    }
}

fn log_from_proto(log: proto::chain::v1::Log) -> Option<Log> {
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
}

impl Recordable for LogsResult {
    fn encode(&self) -> Vec<u8> {
        use proto::chain::v1::logs_result::Outcome;
        let outcome = match self {
            LogsResult::Returned(logs) => Outcome::Returned(proto::chain::v1::BlockLogs {
                logs: logs
                    .iter()
                    .map(|l| proto::chain::v1::BlockLog {
                        block_number: l.block_number,
                        block_hash: l.block_hash.to_vec(),
                        block_timestamp: l.block_timestamp,
                        log: Some(log_to_proto(&l.log)),
                    })
                    .collect(),
            }),
            LogsResult::Failed(error) => Outcome::Failed(error.clone()),
        };
        proto::chain::v1::LogsResult {
            outcome: Some(outcome),
        }
        .encode_to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        use proto::chain::v1::logs_result::Outcome;
        match proto::chain::v1::LogsResult::decode(bytes).ok()?.outcome? {
            Outcome::Returned(logs) => logs
                .logs
                .into_iter()
                .map(|l| {
                    Some(BlockLog {
                        block_number: l.block_number,
                        block_hash: B256::try_from(l.block_hash.as_slice()).ok()?,
                        block_timestamp: l.block_timestamp,
                        log: log_from_proto(l.log?)?,
                    })
                })
                .collect::<Option<_>>()
                .map(LogsResult::Returned),
            Outcome::Failed(error) => Some(LogsResult::Failed(error)),
        }
    }
}

impl Recordable for CallResult {
    fn encode(&self) -> Vec<u8> {
        use proto::chain::v1::call_result::Outcome;
        let outcome = match self {
            CallResult::Returned(data) => Outcome::Returned(data.to_vec()),
            CallResult::Failed(error) => Outcome::Failed(error.clone()),
        };
        proto::chain::v1::CallResult {
            outcome: Some(outcome),
        }
        .encode_to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        use proto::chain::v1::call_result::Outcome;
        match proto::chain::v1::CallResult::decode(bytes).ok()?.outcome? {
            Outcome::Returned(data) => Some(CallResult::Returned(Bytes::from(data))),
            Outcome::Failed(error) => Some(CallResult::Failed(error)),
        }
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
            chain_head: Some(9),
        };
        assert_eq!(Block::decode(&block.encode()), Some(block.clone()));
        let unknown_head = Block {
            chain_head: None,
            ..block
        };
        assert_eq!(Block::decode(&unknown_head.encode()), Some(unknown_head));
    }

    #[test]
    fn log_reads_round_trip() {
        let log = Log {
            address: Address::repeat_byte(3),
            topics: vec![B256::repeat_byte(4)],
            data: Bytes::from(vec![6, 7]),
            log_index: 12,
            transaction_hash: B256::repeat_byte(8),
        };
        for result in [
            LogsResult::Returned(vec![BlockLog {
                block_number: 7,
                block_hash: B256::repeat_byte(1),
                block_timestamp: 1_767_225_600,
                log,
            }]),
            LogsResult::Returned(vec![]),
            LogsResult::Failed("backend response too large".into()),
        ] {
            assert_eq!(LogsResult::decode(&result.encode()), Some(result));
        }
    }

    #[test]
    fn call_results_round_trip() {
        for result in [
            CallResult::Returned(Bytes::from(vec![1, 2, 3])),
            CallResult::Failed("execution reverted".into()),
        ] {
            assert_eq!(CallResult::decode(&result.encode()), Some(result));
        }
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
