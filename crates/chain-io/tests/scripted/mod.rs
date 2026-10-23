//! A scripted JSON-RPC node for chain I/O's fault tests (build rule 8). `HttpChain` talks to it
//! in process, as an alloy transport, so the code under test parses real JSON-RPC answers and
//! errors; and it waits on tokio's clock, so under `det::run_simulated` a 60s bound takes no real
//! time and nothing touches the network (a local HTTP server couldn't run on the simulated
//! clock). Each method follows a `Script`, which a test can change mid-run, and every request is
//! recorded with when it arrived, so a test can check from those times that retries back off and
//! that the caller gave up within its bound.

// Each test file uses part of it.
#![allow(dead_code)]

use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Context, Poll};
use std::time::Duration;

use alloy::consensus;
use alloy::rpc::client::RpcClient;
use alloy::rpc::json_rpc::{
    ErrorPayload, RequestPacket, Response, ResponsePacket, ResponsePayload, SerializedRequest,
};
use alloy::rpc::types::{Block, BlockTransactions, Header};
use alloy::transports::{TransportError, TransportFut};
use chain_io::HttpChain;
use serde_json::value::RawValue;
use tokio::time::Instant;
use types::chain::B256;

/// The JSON-RPC methods chain I/O calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// `eth_blockNumber`
    BlockNumber,
    /// `eth_getBlockByNumber`
    BlockByNumber,
    /// `eth_getLogs`, by block hash or over a range
    Logs,
    /// `eth_call`
    Call,
}

const METHODS: [Method; 4] = [
    Method::BlockNumber,
    Method::BlockByNumber,
    Method::Logs,
    Method::Call,
];

impl Method {
    fn named(name: &str) -> Option<Self> {
        match name {
            "eth_blockNumber" => Some(Self::BlockNumber),
            "eth_getBlockByNumber" => Some(Self::BlockByNumber),
            "eth_getLogs" => Some(Self::Logs),
            "eth_call" => Some(Self::Call),
            _ => None,
        }
    }
}

/// A JSON-RPC error the node answers with.
#[derive(Clone, Copy, Debug)]
pub struct RpcFailure {
    pub code: i64,
    pub message: &'static str,
}

/// A lagging node's answer for a block hash it doesn't have, as `mainnet.base.org` gives it.
pub const BLOCK_NOT_FOUND: RpcFailure = RpcFailure {
    code: -32001,
    message: "block not found",
};

/// The free Base endpoint's rate limit (D82).
pub const RATE_LIMITED: RpcFailure = RpcFailure {
    code: -32016,
    message: "over rate limit",
};

/// PublicNode's answer to every call on 2026-09-30 (D88).
pub const NO_NODES: RpcFailure = RpcFailure {
    code: -32701,
    message: "no available nodes found for platform base-rpc",
};

/// `NO_NODES` as chain I/O reports it, the last error of a read it gave up on.
pub const NO_NODES_ERROR: &str = "rpc: server returned an error response: error code -32701: no available nodes found for platform base-rpc";

/// How the node treats one method's requests.
#[derive(Clone, Copy, Debug)]
pub enum Script {
    /// Answers each request at once, from the node's chain.
    Answer,
    /// Takes each request and never replies, like an endpoint that holds the connection open.
    Hang,
    /// Fails each request with this error.
    Fail(RpcFailure),
    /// Answers each request this long after it arrives.
    AnswerAfter(Duration),
    /// Fails the next `times` requests with `error`, then answers.
    FailThenAnswer { times: u32, error: RpcFailure },
}

/// One request, as the node saw it.
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub method: Method,
    /// How long after the node was made.
    pub at: Duration,
    /// Whether its script answered it, at once or late, rather than failing it or hanging.
    pub answered: bool,
}

/// More requests than any test makes. A caller retrying without backing off makes them in no
/// simulated time, so it fails here rather than spinning forever.
const MAX_REQUESTS: usize = 10_000;

struct State {
    made: Instant,
    head: u64,
    /// By `Method`, in `METHODS` order.
    scripts: [Script; 4],
    requests: Vec<Request>,
}

/// The node. Its chain has a block at every height up to its head, each with no logs; block
/// `n`'s hash is `block_hash(n)` and its timestamp `2n`.
#[derive(Clone)]
pub struct ScriptedNode {
    // An alloy transport must be `Send + Sync + 'static`, so the state is shared rather than
    // borrowed. Only the test's one thread ever locks it.
    state: Arc<Mutex<State>>,
}

pub fn block_hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

