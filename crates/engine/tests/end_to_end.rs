//! The engine's commands end to end: `follow` a scripted chain into Kafka, `engine archive` the
//! recording to S3-compatible storage, then `engine replay` it from Kafka and from the archive.
//! All three must print the same summary. Needs Kafka and RustFS: `scripts/stack up` locally
//! (or set KAFKA_BROKERS and OMNIMARKET_S3_ENDPOINT); CI runs it against service containers.

use std::cell::Cell;
use std::process::Command;
use std::time::Duration;

use alloy_primitives::{U256, address};
use alloy_sol_types::{SolEvent, SolValue};
use chain_io::{CallEndpoint, CallWorker, ChainError, ChainReader, FollowerConfig, Header, Start};
use det::SeededRng;
use engine::commands::{self, Endpoints};
use engine::{EngineConfig, EngineMetrics, M1_TOPICS};
use futures::future::LocalBoxFuture;
use types::chain::{Address, B256, Bytes, CallResult, EthCall, Log};
use venues::multicall;
use venues::v2::{self, BASE, Reserves, Sync};

const TOKEN_A: Address = address!("1000000000000000000000000000000000000001");
const TOKEN_B: Address = address!("2000000000000000000000000000000000000002");
const FIRST: u64 = 100;
const LAST: u64 = 109;

fn pair() -> Address {
    BASE.pair_address(TOKEN_A, TOKEN_B)
}

fn hash(number: u64) -> B256 {
    B256::left_padding_from(&number.to_be_bytes())
}

/// Blocks 100 to 109, one `Sync` each. The head moves one block per poll, so the follower
/// starts at the first and reads each as it appears.
struct ScriptedChain {
    head: Cell<u64>,
}

impl ChainReader for ScriptedChain {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
        let head = self.head.get();
        self.head.set((head + 1).min(LAST));
        Box::pin(async move { Ok(head) })
    }

    fn header(&self, number: u64) -> LocalBoxFuture<'_, Result<Option<Header>, ChainError>> {
        Box::pin(async move {
            Ok((number <= LAST).then(|| Header {
                number,
                hash: hash(number),
                parent_hash: hash(number - 1),
                timestamp: 1_767_225_600 + 2 * (number - FIRST),
            }))
        })
    }

    fn logs<'a>(
        &'a self,
        block_hash: B256,
        topics: &'a [B256],
    ) -> LocalBoxFuture<'a, Result<Vec<Log>, ChainError>> {
        let number = (FIRST..=LAST).find(|n| hash(*n) == block_hash).unwrap();
        let sync = Log {
            address: pair(),
            topics: vec![Sync::SIGNATURE_HASH],
            data: Bytes::from((U256::from(number), U256::from(2 * number)).abi_encode()),
            log_index: 0,
            transaction_hash: B256::ZERO,
        };
        Box::pin(async move {
            Ok(vec![sync]
                .into_iter()
                .filter(|log| topics.contains(&log.topics[0]))
                .collect())
        })
    }
}

/// A node that knows the pair's tokens, and answers its reserves as 100 and 200.
struct Node;

