use alloy::eips::{BlockId, BlockNumberOrTag};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::rpc::client::RpcClient;
use alloy::rpc::types::{Filter, TransactionRequest};
use futures::future::LocalBoxFuture;
use types::chain::{B256, BlockLog, CallResult, EthCall, Log, LogsQuery, LogsResult};

use crate::{CallEndpoint, ChainError, ChainReader, Header, LogsEndpoint};

/// A node over JSON-RPC on HTTP.
pub struct HttpChain {
    provider: RootProvider,
}

impl HttpChain {
    pub fn new(url: &str) -> Result<Self, ChainError> {
        let url = url
            .parse()
            .map_err(|e| ChainError::Rpc(format!("bad RPC URL {url}: {e}")))?;
        Ok(Self {
            provider: ProviderBuilder::default().connect_http(url),
        })
    }

    /// A node behind any JSON-RPC client rather than an HTTP URL: the fault tests' scripted node.
    pub fn with_client(client: RpcClient) -> Self {
        Self {
            provider: ProviderBuilder::default().connect_client(client),
        }
    }
}

impl CallEndpoint for HttpChain {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
        ChainReader::latest_number(self)
    }

    /// An error the node answers with (a revert, a bad argument) is the call's result;
    /// anything else is a failure to retry.
    fn call<'a>(&'a self, call: &'a EthCall) -> LocalBoxFuture<'a, Result<CallResult, ChainError>> {
        Box::pin(async move {
            let request = TransactionRequest::default()
                .to(call.to)
                .input(call.data.clone().into());
            match self
                .provider
                .call(request)
                .block(BlockId::number(call.block))
                .await
            {
                Ok(returned) => Ok(CallResult::Returned(returned)),
                Err(error) => match error.as_error_resp() {
                    Some(payload) if !is_retryable(payload.code, &payload.message) => {
                        Ok(CallResult::Failed(payload.message.to_string()))
                    }
                    _ => Err(rpc_error(error)),
                },
            }
        })
    }
}

impl LogsEndpoint for HttpChain {
    /// A refusal the node answers with (`-32020 backend response too large`, a range over its
    /// limit) is the read's result; anything else is a failure to retry. A log without its
    /// block's number, hash or timestamp fails the whole read.
    fn logs_in_range<'a>(
        &'a self,
        query: &'a LogsQuery,
    ) -> LocalBoxFuture<'a, Result<LogsResult, ChainError>> {
        Box::pin(async move {
            let filter = Filter::new()
                .from_block(query.from_block)
                .to_block(query.to_block)
                .address(query.addresses.clone())
                .event_signature(query.topics.clone());
            let logs = match self.provider.get_logs(&filter).await {
                Ok(logs) => logs,
                Err(error) => {
                    return match error.as_error_resp() {
                        Some(payload) if !is_retryable(payload.code, &payload.message) => {
                            Ok(LogsResult::Failed(payload.message.to_string()))
                        }
                        _ => Err(rpc_error(error)),
                    };
                }
            };
            let read = logs
                .into_iter()
                .map(|log| {
                    Some(BlockLog {
                        block_number: log.block_number?,
                        block_hash: log.block_hash?,
                        block_timestamp: log.block_timestamp?,
                        log: Log {
                            address: log.address(),
                            topics: log.topics().to_vec(),
                            data: log.data().data.clone(),
                            log_index: log.log_index?,
                            transaction_hash: log.transaction_hash?,
                        },
                    })
                })
                .collect::<Option<Vec<_>>>();
            Ok(read.map(LogsResult::Returned).unwrap_or_else(|| {
                LogsResult::Failed(
                    "a log without its block number, hash, timestamp, index or transaction".into(),
                )
            }))
        })
    }
}

/// An error that isn't the node's answer to the call, so it's retried below the core instead of
/// recorded as the call's result:
/// - a rate limit: the free Base endpoint sends HTTP 429 with a JSON-RPC error body
///   (`-32016 over rate limit`), which reads like an answer;
/// - a block the node doesn't have yet: a load-balanced node that lags answers a call at a
///   recent block with `-32001 block not found` or `header not found`;
/// - no node at all: PublicNode answered every call with `-32701 no available nodes found for
///   platform base-rpc` on 2026-09-30 (D88).
///
/// Everything else (a revert, a bad argument, state older than the node keeps) is the answer.
fn is_retryable(code: i64, message: &str) -> bool {
    let message = message.to_lowercase();
    matches!(code, -32016 | -32005 | -32701 | 429)
        || message.contains("rate limit")
        || message.contains("block not found")
        || message.contains("header not found")
        || message.contains("no available nodes")
}

fn rpc_error(e: impl std::fmt::Display) -> ChainError {
    ChainError::Rpc(e.to_string())
}

impl ChainReader for HttpChain {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
        Box::pin(async move { self.provider.get_block_number().await.map_err(rpc_error) })
    }

    fn header(&self, number: u64) -> LocalBoxFuture<'_, Result<Option<Header>, ChainError>> {
        Box::pin(async move {
            let block = self
                .provider
                .get_block_by_number(BlockNumberOrTag::Number(number))
                .await
                .map_err(rpc_error)?;
            Ok(block.map(|b| Header {
                number: b.header.number,
                hash: b.header.hash,
                parent_hash: b.header.parent_hash,
                timestamp: b.header.timestamp,
            }))
        })
    }

    fn logs<'a>(
        &'a self,
        block_hash: B256,
        topics: &'a [B256],
    ) -> LocalBoxFuture<'a, Result<Vec<Log>, ChainError>> {
        Box::pin(async move {
            let filter = Filter::new()
                .at_block_hash(block_hash)
                .event_signature(topics.to_vec());
            let logs = self.provider.get_logs(&filter).await.map_err(rpc_error)?;
            logs.into_iter()
                .map(|log| {
                    let missing = |field| ChainError::Malformed(format!("log without {field}"));
                    Ok(Log {
                        address: log.address(),
                        topics: log.topics().to_vec(),
                        data: log.data().data.clone(),
                        log_index: log.log_index.ok_or_else(|| missing("log index"))?,
                        transaction_hash: log
                            .transaction_hash
                            .ok_or_else(|| missing("transaction hash"))?,
                    })
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_lag_and_outages_are_retried_and_node_answers_are_results() {
        assert!(is_retryable(-32016, "over rate limit"));
        assert!(is_retryable(-32000, "Rate limit exceeded"));
        assert!(is_retryable(-32001, "block not found"));
        assert!(is_retryable(-32000, "header not found"));
        assert!(is_retryable(
            -32701,
            "no available nodes found for platform base-rpc"
        ));
        assert!(!is_retryable(3, "execution reverted"));
        assert!(!is_retryable(-32602, "invalid argument 0"));
        assert!(!is_retryable(
            -32602,
            "Archive requests require a personal token. Get one at: https://www.allnodes.com/publicnode"
        ));
    }
}
