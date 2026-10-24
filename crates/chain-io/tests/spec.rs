//! The head follower against what indexer.md and D80 say it does: on a scripted chain for
//! reorgs, and on a scripted node for faults (build rule 8).

mod scripted;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Duration;

use chain_io::{
    ChainError, ChainReader, FollowerConfig, Header, Start, check_block_endpoint, follow_head,
};
use futures::future::LocalBoxFuture;
use scripted::{
    BLOCK_NOT_FOUND, Method, NO_NODES, NO_NODES_ERROR, RATE_LIMITED, Script, ScriptedNode,
};
use tokio::sync::mpsc;
use types::chain::{B256, Block, Log};

/// `tag` names the fork a block is on.
fn hash(tag: u8, number: u64) -> B256 {
    let mut bytes = [tag; 32];
    bytes[24..].copy_from_slice(&number.to_be_bytes());
    B256::new(bytes)
}

/// (number, hash, parent hash) of one canonical block.
type Entry = (u64, B256, B256);

fn fork(tag: u8, parent_tag: u8, numbers: std::ops::RangeInclusive<u64>) -> Vec<Entry> {
    let first = *numbers.start();
    numbers
        .map(|n| {
            let parent = if n == first { parent_tag } else { tag };
            (n, hash(tag, n), hash(parent, n - 1))
        })
        .collect()
}

/// A node whose head moves per poll and whose canonical chain can change at set moments. It
/// serves logs only for blocks on its canonical chain, as `mainnet.base.org` does (an unknown
/// hash answers `-32001 block not found`, checked live 2026-09-30).
#[derive(Default)]
struct ScriptedChain {
    heads: RefCell<Vec<u64>>,
    canonical: RefCell<BTreeMap<u64, (B256, B256)>>,
    polls: Cell<usize>,
    /// Blocks that become canonical when poll `k` (from 1) is answered.
    at_poll: RefCell<BTreeMap<usize, Vec<Entry>>>,
    /// Blocks that become canonical right after the header of block `n` is read.
    after_header: RefCell<BTreeMap<u64, Vec<Entry>>>,
}

impl ScriptedChain {
    fn with(entries: Vec<Entry>, heads: Vec<u64>) -> Self {
        let chain = Self {
            heads: RefCell::new(heads),
            ..Self::default()
        };
        chain.apply(entries);
        chain
    }

    fn apply(&self, entries: Vec<Entry>) {
        let mut canonical = self.canonical.borrow_mut();
        for (number, hash, parent) in entries {
            canonical.insert(number, (hash, parent));
        }
    }
}

impl ChainReader for ScriptedChain {
    fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
        Box::pin(async move {
            self.polls.set(self.polls.get() + 1);
            if let Some(entries) = self.at_poll.borrow_mut().remove(&self.polls.get()) {
                self.apply(entries);
            }
            let mut heads = self.heads.borrow_mut();
            Ok(if heads.len() > 1 {
                heads.remove(0)
            } else {
                heads[0]
            })
        })
    }

    fn header(&self, number: u64) -> LocalBoxFuture<'_, Result<Option<Header>, ChainError>> {
        Box::pin(async move {
            let header = self
                .canonical
                .borrow()
                .get(&number)
                .map(|&(hash, parent_hash)| Header {
                    number,
                    hash,
                    parent_hash,
                    timestamp: number * 2,
                });
            if let Some(entries) = self.after_header.borrow_mut().remove(&number) {
                self.apply(entries);
            }
            Ok(header)
        })
    }

    fn logs<'a>(
        &'a self,
        block_hash: B256,
        _topics: &'a [B256],
    ) -> LocalBoxFuture<'a, Result<Vec<Log>, ChainError>> {
        Box::pin(async move {
            let known = self
                .canonical
                .borrow()
                .values()
                .any(|&(hash, _)| hash == block_hash);
            if known {
                Ok(Vec::new())
            } else {
                Err(ChainError::Rpc(format!(
                    "block not found: hash {block_hash}"
                )))
            }
        })
    }
}

/// Runs the follower for `run_for` of simulated time and returns what it delivered, or
/// `None` if it was still running a minute after `run_for` ended.
fn follow(chain: &ScriptedChain, start: Start, run_for: Duration) -> Option<Vec<Block>> {
    det::run_simulated(async {
        let config = FollowerConfig {
            poll: Duration::from_millis(500),
            topics: vec![B256::repeat_byte(9)],
            start,
            run_for: Some(run_for),
        };
        let (sender, mut receiver) = mpsc::channel(64);
        let finished = tokio::time::timeout(
            run_for + Duration::from_secs(60),
            follow_head(chain, &config, &sender),
        )
        .await
        .ok();
        drop(sender);
        let mut blocks = Vec::new();
        while let Some(block) = receiver.recv().await {
            blocks.push(block);
        }
        finished.map(|result| {
            result.unwrap();
            blocks
        })
    })
}

