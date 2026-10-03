//! `HttpChain` and the call worker against a local JSON-RPC server: which of the node's errors
//! are the call's result and which are failures to retry (indexer.md, D88), and the call rate.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use chain_io::{CallEndpoint, ChainError, HttpChain, spawn_call_worker};
use types::chain::{Address, Bytes, CallResult, EthCall};

/// Serves every request on its own connection with `status` and a JSON-RPC body whose
/// `result` or `error` member is `member`, echoing the request's id. Returns the server's URL.
fn node(status: &'static str, member: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(&stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut request = vec![0; length];
            reader.read_exact(&mut request).unwrap();
            let request = String::from_utf8(request).unwrap();
            let id: String = request[request.find("\"id\":").unwrap() + 5..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            let body = format!(r#"{{"jsonrpc":"2.0","id":{id},{member}}}"#);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    url
}

fn eth_call() -> EthCall {
    EthCall {
        to: Address::ZERO,
        data: Bytes::new(),
        block: 1_000,
    }
}

fn call(url: &str) -> Result<CallResult, ChainError> {
    let call = eth_call();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { HttpChain::new(url).unwrap().call(&call).await })
}

#[test]
fn a_returned_value_is_the_result() {
    let url = node("200 OK", r#""result":"0x0102""#);
    assert_eq!(
        call(&url).unwrap(),
        CallResult::Returned(Bytes::from(vec![1, 2]))
    );
}

// indexer.md: "Everything else (a revert, a bad argument, state older than the node keeps) is
// the answer."
#[test]
fn a_revert_is_the_result() {
    let url = node(
        "200 OK",
        r#""error":{"code":3,"message":"execution reverted"}"#,
    );
    assert_eq!(
        call(&url).unwrap(),
        CallResult::Failed("execution reverted".into())
    );
}

// The free Base endpoint rate limits with HTTP 429 and a JSON-RPC error body, which reads like
// the node's answer. It isn't one: it's retried below the core.
#[test]
fn a_rate_limit_is_a_failure_to_retry() {
    let url = node(
        "429 Too Many Requests",
        r#""error":{"code":-32016,"message":"over rate limit"}"#,
    );
    match call(&url) {
        Err(ChainError::Rpc(error)) => assert!(error.contains("over rate limit"), "{error}"),
        other => panic!("expected a failure to retry, got {other:?}"),
    }
}

// `engine follow --calls-per-second 5`: six calls sent at once start 200ms apart, so the last
// starts 1s after the first.
#[test]
fn the_call_worker_starts_at_most_calls_per_second() {
    let url = node("200 OK", r#""result":"0x0102""#);
    let worker = spawn_call_worker(&url, 5).unwrap();
    let start = Instant::now();
    let answers: Vec<_> = (0..6)
        .map(|_| {
            let (reply, answer) = tokio::sync::oneshot::channel();
            worker.requests.send((eth_call(), reply)).unwrap();
            answer
        })
        .collect();
    for answer in answers {
        assert_eq!(
            answer.blocking_recv().unwrap(),
            CallResult::Returned(Bytes::from(vec![1, 2]))
        );
    }
    let took = start.elapsed();
    assert!(
        (Duration::from_secs(1)..Duration::from_secs(3)).contains(&took),
        "{took:?}"
    );
    drop(worker.requests);
    worker.thread.join().unwrap();
}
