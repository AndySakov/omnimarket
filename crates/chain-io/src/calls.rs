use std::thread::JoinHandle;
use std::time::Duration;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::time::Instant;
use types::chain::{CallResult, EthCall};

use crate::follower::retry;
use crate::{ChainError, HttpChain};

pub type CallRequest = (EthCall, oneshot::Sender<CallResult>);

/// Calls in flight at once. Bootstraps queue thousands of calls in bursts; more than a few at a
/// time only buys rate-limit errors from a free endpoint.
const MAX_IN_FLIGHT: usize = 8;

/// Answers the core's `eth_call`s on its own thread (rule 6), several at a time, starting at
/// most `calls_per_second` of them a second. A transport failure (including a rate limit) is
/// retried with backoff until it succeeds, so the core only ever sees what the node answered.
/// The worker stops when every sender is dropped.
pub fn spawn_call_worker(
    rpc_url: String,
    calls_per_second: u32,
) -> (
    UnboundedSender<CallRequest>,
    JoinHandle<Result<(), ChainError>>,
) {
    let (sender, receiver) = unbounded_channel();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ChainError::Rpc(format!("building the call worker runtime: {e}")))?;
        let chain = HttpChain::new(&rpc_url)?;
        let spacing = Duration::from_secs(1) / calls_per_second.max(1);
        runtime.block_on(answer_calls(&chain, receiver, spacing));
        Ok(())
    });
    (sender, worker)
}

async fn answer_calls(
    chain: &HttpChain,
    mut requests: UnboundedReceiver<CallRequest>,
    spacing: Duration,
) {
    let mut in_flight = FuturesUnordered::new();
    let mut open = true;
    // Each call gets a start slot `spacing` after the previous one, so bursts from bootstraps
    // turn into a steady rate the endpoint accepts.
    let mut next_slot = Instant::now();
    while open || !in_flight.is_empty() {
        tokio::select! {
            biased;
            Some(()) = in_flight.next(), if !in_flight.is_empty() => {}
            request = requests.recv(), if open && in_flight.len() < MAX_IN_FLIGHT => match request {
                Some((call, reply)) => {
                    let slot = next_slot.max(Instant::now());
                    next_slot = slot + spacing;
                    in_flight.push(async move {
                        tokio::time::sleep_until(slot).await;
                        let result = retry("eth_call", || Box::pin(chain.call(&call))).await;
                        // The core may have stopped waiting; nothing to do then.
                        let _ = reply.send(result);
                    });
                }
                None => open = false,
            },
        }
    }
}
