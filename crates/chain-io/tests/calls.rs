//! The call worker against what indexer.md and D88 say it does, on a scripted node (build
//! rule 8).

mod scripted;

use std::time::Duration;

use chain_io::{CallRequest, ChainError, answer_calls, check_call_endpoint};
use scripted::{Method, NO_NODES, NO_NODES_ERROR, RATE_LIMITED, RpcFailure, Script, ScriptedNode};
use tokio::sync::oneshot::error::TryRecvError;
use tokio::sync::{mpsc, oneshot};
use types::chain::{Address, Bytes, CallResult, EthCall};

/// The node's head, so the check's call is at this block.
const HEAD: u64 = 1_000;

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

/// Runs the check against `node`, and returns its result. Call inside `det::run_simulated`, as
/// the node is made.
async fn check(node: &ScriptedNode) -> Result<(), ChainError> {
    tokio::time::timeout(Duration::from_secs(300), check_call_endpoint(&node.chain()))
        .await
        .expect("the check never gave up on the endpoint")
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
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script(Method::Call, Script::Fail(NO_NODES));
        let result = check(&node).await;
        node.assert_gave_up_within(
            Method::Call,
            node.now(),
            Duration::from_secs(5),
            Duration::from_secs(3),
        );
        let (what, last) = unanswered(result);
        assert_eq!((what, last.as_str()), ("eth_call", NO_NODES_ERROR));
    });

    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script_all(Script::Fail(NO_NODES));
        let result = check(&node).await;
        node.assert_gave_up_within(
            Method::BlockNumber,
            node.now(),
            Duration::from_secs(5),
            Duration::from_secs(3),
        );
        let (what, last) = unanswered(result);
        assert_eq!(
            (what, last.as_str()),
            ("latest block number", NO_NODES_ERROR)
        );
        assert!(node.requests(Method::Call).is_empty());
    });
}

// indexer.md: "Each attempt is bounded too, so an endpoint that takes the connection and never
// replies counts as unanswered."
#[test]
fn an_endpoint_that_never_replies_fails_the_check_within_8s() {
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script(Method::Call, Script::Hang);
        let result = check(&node).await;
        node.assert_gave_up_within(
            Method::Call,
            node.now(),
            Duration::from_secs(5),
            Duration::from_secs(3),
        );
        let (what, last) = unanswered(result);
        assert_eq!((what, last.as_str()), ("eth_call", "no reply within 5s"));
    });
}

// indexer.md: "The node answering the check's call with an error fails the check at once."
#[test]
fn an_endpoint_answering_the_check_with_an_error_fails_it_at_once() {
    let no_eth_call = RpcFailure {
        code: -32601,
        message: "the method eth_call does not exist/is not available",
    };
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script(Method::Call, Script::Fail(no_eth_call));
        let result = check(&node).await;
        assert_eq!(node.now(), Duration::ZERO);
        assert_eq!(node.requests(Method::Call).len(), 1);
        let Err(ChainError::Rpc(error)) = result else {
            panic!("expected the check to fail, got {result:?}");
        };
        assert_eq!(
            error,
            "a plain eth_call at block 1000 failed: the method eth_call does not exist/is not available"
        );
    });

    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        check(&node).await.unwrap();
    });
}

// indexer.md: "During a run, a call still unanswered after 60s makes the worker give up: it
// stops calling and reports the error, and `engine follow` stops the core and exits with it.
// Until the core is gone the worker holds every call it hasn't answered, so the core never
// sees one dropped."
#[test]
fn an_endpoint_failing_every_call_mid_run_stops_the_worker_holding_its_calls() {
    let (what, last) = stop_the_worker_holding_its_calls(Script::Fail(NO_NODES));
    assert_eq!((what, last.as_str()), ("eth_call", NO_NODES_ERROR));
}

// indexer.md: "Each attempt is bounded too, so an endpoint that takes the connection and never
// replies counts as unanswered."
#[test]
fn an_endpoint_that_never_replies_mid_run_stops_the_worker_holding_its_calls() {
    let (what, last) = stop_the_worker_holding_its_calls(Script::Hang);
    assert_eq!((what, last.as_str()), ("eth_call", "no reply within 60s"));
}

/// Runs the worker against a node whose calls follow `dead` until it gives up, checks it gave up
/// within 60 to 68s and held every call until the core was gone, and returns what went
/// unanswered and its last error.
fn stop_the_worker_holding_its_calls(dead: Script) -> (&'static str, String) {
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script(Method::Call, dead);
        let chain = node.chain();
        let (requests, receiver) = mpsc::unbounded_channel();
        let (report, gave_up) = oneshot::channel();
        let mut answers: Vec<_> = (1..=3).map(|block| send(&requests, block)).collect();

        let core = async {
            let error = tokio::time::timeout(Duration::from_secs(300), gave_up)
                .await
                .expect("the worker never gave up on the endpoint")
                .expect("the worker reports why it gave up");
            let gave_up_at = node.now();
            let attempts = node.requests(Method::Call).len();
            // A call made after the worker gave up is held too, and nothing is attempted.
            answers.push(send(&requests, 4));
            tokio::time::sleep(Duration::from_secs(60)).await;
            assert_eq!(node.requests(Method::Call).len(), attempts);
            for answer in &mut answers {
                assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
            }
            // The binary drops the core, and with it the last sender.
            drop(requests);
            (error, gave_up_at, answers)
        };
        let worker = answer_calls(&chain, receiver, Duration::from_millis(200), report);
        let ((error, gave_up_at, mut answers), ()) = futures::join!(core, worker);

        node.assert_gave_up_within(
            Method::Call,
            gave_up_at,
            Duration::from_secs(60),
            Duration::from_secs(8),
        );
        // With the core gone, the worker has returned and let the calls go.
        for answer in &mut answers {
            assert_eq!(answer.try_recv(), Err(TryRecvError::Closed));
        }
        match error {
            ChainError::Unanswered { what, last, .. } => (what, last),
            other => panic!("expected the endpoint to go unanswered, got {other:?}"),
        }
    })
}

