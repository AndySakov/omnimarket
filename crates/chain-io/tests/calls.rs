//! The call worker against what indexer.md and D86 say it does, on scripted endpoints.

use std::cell::Cell;
use std::time::Duration;

use chain_io::{CallEndpoint, CallRequest, ChainError, answer_calls, check_call_endpoint};
use futures::future::LocalBoxFuture;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use types::chain::{Address, Bytes, CallResult, EthCall};

/// PublicNode's answer to every call on 2026-09-30, as `HttpChain` reports it: not an answer
/// to the call, so a failure to retry.
const NO_NODES: &str = "server returned an error response: error code -32701: no available nodes found for platform base-rpc";

/// How a scripted endpoint treats `eth_call`s.
enum Calls {
    /// Every attempt fails as PublicNode's did.
    Down,
    /// Every attempt is rate limited until this long after the endpoint is made, then answered.
    RateLimitedFor(Duration),
    /// The node answers every call with this.
    Answer(CallResult),
}

struct Endpoint {
    made: Instant,
    /// `eth_blockNumber` fails as PublicNode's calls did.
    number_down: bool,
    calls: Calls,
    attempts: Cell<u32>,
}

impl Endpoint {
    /// Call inside `det::run_simulated`: it reads the simulated clock.
    fn new(calls: Calls) -> Self {
        Self {
            made: Instant::now(),
            number_down: false,
            calls,
            attempts: Cell::new(0),
        }
    }
}

impl CallEndpoint for Endpoint {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
        Box::pin(async move {
            if self.number_down {
                Err(ChainError::Rpc(NO_NODES.into()))
            } else {
                Ok(1_000)
            }
        })
    }

    fn call<'a>(
        &'a self,
        _call: &'a EthCall,
    ) -> LocalBoxFuture<'a, Result<CallResult, ChainError>> {
        Box::pin(async move {
            self.attempts.set(self.attempts.get() + 1);
            match &self.calls {
                Calls::Down => Err(ChainError::Rpc(NO_NODES.into())),
                Calls::RateLimitedFor(period) if self.made.elapsed() < *period => {
                    Err(ChainError::Rpc("over rate limit".into()))
                }
                Calls::RateLimitedFor(_) => Ok(CallResult::Returned(Bytes::new())),
                Calls::Answer(result) => Ok(result.clone()),
            }
        })
    }
}

/// Sends a call at `block` to the worker, as the core's `ChannelRpc` does.
fn send(
    requests: &mpsc::UnboundedSender<CallRequest>,
    block: u64,
) -> oneshot::Receiver<CallResult> {
    let call = EthCall {
        to: Address::ZERO,
        data: Bytes::new(),
        block,
    };
    let (reply, answer) = oneshot::channel();
    requests.send((call, reply)).unwrap();
    answer
}

/// Runs the check against `endpoint` and returns its result and how long it took.
fn check(endpoint: impl FnOnce() -> Endpoint) -> (Result<(), ChainError>, Duration) {
    det::run_simulated(async {
        let endpoint = endpoint();
        let start = Instant::now();
        let result = check_call_endpoint(&endpoint).await;
        (result, start.elapsed())
    })
}

fn unanswered(result: Result<(), ChainError>) -> (&'static str, String) {
    match result {
        Err(ChainError::Unanswered { what, last, .. }) => (what, last),
        other => panic!("expected the endpoint to go unanswered, got {other:?}"),
    }
}

// indexer.md: "A failure that isn't an answer is retried for up to 5s, so a dead endpoint
// stops `engine follow` within about 8s, before anything else starts."
#[test]
fn an_endpoint_failing_every_call_fails_the_check_within_8s() {
    let (result, took) = check(|| Endpoint::new(Calls::Down));
    assert!(
        (Duration::from_secs(5)..=Duration::from_secs(8)).contains(&took),
        "{took:?}"
    );
    assert_eq!(unanswered(result), ("eth_call", format!("rpc: {NO_NODES}")));

    let (result, took) = check(|| Endpoint {
        number_down: true,
        ..Endpoint::new(Calls::Down)
    });
    assert!(took <= Duration::from_secs(8), "{took:?}");
    assert_eq!(
        unanswered(result),
        ("latest block number", format!("rpc: {NO_NODES}"))
    );
}

