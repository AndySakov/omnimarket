use alloy::eips::{BlockId, BlockNumberOrTag};
use alloy::providers::{Provider, ProviderBuilder, RootProvider};
use alloy::rpc::types::{Filter, TransactionRequest};
use futures::future::LocalBoxFuture;
use types::chain::{B256, CallResult, EthCall, Log};

use crate::{ChainError, ChainReader, Header};

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
}

impl HttpChain {
    /// `eth_call` at the call's block. An error the node answers with (a revert, a bad
    /// argument) is the call's result; anything else is a transport failure to retry.
    pub async fn call(&self, call: &EthCall) -> Result<CallResult, ChainError> {
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
                Some(payload) if !is_rate_limit(payload.code, &payload.message) => {
                    Ok(CallResult::Failed(payload.message.to_string()))
                }
                _ => Err(rpc_error(error)),
            },
        }
    }
}

/// A node refusing for load, not answering the call. The free Base endpoint sends HTTP 429 with
/// a JSON-RPC error body (`-32016 over rate limit`), which reads like an answer; retrying it is
/// right, recording it as the call's result is not.
fn is_rate_limit(code: i64, message: &str) -> bool {
    matches!(code, -32016 | -32005 | 429) || message.to_lowercase().contains("rate limit")
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
    fn rate_limits_are_retried_and_reverts_are_answers() {
        assert!(is_rate_limit(-32016, "over rate limit"));
        assert!(is_rate_limit(-32000, "Rate limit exceeded"));
        assert!(!is_rate_limit(3, "execution reverted"));
        assert!(!is_rate_limit(-32000, "header not found"));
    }
}
