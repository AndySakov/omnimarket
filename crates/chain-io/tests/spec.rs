//! The head follower against what indexer.md and D80 say it does, on a scripted chain.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Duration;

use chain_io::{ChainError, ChainReader, FollowerConfig, Header, Start, follow_head};
use futures::future::LocalBoxFuture;
use tokio::sync::mpsc;
use tokio::time::Instant;
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
    poll_failures_left: Cell<u32>,
    poll_failed_at: RefCell<Vec<Instant>>,
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
            if self.poll_failures_left.get() > 0 {
                self.poll_failures_left
                    .set(self.poll_failures_left.get() - 1);
                self.poll_failed_at.borrow_mut().push(Instant::now());
                return Err(ChainError::Rpc("over rate limit".into()));
            }
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

// indexer.md: "a failed call is retried with backoff (250ms doubling to 8s), never skipped".
#[test]
fn a_failed_call_backs_off_from_250ms_doubling_to_8s() {
    let chain = ScriptedChain::with(fork(b'a', b'a', 10..=10), vec![10]);
    chain.poll_failures_left.set(8);
    follow(&chain, Start::At(10), Duration::from_secs(60)).unwrap();
    let failed_at = chain.poll_failed_at.borrow();
    let gaps: Vec<u128> = failed_at
        .windows(2)
        .map(|w| (w[1] - w[0]).as_millis())
        .collect();
    assert_eq!(gaps, [250, 500, 1_000, 2_000, 4_000, 8_000, 8_000]);
}

// indexer.md: "the core sees every block in order". Block 11 is replaced between reading
// its header and its logs, so the logs call by hash can never succeed. The follower must
// move on to the new canonical 11 rather than retry a dead hash forever: stalled, it never
// delivers the block that would show the core the reorg.
#[test]
#[ignore = "spec gap: the follower retries logs of a reorged-out hash forever"]
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