fn ids(blocks: &[Block]) -> Vec<(u64, B256)> {
    blocks.iter().map(|b| (b.number, b.hash)).collect()
}

// indexer.md: "A header the node doesn't have yet (load-balanced nodes can lag each other)
// waits for the next poll."
#[test]
fn a_header_the_node_lacks_waits_for_the_next_poll() {
    let chain = ScriptedChain::with(fork(b'a', b'a', 10..=11), vec![12]);
    chain
        .at_poll
        .borrow_mut()
        .insert(3, fork(b'a', b'a', 12..=12));
    let blocks = follow(&chain, Start::At(10), Duration::from_secs(5)).unwrap();
    assert_eq!(
        ids(&blocks),
        [
            (10, hash(b'a', 10)),
            (11, hash(b'a', 11)),
            (12, hash(b'a', 12))
        ]
    );
}

// indexer.md: "the core sees every block in order". Block 11 is replaced between reading
// its header and its logs, so the logs call by hash can never succeed. The follower must
// move on to the new canonical 11 rather than retry a dead hash forever: stalled, it never
// delivers the block that would show the core the reorg.
#[test]
fn a_block_reorged_out_between_header_and_logs_does_not_stall_the_follower() {
    let chain = ScriptedChain::with(fork(b'a', b'a', 10..=11), vec![10, 12]);
    chain
        .after_header
        .borrow_mut()
        .insert(11, fork(b'b', b'a', 11..=12));
    let blocks = follow(&chain, Start::At(10), Duration::from_secs(5))
        .expect("the follower stalled on a reorged-out block");
    assert_eq!(
        ids(&blocks),
        [
            (10, hash(b'a', 10)),
            (11, hash(b'b', 11)),
            (12, hash(b'b', 12))
        ]
    );
}

/// What a follower run against a scripted node did.
struct Run {
    result: Result<(), ChainError>,
    /// When `follow_head` returned, by the node's clock.
    returned: Duration,
    blocks: Vec<Block>,
}

/// Runs the follower against `node` from block 10 until it returns, `run_for` ends, or 300s
/// pass. Call inside `det::run_simulated`, as the node is made.
async fn follow_node(node: &ScriptedNode, run_for: Duration) -> Run {
    let config = FollowerConfig {
        poll: Duration::from_millis(500),
        topics: vec![B256::repeat_byte(9)],
        start: Start::At(10),
        run_for: Some(run_for),
    };
    let (sender, mut receiver) = mpsc::channel(64);
    let result = tokio::time::timeout(
        Duration::from_secs(300),
        follow_head(&node.chain(), &config, &sender),
    )
    .await
    .expect("the follower neither gave up nor finished");
    let returned = node.now();
    drop(sender);
    let mut blocks = Vec::new();
    while let Some(block) = receiver.recv().await {
        blocks.push(block);
    }
    Run {
        result,
        returned,
        blocks,
    }
}

fn unanswered(result: Result<(), ChainError>) -> (&'static str, String) {
    match result {
        Err(ChainError::Unanswered { what, last, .. }) => (what, last),
        other => panic!("expected the endpoint to go unanswered, got {other:?}"),
    }
}

fn millis(gaps: Vec<Duration>) -> Vec<u128> {
    gaps.iter().map(Duration::as_millis).collect()
}

// indexer.md: "a failed call is retried with backoff (250ms doubling to 8s), never skipped".
#[test]
fn a_failed_call_backs_off_from_250ms_doubling_to_8s() {
    det::run_simulated(async {
        let node = ScriptedNode::new(10);
        let rate_limited = Script::FailThenAnswer {
            times: 8,
            error: RATE_LIMITED,
        };
        node.script(Method::BlockNumber, rate_limited);
        let run = follow_node(&node, Duration::from_secs(60)).await;
        run.result.unwrap();
        assert_eq!(
            millis(node.retry_gaps(Method::BlockNumber)),
            [250, 500, 1_000, 2_000, 4_000, 8_000, 8_000, 8_000]
        );
        assert_eq!(ids(&run.blocks), [(10, scripted::block_hash(10))]);
    });
}

