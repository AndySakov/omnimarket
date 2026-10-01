use std::collections::BTreeMap;
use std::thread::JoinHandle;
use std::time::Duration;

use futures::future::LocalBoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::time::Instant;
use types::chain::{Address, Bytes, CallResult, EthCall};

use crate::retry::{ANSWER_LIMIT, CHECK_LIMIT, retry_for};
use crate::{ChainError, HttpChain};

pub type CallRequest = (EthCall, oneshot::Sender<CallResult>);

/// Calls in flight at once. Bootstraps queue thousands of calls in bursts; more than a few at a
/// time only buys rate-limit errors from a free endpoint.
const MAX_IN_FLIGHT: usize = 8;

/// What the call worker needs from a node. `HttpChain` is the real one; tests use a fake.
pub trait CallEndpoint {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>>;

    /// `eth_call` at the call's block. `Ok` is the node's answer, including an error it
    /// answered with (a revert, a bad argument); `Err` is anything else, to retry.
    fn call<'a>(&'a self, call: &'a EthCall) -> LocalBoxFuture<'a, Result<CallResult, ChainError>>;
}

/// A call worker running on its own thread.
pub struct CallWorker {
    /// Where the core's `ChannelRpc` sends calls. The worker stops once every sender is gone.
    pub requests: UnboundedSender<CallRequest>,
    /// Resolves if the worker gives up on its endpoint mid-run. It then holds every call it
    /// hasn't answered until `requests` closes, so the caller stops the core on this.
    pub gave_up: oneshot::Receiver<ChainError>,
    pub thread: JoinHandle<()>,
}

/// Checks that `rpc_url` answers an `eth_call` (`check_call_endpoint`), then answers the core's
/// calls on their own thread (rule 6) with `answer_calls`, starting at most `calls_per_second`
/// of them a second. Fails without starting the thread if the check fails.
pub fn spawn_call_worker(rpc_url: &str, calls_per_second: u32) -> Result<CallWorker, ChainError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ChainError::Rpc(format!("building the call worker runtime: {e}")))?;
    let chain = HttpChain::new(rpc_url)?;
    runtime.block_on(check_call_endpoint(&chain))?;
    let (requests, receiver) = unbounded_channel();
    let (report, gave_up) = oneshot::channel();
    let spacing = Duration::from_secs(1) / calls_per_second.max(1);
    // The runtime and the endpoint move to the worker's thread; the check ran on this one.
    let thread = std::thread::spawn(move || {
        runtime.block_on(answer_calls(&chain, receiver, spacing, report));
    });
    Ok(CallWorker {
        requests,
        gave_up,
        thread,
    })
}

/// The check before a run (D88): the endpoint's latest block number, then a plain `eth_call`
/// at that block (to the zero address, no data), which any node answers with empty bytes. A
/// failure that isn't an answer is retried for up to `CHECK_LIMIT`. The node answering the
/// call with an error fails the check at once.
pub async fn check_call_endpoint(endpoint: &dyn CallEndpoint) -> Result<(), ChainError> {
    let block = retry_for("latest block number", CHECK_LIMIT, || {
        endpoint.latest_number()
    })
    .await?;
    let call = EthCall {
        to: Address::ZERO,
        data: Bytes::new(),
        block,
    };
    match retry_for("eth_call", CHECK_LIMIT, || endpoint.call(&call)).await? {
        CallResult::Returned(_) => Ok(()),
        CallResult::Failed(error) => Err(ChainError::Rpc(format!(
            "a plain eth_call at block {block} failed: {error}"
        ))),
    }
}

/// Answers `requests` several at a time, starting one every `spacing`, so bursts from
/// bootstraps turn into a steady rate the endpoint accepts. A failure that isn't the node's
/// answer (a transport error, a rate limit, a node that is down) is retried with backoff, so
/// the core only ever sees what the node answered.
///
/// A call still unanswered after `ANSWER_LIMIT` means the endpoint is unusable (D88): the
/// worker stops calling, sends the error on `report`, and holds every call it hasn't answered
/// until `requests` closes. Returns once `requests` has closed and nothing is in flight.
pub async fn answer_calls(
    endpoint: &dyn CallEndpoint,
    mut requests: UnboundedReceiver<CallRequest>,
    spacing: Duration,
    report: oneshot::Sender<ChainError>,
) {
    // Replies live here, not in the call futures, so giving up can drop the futures without
    // dropping a reply: `ChannelRpc` takes a dropped reply for a dead worker and panics.
    let mut replies = BTreeMap::new();
    let mut next_id: u64 = 0;
    let mut in_flight = FuturesUnordered::new();
    let mut open = true;
    let mut next_slot = Instant::now();
    let error = loop {
        if !open && in_flight.is_empty() {
            return;
        }
        tokio::select! {
            biased;
            Some((id, result)) = in_flight.next(), if !in_flight.is_empty() => match result {
                Ok(answer) => {
                    let reply: oneshot::Sender<CallResult> =
                        replies.remove(&id).expect("each call in flight has its reply");
                    // The core may have stopped waiting; nothing to do then.
                    let _ = reply.send(answer);
                }
                Err(error) => break error,
            },
            request = requests.recv(), if open && in_flight.len() < MAX_IN_FLIGHT => match request {
                Some((call, reply)) => {
                    let id = next_id;
                    next_id += 1;
                    replies.insert(id, reply);
                    let slot = next_slot.max(Instant::now());
                    next_slot = slot + spacing;
                    in_flight.push(async move {
                        tokio::time::sleep_until(slot).await;
                        let result = retry_for("eth_call", ANSWER_LIMIT, || endpoint.call(&call));
                        (id, result.await)
                    });
                }
                None => open = false,
            },
        }
    };
    drop(in_flight);
    tracing::error!(%error, "giving up on the call endpoint");
    // Report first: the caller stops the core on it, which closes `requests`. Until then keep
    // every reply open, including those of calls still arriving, so the core never sees one
    // dropped. Everything held drops when this returns.
    let _ = report.send(error);
    let mut held = Vec::new();
    while let Some(request) = requests.recv().await {
        held.push(request);
    }
}