impl ScriptedNode {
    /// A node whose chain's head is `head`, answering every method. Make it inside
    /// `det::run_simulated`: it reads the simulated clock.
    pub fn new(head: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                made: Instant::now(),
                head,
                scripts: [Script::Answer; 4],
                requests: Vec::new(),
            })),
        }
    }

    /// `HttpChain` talking to this node.
    pub fn chain(&self) -> HttpChain {
        HttpChain::with_client(RpcClient::new(self.clone(), true))
    }

    /// From now on, `method`'s requests follow `script`.
    pub fn script(&self, method: Method, script: Script) {
        self.state().scripts[method as usize] = script;
    }

    /// From now on, every method's requests follow `script`, as when the endpoint dies.
    pub fn script_all(&self, script: Script) {
        self.state().scripts = [script; 4];
    }

    /// How long since the node was made. Read it inside `det::run_simulated`.
    pub fn now(&self) -> Duration {
        self.state().made.elapsed()
    }

    /// Every request for `method`, in the order they arrived.
    pub fn requests(&self, method: Method) -> Vec<Request> {
        let state = self.state();
        state
            .requests
            .iter()
            .filter(|r| r.method == method)
            .copied()
            .collect()
    }

    /// The gaps between `method`'s requests from its first that wasn't answered to the first
    /// answered after it, or to its last: the caller's retries.
    pub fn retry_gaps(&self, method: Method) -> Vec<Duration> {
        let requests = self.requests(method);
        let Some(first) = requests.iter().position(|r| !r.answered) else {
            return Vec::new();
        };
        let last = requests[first..]
            .iter()
            .position(|r| r.answered)
            .map_or(requests.len() - 1, |answered| first + answered);
        requests[first..=last]
            .windows(2)
            .map(|pair| pair[1].at - pair[0].at)
            .collect()
    }

    /// Asserts the caller retried `method` with backoff: the gaps between its retries start at
    /// `first`, double up to `max` and stay there.
    pub fn assert_backs_off(&self, method: Method, first: Duration, max: Duration) {
        let gaps = self.retry_gaps(method);
        assert!(!gaps.is_empty(), "{method:?} was never retried");
        let mut backoff = first;
        let mut expected = Vec::new();
        for _ in &gaps {
            expected.push(backoff);
            backoff = (backoff * 2).min(max);
        }
        assert_eq!(
            gaps, expected,
            "{method:?} wasn't retried with backoff from {first:?} doubling to {max:?}"
        );
    }

    /// Asserts the caller, having given up at `gave_up` (from `now`), did so between `bound`
    /// and `bound + slack` after `method`'s first request that wasn't answered: no sooner, so it
    /// would have ridden out a shorter outage, and no later, so it wasn't stuck.
    pub fn assert_gave_up_within(
        &self,
        method: Method,
        gave_up: Duration,
        bound: Duration,
        slack: Duration,
    ) {
        let requests = self.requests(method);
        let failed = requests
            .iter()
            .find(|r| !r.answered)
            .unwrap_or_else(|| panic!("every {method:?} request was answered"));
        let waited = gave_up
            .checked_sub(failed.at)
            .expect("the caller gave up after its request");
        assert!(
            (bound..=bound + slack).contains(&waited),
            "gave up {waited:?} after {method:?} first went unanswered, not within {bound:?} to {:?}",
            bound + slack
        );
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // A panicking test poisons the lock; the state is still what it recorded.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records `request` and decides its reply by its method's script.
    fn reply(&self, request: &SerializedRequest) -> (Method, Reply) {
        let mut state = self.state();
        let name = request.method();
        let method = Method::named(name).unwrap_or_else(|| panic!("the node doesn't serve {name}"));
        assert!(
            state.requests.len() < MAX_REQUESTS,
            "over {MAX_REQUESTS} requests: the caller retries without backing off"
        );
        let script = &mut state.scripts[method as usize];
        let reply = match script {
            Script::Answer | Script::FailThenAnswer { times: 0, .. } => {
                Reply::Answer(Duration::ZERO)
            }
            Script::AnswerAfter(delay) => Reply::Answer(*delay),
            Script::Hang => Reply::Never,
            Script::Fail(error) => Reply::Fail(*error),
            Script::FailThenAnswer { times, error } => {
                *times -= 1;
                Reply::Fail(*error)
            }
        };
        let at = state.made.elapsed();
        state.requests.push(Request {
            method,
            at,
            answered: matches!(reply, Reply::Answer(_)),
        });
        (method, reply)
    }

    /// The node's answer to `request`, from its chain.
    fn answer(&self, method: Method, request: &SerializedRequest) -> Box<RawValue> {
        let head = self.state().head;
        let json = match method {
            Method::BlockNumber => format!("\"0x{head:x}\""),
            Method::BlockByNumber => {
                let params = request.params().expect("eth_getBlockByNumber has params");
                let (number, _full): (String, bool) =
                    serde_json::from_str(params.get()).expect("a block number and a flag");
                let number = u64::from_str_radix(number.trim_start_matches("0x"), 16)
                    .expect("a hex block number");
                if number > head {
                    "null".to_string()
                } else {
                    serde_json::to_string(&block(number)).unwrap()
                }
            }
            Method::Logs => "[]".to_string(),
            Method::Call => "\"0x\"".to_string(),
        };
        RawValue::from_string(json).unwrap()
    }
}

/// What the node does with a request.
enum Reply {
    /// Answers after this long.
    Answer(Duration),
    Fail(RpcFailure),
    Never,
}

fn block(number: u64) -> Block {
    Block {
        header: Header {
            hash: block_hash(number),
            inner: consensus::Header {
                number,
                parent_hash: block_hash(number.saturating_sub(1)),
                timestamp: number * 2,
                ..consensus::Header::default()
            },
            total_difficulty: None,
            size: None,
        },
        transactions: BlockTransactions::Hashes(Vec::new()),
        ..Block::default()
    }
}

impl tower_service::Service<RequestPacket> for ScriptedNode {
    type Response = ResponsePacket;
    type Error = TransportError;
    type Future = TransportFut<'static>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), TransportError>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, packet: RequestPacket) -> TransportFut<'static> {
        let RequestPacket::Single(request) = packet else {
            panic!("chain I/O sends no batches");
        };
        let node = self.clone();
        let (method, reply) = node.reply(&request);
        Box::pin(async move {
            let payload = match reply {
                Reply::Answer(after) => {
                    if !after.is_zero() {
                        tokio::time::sleep(after).await;
                    }
                    ResponsePayload::Success(node.answer(method, &request))
                }
                Reply::Fail(error) => ResponsePayload::Failure(ErrorPayload {
                    code: error.code,
                    message: error.message.into(),
                    data: None,
                }),
                Reply::Never => std::future::pending().await,
            };
            Ok(ResponsePacket::Single(Response {
                id: request.id().clone(),
                payload,
            }))
        })
    }
}
