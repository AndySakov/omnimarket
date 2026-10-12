use std::thread::JoinHandle;
use std::time::Duration;

use futures::future::LocalBoxFuture;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::time::Instant;
use types::chain::{LogsQuery, LogsResult};

use crate::retry::{ANSWER_LIMIT, retry_for};
use crate::{ChainError, HttpChain};

pub type LogsRequest = (LogsQuery, oneshot::Sender<LogsResult>);

/// The most logs one read may answer with. A bigger answer is refused as if the node had, so
/// the core splits the range: each answer stays a small input record, well inside Kafka's
/// default 1 MB message.
pub const MAX_LOGS: usize = 2_000;

/// What the logs worker needs from a node. `HttpChain` is the real one; tests use a fake.
pub trait LogsEndpoint {
    /// `eth_getLogs` over the query's range. `Ok` is the node's answer, including a refusal
    /// (too large a range or response); `Err` is anything else, to retry.
    fn logs_in_range<'a>(
        &'a self,
        query: &'a LogsQuery,
    ) -> LocalBoxFuture<'a, Result<LogsResult, ChainError>>;
}

/// A logs worker running on its own thread.
pub struct LogsWorker {
    /// Where the core's `ChannelRpc` sends reads. The worker stops once every sender is gone.
    pub requests: UnboundedSender<LogsRequest>,
    pub thread: JoinHandle<()>,
}

/// Answers the core's log reads on their own thread (rule 6), one at a time and at most
/// `reads_per_second` of them a second, against `rpc_url`: the follower's endpoint, whose ranged
/// `eth_getLogs` the call endpoint doesn't serve past ~90 blocks (D82).
pub fn spawn_logs_worker(rpc_url: &str, reads_per_second: u32) -> Result<LogsWorker, ChainError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ChainError::Rpc(format!("building the logs worker runtime: {e}")))?;
    let chain = HttpChain::new(rpc_url)?;
    let (requests, receiver) = unbounded_channel();
    let spacing = Duration::from_secs(1) / reads_per_second.max(1);
    let thread = std::thread::spawn(move || {
        runtime.block_on(answer_log_reads(&chain, receiver, spacing));
    });
    Ok(LogsWorker { requests, thread })
}

/// Answers `requests` in order, starting one at most every `spacing`. A failure that isn't the
/// node's answer is retried with backoff; one still unanswered after `ANSWER_LIMIT` is answered
/// as failed, since a log read is history, not the live path, and the core can do without it.
/// An answer of more than `MAX_LOGS` logs is answered as failed too.
pub async fn answer_log_reads(
    endpoint: &dyn LogsEndpoint,
    mut requests: UnboundedReceiver<LogsRequest>,
    spacing: Duration,
) {
    let mut next_slot = Instant::now();
    while let Some((query, reply)) = requests.recv().await {
        let slot = next_slot.max(Instant::now());
        next_slot = slot + spacing;
        tokio::time::sleep_until(slot).await;
        let answer = match retry_for("eth_getLogs", ANSWER_LIMIT, || {
            endpoint.logs_in_range(&query)
        })
        .await
        {
            Ok(LogsResult::Returned(logs)) if logs.len() > MAX_LOGS => {
                LogsResult::Failed(format!("more than {MAX_LOGS} logs"))
            }
            Ok(answer) => answer,
            Err(error) => {
                tracing::warn!(%error, "a log read went unanswered");
                LogsResult::Failed(error.to_string())
            }
        };
        // The core may have stopped waiting; nothing to do then.
        let _ = reply.send(answer);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use types::chain::{Address, B256, BlockLog, Bytes, Log};

    use super::*;

    /// Answers each read with `logs` logs, after failing the first `failures` attempts.
    struct FakeNode {
        logs: usize,
        failures: Cell<u32>,
        asked_at: RefCell<Vec<Instant>>,
    }

    impl LogsEndpoint for FakeNode {
        fn logs_in_range<'a>(
            &'a self,
            query: &'a LogsQuery,
        ) -> LocalBoxFuture<'a, Result<LogsResult, ChainError>> {
            Box::pin(async move {
                self.asked_at.borrow_mut().push(Instant::now());
                if self.failures.get() > 0 {
                    self.failures.set(self.failures.get() - 1);
                    return Err(ChainError::Rpc("over rate limit".into()));
                }
                let log = BlockLog {
                    block_number: query.from_block,
                    block_hash: B256::ZERO,
                    block_timestamp: 0,
                    log: Log {
                        address: Address::ZERO,
                        topics: vec![],
                        data: Bytes::new(),
                        log_index: 0,
                        transaction_hash: B256::ZERO,
                    },
                };
                Ok(LogsResult::Returned(vec![log; self.logs]))
            })
        }
    }

    fn query(from_block: u64) -> LogsQuery {
        LogsQuery {
            addresses: vec![],
            topics: vec![],
            from_block,
            to_block: from_block + 9,
        }
    }

    fn answers(node: &FakeNode, reads: usize) -> Vec<LogsResult> {
        det::run_simulated(async {
            let (requests, receiver) = unbounded_channel();
            let mut replies = Vec::new();
            for i in 0..reads {
                let (reply, answer) = oneshot::channel();
                requests.send((query(i as u64 * 10), reply)).unwrap();
                replies.push(answer);
            }
            drop(requests);
            answer_log_reads(node, receiver, Duration::from_millis(500)).await;
            let mut out = Vec::new();
            for reply in replies {
                out.push(reply.await.unwrap());
            }
            out
        })
    }

    #[test]
    fn reads_are_spaced_and_retried_until_answered() {
        let node = FakeNode {
            logs: 1,
            failures: Cell::new(1),
            asked_at: RefCell::default(),
        };
        let answers = answers(&node, 2);
        assert!(
            answers
                .iter()
                .all(|a| matches!(a, LogsResult::Returned(l) if l.len() == 1))
        );
        let at = node.asked_at.borrow();
        let gaps: Vec<u128> = at.windows(2).map(|w| (w[1] - w[0]).as_millis()).collect();
        // The first read fails once and is retried after 250ms; the second waits for its slot,
        // 500ms after the first's.
        assert_eq!(gaps, [250, 250]);
    }

    #[test]
    fn an_answer_over_the_limit_is_a_failed_read() {
        let node = FakeNode {
            logs: MAX_LOGS + 1,
            failures: Cell::new(0),
            asked_at: RefCell::default(),
        };
        assert_eq!(
            answers(&node, 1),
            [LogsResult::Failed("more than 2000 logs".into())]
        );
        let node = FakeNode {
            logs: MAX_LOGS,
            ..node
        };
        assert!(matches!(&answers(&node, 1)[0], LogsResult::Returned(l) if l.len() == MAX_LOGS));
    }
}
