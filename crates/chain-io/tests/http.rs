//! `HttpChain` and the call and logs workers against a local JSON-RPC server: which of the node's
//! errors are the call's or the read's result and which are failures to retry (indexer.md, D88),
//! and their rates.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use chain_io::{
    CallEndpoint, ChainError, HttpChain, LogsEndpoint, spawn_call_worker, spawn_logs_worker,
};
use types::chain::{
    Address, B256, BlockLog, Bytes, CallResult, EthCall, Log, LogsQuery, LogsResult,
};

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

fn read_logs(url: &str) -> Result<LogsResult, ChainError> {
    let query = LogsQuery {
        addresses: vec![Address::repeat_byte(0xaa)],
        topics: vec![B256::repeat_byte(0xc4)],
        from_block: 100,
        to_block: 599,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { HttpChain::new(url).unwrap().logs_in_range(&query).await })
}

const LOG: &str = r#"{"address":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","topics":["0xc4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4c4"],"data":"0x0102","blockNumber":"0x1f4","blockHash":"0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","blockTimestamp":"0x6a000000","transactionHash":"0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","transactionIndex":"0x0","logIndex":"0x7","removed":false}"#;

#[test]
fn a_ranged_read_returns_each_log_with_its_block() {
    let url = node(
        "200 OK",
        Box::leak(format!(r#""result":[{LOG}]"#).into_boxed_str()),
    );
    assert_eq!(
        read_logs(&url).unwrap(),
        LogsResult::Returned(vec![BlockLog {
            block_number: 500,
            block_hash: B256::repeat_byte(0xbb),
            block_timestamp: 0x6a00_0000,
            log: Log {
                address: Address::repeat_byte(0xaa),
                topics: vec![B256::repeat_byte(0xc4)],
                data: Bytes::from(vec![1, 2]),
                log_index: 7,
                transaction_hash: B256::repeat_byte(0xdd),
            },
        }])
    );
}

// indexer.md: "A log without its block's timestamp fails its read too."
#[test]
fn a_log_without_its_block_timestamp_fails_the_read() {
    let log = LOG.replace(r#""blockTimestamp":"0x6a000000","#, "");
    let url = node(
        "200 OK",
        Box::leak(format!(r#""result":[{log}]"#).into_boxed_str()),
    );
    assert_eq!(
        read_logs(&url).unwrap(),
        LogsResult::Failed(
            "a log without its block number, hash, timestamp, index or transaction".into()
        )
    );
}

// Base's public endpoint refuses a range over 501 blocks. That's the node's answer, so the core
// splits the range.
#[test]
fn a_range_the_node_refuses_is_a_failed_read() {
    let url = node(
        "200 OK",
        r#""error":{"code":-32614,"message":"eth_getLogs is limited to a 500 range"}"#,
    );
    assert_eq!(
        read_logs(&url).unwrap(),
        LogsResult::Failed("eth_getLogs is limited to a 500 range".into())
    );
}

#[test]
fn a_rate_limited_log_read_is_a_failure_to_retry() {
    let url = node(
        "429 Too Many Requests",
        r#""error":{"code":-32016,"message":"over rate limit"}"#,
    );
    match read_logs(&url) {
        Err(ChainError::Rpc(error)) => assert!(error.contains("over rate limit"), "{error}"),
        other => panic!("expected a failure to retry, got {other:?}"),
    }
}

// Six reads sent at once at 5 a second: the last starts 1s after the first.
#[test]
fn the_logs_worker_starts_at_most_reads_per_second() {
    let url = node("200 OK", r#""result":[]"#);
    let worker = spawn_logs_worker(&url, 5).unwrap();
    let start = Instant::now();
    let answers: Vec<_> = (0..6)
        .map(|_| {
            let (reply, answer) = tokio::sync::oneshot::channel();
            let query = LogsQuery {
                addresses: vec![],
                topics: vec![],
                from_block: 1,
                to_block: 2,
            };
            worker.requests.send((query, reply)).unwrap();
            answer
        })
        .collect();
    for answer in answers {
        assert_eq!(
            answer.blocking_recv().unwrap(),
            LogsResult::Returned(vec![])
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