impl CallEndpoint for Node {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
        Box::pin(async { Ok(LAST) })
    }

    fn call<'a>(&'a self, call: &'a EthCall) -> LocalBoxFuture<'a, Result<CallResult, ChainError>> {
        let results: Vec<Option<Vec<u8>>> = multicall::decode_calls(&call.data)
            .unwrap()
            .into_iter()
            .map(|(target, data)| {
                assert_eq!(target, pair());
                Some(if data.as_ref() == v2::token0_call().as_slice() {
                    v2::encode_address(TOKEN_A)
                } else if data.as_ref() == v2::token1_call().as_slice() {
                    v2::encode_address(TOKEN_B)
                } else {
                    v2::encode_reserves(Reserves {
                        reserve0: 100,
                        reserve1: 200,
                    })
                })
            })
            .collect();
        Box::pin(async move {
            Ok(CallResult::Returned(Bytes::from(
                multicall::encode_results(&results),
            )))
        })
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// The follower and the call worker on their own threads, as `chain_io`'s spawns run them,
/// against the scripted chain and node.
#[allow(clippy::disallowed_methods)] // the test's own chain-io threads
fn endpoints() -> Endpoints {
    let (sender, blocks) = tokio::sync::mpsc::channel(1024);
    let follower = std::thread::spawn(move || {
        let chain = ScriptedChain {
            head: Cell::new(FIRST),
        };
        let config = FollowerConfig {
            poll: Duration::from_millis(20),
            topics: M1_TOPICS.to_vec(),
            start: Start::Latest,
            run_for: Some(Duration::from_secs(2)),
        };
        runtime().block_on(chain_io::follow_head(&chain, &config, &sender))
    });
    let (requests, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (report, gave_up) = tokio::sync::oneshot::channel();
    let thread = std::thread::spawn(move || {
        runtime().block_on(chain_io::answer_calls(
            &Node,
            receiver,
            Duration::ZERO,
            report,
        ));
    });
    Endpoints {
        blocks,
        follower,
        calls: CallWorker {
            requests,
            gave_up,
            thread,
        },
        logs: None,
    }
}

/// Runs the engine binary with `args` and the test's Kafka and archive, and returns what it
/// printed. Fails the test if it exits with an error.
fn engine(args: &[&str], brokers: &str, s3_endpoint: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_engine"))
        .args(args)
        .args(["--kafka", brokers, "--s3-endpoint", s3_endpoint])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "engine {args:?} failed:\n{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
}

/// Every scripted block, its `Sync` and the pair's one proof.
fn assert_ran_the_scripted_chain(summary: &str) {
    let lines: Vec<&str> = summary.lines().collect();
    assert_eq!(
        lines[0],
        "head 109 0x000000000000000000000000000000000000000000000000000000000000006d"
    );
    assert!(
        lines[1].starts_with("10 blocks, 10 logs, 0 reorgs detected"),
        "{summary}"
    );
    assert_eq!(
        lines[2],
        "v2: 1 pairs tracked, 0 rejected, 1 verification calls (0 failed)"
    );
    assert!(lines[4].starts_with("10 pool updates, "), "{summary}");
}

/// The same run with the inputs in memory, so it needs no services.
#[test]
fn a_followed_run_replays_to_the_same_summary_from_memory() {
    let followed = commands::follow(
        endpoints(),
        None,
        "engine-test",
        EngineConfig::base(),
        &EngineMetrics::default(),
    )
    .unwrap();
    assert_ran_the_scripted_chain(&followed.summary.to_string());
    assert_eq!(
        commands::replay(followed.recording).unwrap(),
        followed.summary
    );
}

#[test]
#[ignore = "needs Kafka and RustFS: scripts/stack up, then cargo test -p engine --test end_to_end -- --ignored"]
fn a_followed_run_replays_to_the_same_summary_from_kafka_and_from_the_archive() {
    let brokers = std::env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".into());
    let s3_endpoint =
        std::env::var("OMNIMARKET_S3_ENDPOINT").unwrap_or_else(|_| "http://localhost:9000".into());
    // A fresh core instance per run, so earlier runs' records on the topic are skipped.
    let core_instance = format!("engine-test-{:016x}", SeededRng::from_os().seed());

    let followed = commands::follow(
        endpoints(),
        Some(&brokers),
        &core_instance,
        EngineConfig::base(),
        &EngineMetrics::default(),
    )
    .unwrap();
    let summary = followed.summary.to_string();
    assert_ran_the_scripted_chain(&summary);
    assert!(followed.recording.is_empty(), "the inputs went to Kafka");

    let archived = engine(
        &["archive", "--core-instance", &core_instance],
        &brokers,
        &s3_endpoint,
    );
    assert!(
        archived.starts_with("archived ") && archived.contains(&core_instance),
        "{archived}"
    );
    let inputs = archived.split(' ').nth(1).unwrap();

    let replayed = format!("replaying {inputs} inputs of {core_instance}\n{summary}");
    let from_kafka = engine(
        &["replay", "--core-instance", &core_instance],
        &brokers,
        &s3_endpoint,
    );
    assert_eq!(from_kafka, replayed);
    let from_archive = engine(
        &[
            "replay",
            "--from-archive",
            "--core-instance",
            &core_instance,
        ],
        &brokers,
        &s3_endpoint,
    );
    assert_eq!(from_archive, replayed);
}

#[test]
fn a_recording_that_doesnt_start_with_the_config_is_refused() {
    assert_eq!(
        commands::replay(Vec::new()).unwrap_err().to_string(),
        "the recording doesn't start with an engine config"
    );
}