// indexer.md: "The check before a run reads what one poll reads, retried for up to 5s each, so
// a dead endpoint stops `engine follow` within about 8s, before Kafka or the core start."
// Also: "Each attempt is bounded too, so an endpoint that takes the connection and never
// replies counts as unanswered."
#[test]
fn a_dead_block_endpoint_fails_the_check_within_8s() {
    let topics = [B256::repeat_byte(9)];
    for (dead, last_error) in [
        (Script::Fail(NO_NODES), NO_NODES_ERROR),
        (Script::Hang, "no reply within 5s"),
    ] {
        det::run_simulated(async {
            let node = ScriptedNode::new(10);
            node.script_all(dead);
            let result = tokio::time::timeout(
                Duration::from_secs(300),
                check_block_endpoint(&node.chain(), &topics),
            )
            .await
            .expect("the check never gave up on a dead endpoint");
            node.assert_gave_up_within(
                Method::BlockNumber,
                node.now(),
                Duration::from_secs(5),
                Duration::from_secs(3),
            );
            let (what, last) = unanswered(result);
            assert_eq!((what, last.as_str()), ("latest block number", last_error));
        });
    }

    det::run_simulated(async {
        let node = ScriptedNode::new(10);
        check_block_endpoint(&node.chain(), &topics).await
    })
    .unwrap();
}

// indexer.md: "During a run, a read still unanswered after 60s, or a height still unreadable
// after 60s of reading it again, makes the follower give up: it returns the error, which ends
// the core's blocks, and `engine follow` exits with it once the core has finished what's in
// flight."
#[test]
fn a_block_endpoint_that_dies_mid_run_stops_the_follower_after_60s() {
    for (dead, last_error) in [
        (Script::Fail(NO_NODES), NO_NODES_ERROR),
        (Script::Hang, "no reply within 60s"),
    ] {
        det::run_simulated(async {
            let node = ScriptedNode::new(13);
            // The follower delivers 10 to 13 at once and polls every 500ms; the node dies
            // between the polls at 500ms and 1s.
            let dies = async {
                tokio::time::sleep(Duration::from_millis(750)).await;
                node.script_all(dead);
            };
            let (run, ()) = futures::join!(follow_node(&node, Duration::from_secs(600)), dies);
            let (what, last) = unanswered(run.result);
            assert_eq!((what, last.as_str()), ("latest block number", last_error));
            node.assert_gave_up_within(
                Method::BlockNumber,
                run.returned,
                Duration::from_secs(60),
                Duration::from_secs(8),
            );
            // Every block from before the endpoint died, in order: nothing skipped.
            let numbers: Vec<u64> = run.blocks.iter().map(|b| b.number).collect();
            assert_eq!(numbers, [10, 11, 12, 13]);
        });
    }
}

// indexer.md: "If the node doesn't know a block's hash when its logs are read (`-32001 block not
// found`), the block was either replaced after its header was read (a reorg) or a lagging node
// hasn't got it yet; either way the follower backs off (250ms doubling to 8s), reads the
// height's header again and continues with whatever is canonical now."
#[test]
fn a_block_the_node_lacks_is_read_again_with_backoff_until_it_has_it() {
    det::run_simulated(async {
        let node = ScriptedNode::new(10);
        let lagging = Script::FailThenAnswer {
            times: 4,
            error: BLOCK_NOT_FOUND,
        };
        node.script(Method::Logs, lagging);
        let run = follow_node(&node, Duration::from_secs(5)).await;
        run.result.unwrap();
        assert_eq!(
            millis(node.retry_gaps(Method::Logs)),
            [250, 500, 1_000, 2_000]
        );
        // The height's header is read again before each read of its logs.
        assert_eq!(node.requests(Method::BlockByNumber).len(), 5);
        assert_eq!(ids(&run.blocks), [(10, scripted::block_hash(10))]);
    });
}

// D88: reading a height again when the node doesn't know its hash "is bounded by the same 60s".
#[test]
fn a_node_that_never_knows_the_hash_stops_the_follower_after_60s() {
    det::run_simulated(async {
        let node = ScriptedNode::new(10);
        node.script(Method::Logs, Script::Fail(BLOCK_NOT_FOUND));
        let run = follow_node(&node, Duration::from_secs(600)).await;
        let (what, last) = unanswered(run.result);
        assert_eq!(what, "block logs");
        assert_eq!(
            last,
            format!("block not found: {} at height 10", scripted::block_hash(10))
        );
        node.assert_backs_off(
            Method::Logs,
            Duration::from_millis(250),
            Duration::from_secs(8),
        );
        node.assert_gave_up_within(
            Method::Logs,
            run.returned,
            Duration::from_secs(60),
            Duration::from_secs(8),
        );
        assert!(run.blocks.is_empty(), "{:?}", ids(&run.blocks));
    });
}