// indexer.md: "The 60s rides out a rate-limit window (Base's is 30s, D82) or a brief outage."
#[test]
fn an_endpoint_that_rate_limits_for_45s_gets_every_call_answered() {
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script(Method::Call, Script::Fail(RATE_LIMITED));
        let chain = node.chain();
        let (requests, receiver) = mpsc::unbounded_channel();
        let (report, gave_up) = oneshot::channel();
        let answers: Vec<_> = (1..=3).map(|block| send(&requests, block)).collect();
        drop(requests);
        let recovers = async {
            tokio::time::sleep(Duration::from_secs(45)).await;
            node.script(Method::Call, Script::Answer);
        };
        futures::join!(
            answer_calls(&chain, receiver, Duration::from_millis(200), report),
            recovers
        );

        assert!(gave_up.await.is_err(), "the worker gave up");
        for answer in answers {
            assert_eq!(answer.await, Ok(CallResult::Returned(Bytes::new())));
        }
        // Each call is answered within one backoff, at most 8s, of the endpoint recovering.
        let answered = node.now();
        assert!(
            (Duration::from_secs(45)..=Duration::from_secs(53)).contains(&answered),
            "{answered:?}"
        );
    });
}

// indexer.md: the call worker retries what isn't the node's answer "with backoff and below the
// core".
#[test]
fn a_rate_limited_call_is_retried_with_backoff_until_answered() {
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        let rate_limited = Script::FailThenAnswer {
            times: 6,
            error: RATE_LIMITED,
        };
        node.script(Method::Call, rate_limited);
        let (requests, receiver) = mpsc::unbounded_channel();
        let (report, gave_up) = oneshot::channel();
        let answer = send(&requests, 1);
        drop(requests);
        answer_calls(&node.chain(), receiver, Duration::from_millis(200), report).await;

        assert_eq!(answer.await, Ok(CallResult::Returned(Bytes::new())));
        assert!(gave_up.await.is_err(), "the worker gave up");
        let gaps: Vec<u128> = node
            .retry_gaps(Method::Call)
            .iter()
            .map(Duration::as_millis)
            .collect();
        assert_eq!(gaps, [250, 500, 1_000, 2_000, 4_000, 8_000]);
    });
}

// indexer.md: "An attempt gets the time left until the limit, at least 1s": a slow endpoint
// isn't asked again while its answer is on the way.
#[test]
fn a_slow_endpoint_gets_each_call_answered_without_a_retry() {
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script(Method::Call, Script::AnswerAfter(Duration::from_secs(30)));
        let (requests, receiver) = mpsc::unbounded_channel();
        let (report, gave_up) = oneshot::channel();
        let answers: Vec<_> = (1..=3).map(|block| send(&requests, block)).collect();
        drop(requests);
        answer_calls(&node.chain(), receiver, Duration::from_millis(200), report).await;

        assert!(gave_up.await.is_err(), "the worker gave up");
        for answer in answers {
            assert_eq!(answer.await, Ok(CallResult::Returned(Bytes::new())));
        }
        let at: Vec<u128> = node
            .requests(Method::Call)
            .iter()
            .map(|r| r.at.as_millis())
            .collect();
        assert_eq!(at, [0, 200, 400]);
        assert_eq!(node.now(), Duration::from_millis(30_400));
    });
}

// indexer.md, "Calls with injected code": a revert the node answers with is the call's result;
// anything else is a failure.
#[test]
fn a_call_with_injected_code_returns_a_revert_and_fails_on_anything_else() {
    let call = EthCall {
        to: Address::repeat_byte(5),
        data: Bytes::new(),
        block: HEAD,
    };
    let with_code = |node: &ScriptedNode| {
        let chain = node.chain();
        let call = call.clone();
        async move {
            chain
                .call_with_code(
                    &call,
                    Address::repeat_byte(1),
                    Address::repeat_byte(5),
                    Bytes::from(vec![0u8]),
                    alloy::primitives::U256::from(1u64),
                )
                .await
        }
    };
    det::run_simulated(async {
        let node = ScriptedNode::new(HEAD);
        node.script(
            Method::Call,
            Script::Fail(RpcFailure {
                code: 3,
                message: "execution reverted: sells are disabled",
            }),
        );
        assert_eq!(
            with_code(&node).await.unwrap(),
            CallResult::Failed("execution reverted: sells are disabled".into())
        );
        node.script(Method::Call, Script::Fail(RATE_LIMITED));
        assert!(matches!(with_code(&node).await, Err(ChainError::Rpc(_))));
        node.script(Method::Call, Script::Answer);
        assert!(matches!(
            with_code(&node).await,
            Ok(CallResult::Returned(_))
        ));
    });
}
