use std::thread::JoinHandle;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use types::chain::{CallResult, EthCall};

use crate::follower::retry;
use crate::{ChainError, HttpChain};

pub type CallRequest = (EthCall, oneshot::Sender<CallResult>);

/// Answers the core's `eth_call`s on its own thread (rule 6), several at a time. A transport
/// failure is retried with backoff until it succeeds, so the core only ever sees what the node
/// answered. The worker stops when every sender is dropped.
pub fn spawn_call_worker(
    rpc_url: String,
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
        runtime.block_on(answer_calls(&chain, receiver));
        Ok(())
    });
    (sender, worker)
}

async fn answer_calls(chain: &HttpChain, mut requests: UnboundedReceiver<CallRequest>) {
    let mut in_flight = FuturesUnordered::new();
    let mut open = true;
    while open || !in_flight.is_empty() {
        tokio::select! {
            biased;
            Some(()) = in_flight.next(), if !in_flight.is_empty() => {}
            request = requests.recv(), if open => match request {
                Some((call, reply)) => in_flight.push(async move {
                    let result = retry("eth_call", || Box::pin(chain.call(&call))).await;
                    // The core may have stopped waiting; nothing to do then.
                    let _ = reply.send(result);
                }),
                None => open = false,
            },
        }
    }
}