// indexer.md: "The node answering the check's call with an error fails the check at once."
#[test]
fn an_endpoint_answering_the_check_with_an_error_fails_it_at_once() {
    let (result, took) = check(|| {
        Endpoint::new(Calls::Answer(CallResult::Failed(
            "the method eth_call does not exist".into(),
        )))
    });
    assert_eq!(took, Duration::ZERO);
    let Err(ChainError::Rpc(error)) = result else {
        panic!("expected the check to fail, got {result:?}");
    };
    assert!(
        error.contains("the method eth_call does not exist"),
        "{error}"
    );

    let (result, _) = check(|| Endpoint::new(Calls::Answer(CallResult::Returned(Bytes::new()))));
    assert!(result.is_ok(), "{result:?}");
}

// indexer.md: "During a run, a call still unanswered after 60s makes the worker give up: it
// stops calling and reports the error, and `engine follow` stops the core and exits with it.
// Until the core is gone the worker holds every call it hasn't answered, so the core never
// sees one dropped."
#[test]
fn an_endpoint_failing_every_call_mid_run_stops_the_worker_holding_its_calls() {
    det::run_simulated(async {
        let endpoint = Endpoint::new(Calls::Down);
        let (requests, receiver) = mpsc::unbounded_channel();
        let (report, gave_up) = oneshot::channel();
        let mut answers: Vec<_> = (1..=3).map(|block| send(&requests, block)).collect();
        let start = Instant::now();

        let core = async {
            let error = gave_up.await.expect("the worker reports why it gave up");
            let waited = start.elapsed();
            let attempts = endpoint.attempts.get();
            // A call made after the worker gave up is held too, and nothing is attempted.
            answers.push(send(&requests, 4));
            tokio::time::sleep(Duration::from_secs(60)).await;
            assert_eq!(endpoint.attempts.get(), attempts);
            for answer in &mut answers {
                assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
            }
            // The binary drops the core, and with it the last sender.
            drop(requests);
            (error, waited, answers)
        };
        let worker = answer_calls(&endpoint, receiver, Duration::from_millis(200), report);
        let ((error, waited, mut answers), ()) = futures::join!(core, worker);

        assert!(
            (Duration::from_secs(60)..=Duration::from_secs(68)).contains(&waited),
            "{waited:?}"
        );
        let ChainError::Unanswered { what, last, .. } = error else {
            panic!("expected the endpoint to go unanswered, got {error:?}");
        };
        assert_eq!((what, last), ("eth_call", format!("rpc: {NO_NODES}")));
        // With the core gone, the worker has returned and let the calls go.
        for answer in &mut answers {
            assert_eq!(answer.try_recv(), Err(TryRecvError::Closed));
        }
    });
}

// indexer.md: "The 60s rides out a rate-limit window (Base's is 30s, D82) or a brief outage."
#[test]
fn an_endpoint_that_rate_limits_for_45s_gets_every_call_answered() {
    det::run_simulated(async {
        let endpoint = Endpoint::new(Calls::RateLimitedFor(Duration::from_secs(45)));
        let (requests, receiver) = mpsc::unbounded_channel();
        let (report, gave_up) = oneshot::channel();
        let answers: Vec<_> = (1..=3).map(|block| send(&requests, block)).collect();
        drop(requests);
        answer_calls(&endpoint, receiver, Duration::from_millis(200), report).await;

        assert!(gave_up.await.is_err(), "the worker gave up");
        for answer in answers {
            assert_eq!(answer.await, Ok(CallResult::Returned(Bytes::new())));
        }
    });
}
