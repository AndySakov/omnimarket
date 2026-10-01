use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc::Sender;
use tokio::time::Instant;
use types::chain::{B256, Block};

use crate::retry::{ANSWER_LIMIT, CHECK_LIMIT, FIRST_BACKOFF, MAX_BACKOFF, retry_for};
use crate::{ChainError, ChainReader, HttpChain};

pub enum Start {
    /// The node's latest block when the follower starts.
    Latest,
    At(u64),
}

pub struct FollowerConfig {
    /// How often to ask the node for its latest block number.
    pub poll: Duration,
    /// First topics of the logs to deliver with each block.
    pub topics: Vec<B256>,
    pub start: Start,
    /// Stop after this long, ending the core's event source. `None` runs until the core stops.
    pub run_for: Option<Duration>,
}

/// Delivers every canonical block from the start, in order, with its logs, until `run_for`
/// ends or the receiver is dropped. A block the poll skipped is fetched by number, so the core
/// never sees a gap. A failed call is retried with backoff, never skipped: the input log must
/// hold every block (D54). A read still unanswered after `ANSWER_LIMIT` means the endpoint is
/// unusable: the follower returns the error, which ends the core's blocks (D88).
pub async fn follow_head(
    reader: &dyn ChainReader,
    config: &FollowerConfig,
    sender: &Sender<Block>,
) -> Result<(), ChainError> {
    let deadline = config.run_for.map(|d| Instant::now() + d);
    let mut next = match config.start {
        Start::At(number) => number,
        Start::Latest => {
            retry_for("latest block number", ANSWER_LIMIT, || {
                reader.latest_number()
            })
            .await?
        }
    };
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Ok(());
        }
        let latest = retry_for("latest block number", ANSWER_LIMIT, || {
            reader.latest_number()
        })
        .await?;
        while next <= latest {
            let Some((header, mut logs)) = block_at(reader, next, &config.topics).await? else {
                // Load-balanced nodes can lag each other: ask again after the next poll.
                break;
            };
            logs.sort_by_key(|log| log.log_index);
            let block = Block {
                number: header.number,
                hash: header.hash,
                parent_hash: header.parent_hash,
                timestamp: header.timestamp,
                logs,
            };
            if sender.send(block).await.is_err() {
                return Ok(());
            }
            next += 1;
        }
        tokio::time::sleep(config.poll).await;
    }
}

/// The check before a run (D88): what one poll reads, retried for up to `CHECK_LIMIT` each. The
/// endpoint's latest block number, that block's header, and the header's logs of `topics`.
pub async fn check_block_endpoint(
    reader: &dyn ChainReader,
    topics: &[B256],
) -> Result<(), ChainError> {
    let latest = retry_for("latest block number", CHECK_LIMIT, || {
        reader.latest_number()
    })
    .await?;
    // A load-balanced node can lack a block another just reported; the number was the answer.
    if let Some(header) = retry_for("block header", CHECK_LIMIT, || reader.header(latest)).await? {
        retry_for("block logs", CHECK_LIMIT, || {
            reader.logs(header.hash, topics)
        })
        .await?;
    }
    Ok(())
}

/// The header at `number` and its logs, read by that header's hash. `None` when the node
/// doesn't have a block at `number` yet.
///
/// If the node doesn't know the hash when the logs are read, either the block was replaced
/// after its header was read (a reorg) or a lagging load-balanced node hasn't got it yet. In
/// both cases the answer is to read whatever is canonical at `number` now: retrying the old
/// hash would never succeed after a reorg, and the follower would stall. Each read is bounded
/// by `ANSWER_LIMIT`, and so is reading the height again (D88).
async fn block_at(
    reader: &dyn ChainReader,
    number: u64,
    topics: &[B256],
) -> Result<Option<(crate::Header, Vec<types::chain::Log>)>, ChainError> {
    let first = Instant::now();
    let mut backoff = FIRST_BACKOFF;
    loop {
        let header = retry_for("block header", ANSWER_LIMIT, || reader.header(number)).await?;
        let Some(header) = header else {
            return Ok(None);
        };
        // An unknown hash comes back as `None`, an answer, so only other failures are retried.
        let logs = retry_for("block logs", ANSWER_LIMIT, || {
            let logs = reader.logs(header.hash, topics);
            Box::pin(async move {
                match logs.await {
                    Ok(logs) => Ok(Some(logs)),
                    Err(error) if is_unknown_block(&error) => Ok(None),
                    Err(error) => Err(error),
                }
            })
        })
        .await?;
        if let Some(logs) = logs {
            return Ok(Some((header, logs)));
        }
        if first.elapsed() >= ANSWER_LIMIT {
            return Err(ChainError::Unanswered {
                what: "block logs",
                waited: first.elapsed(),
                last: format!("block not found: {} at height {number}", header.hash),
            });
        }
        tracing::warn!(number, hash = %header.hash, ?backoff, "block unknown when reading its logs; reading the height again");
        // A lagging node answers this until it catches up: back off like any failure.
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// A node's answer for a block hash it doesn't have (`-32001 block not found` on Base).
fn is_unknown_block(error: &ChainError) -> bool {
    let ChainError::Rpc(message) = error else {
        return false;
    };
    let message = message.to_lowercase();
    message.contains("block not found") || message.contains("unknown block")
}

/// Checks that `rpc_url` answers what the follower reads (`check_block_endpoint`), then runs
/// `follow_head` against it on its own thread with its own runtime, so chain I/O never shares a
/// thread with the core (rule 6). Fails without starting the thread if the check fails.
pub fn spawn_head_follower(
    rpc_url: &str,
    config: FollowerConfig,
    sender: Sender<Block>,
) -> Result<JoinHandle<Result<(), ChainError>>, ChainError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| ChainError::Rpc(format!("building the follower runtime: {e}")))?;
    let chain = HttpChain::new(rpc_url)?;
    runtime.block_on(check_block_endpoint(&chain, &config.topics))?;
    // The runtime and the endpoint move to the follower's thread; the check ran on this one.
    Ok(std::thread::spawn(move || {
        runtime.block_on(follow_head(&chain, &config, &sender))
    }))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use futures::future::LocalBoxFuture;
    use tokio::sync::mpsc;
    use types::chain::{Address, Bytes, Log};

    use super::*;
    use crate::Header;

    /// A chain whose head moves by a scripted amount per poll, and whose first few calls fail.
    struct FakeChain {
        heads: RefCell<Vec<u64>>,
        failures_left: Cell<u32>,
        topic: B256,
    }

    fn hash(number: u64) -> B256 {
        B256::left_padding_from(&number.to_be_bytes())
    }

    impl FakeChain {
        fn fail_once(&self) -> Result<(), ChainError> {
            if self.failures_left.get() > 0 {
                self.failures_left.set(self.failures_left.get() - 1);
                return Err(ChainError::Rpc("rate limited".into()));
            }
            Ok(())
        }
    }

    impl ChainReader for FakeChain {
        fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
            Box::pin(async move {
                self.fail_once()?;
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
                self.fail_once()?;
                Ok(Some(Header {
                    number,
                    hash: hash(number),
                    parent_hash: hash(number - 1),
                    timestamp: number * 2,
                }))
            })
        }

        fn logs<'a>(
            &'a self,
            block_hash: B256,
            topics: &'a [B256],
        ) -> LocalBoxFuture<'a, Result<Vec<Log>, ChainError>> {
            Box::pin(async move {
                assert_eq!(topics, [self.topic]);
                // Two logs, out of order, to check the follower sorts them.
                Ok([5, 2]
                    .map(|log_index| Log {
                        address: Address::ZERO,
                        topics: vec![self.topic],
                        data: Bytes::new(),
                        log_index,
                        transaction_hash: block_hash,
                    })
                    .to_vec())
            })
        }
    }

    /// A chain where the logs of some hashes can't be read: `-32001 block not found`.
    struct ReorgingChain {
        /// The canonical hash at each height, per header read: a height's list advances each
        /// time its header is read, and its last entry sticks.
        headers: RefCell<std::collections::BTreeMap<u64, Vec<B256>>>,
        /// Hashes whose logs fail this many more times.
        unknown: RefCell<std::collections::BTreeMap<B256, u32>>,
        head: u64,
        /// When each logs call was made.
        logs_at: RefCell<Vec<Instant>>,
    }

    impl ChainReader for ReorgingChain {
        fn latest_number(&self) -> LocalBoxFuture<'_, Result<u64, ChainError>> {
            Box::pin(async move { Ok(self.head) })
        }

        fn header(&self, number: u64) -> LocalBoxFuture<'_, Result<Option<Header>, ChainError>> {
            Box::pin(async move {
                let mut headers = self.headers.borrow_mut();
                let hashes = headers.get_mut(&number).expect("scripted height");
                let hash = if hashes.len() > 1 {
                    hashes.remove(0)
                } else {
                    hashes[0]
                };
                Ok(Some(Header {
                    number,
                    hash,
                    parent_hash: B256::ZERO,
                    timestamp: number * 2,
                }))
            })
        }

        fn logs<'a>(
            &'a self,
            block_hash: B256,
            _topics: &'a [B256],
        ) -> LocalBoxFuture<'a, Result<Vec<Log>, ChainError>> {
            Box::pin(async move {
                self.logs_at.borrow_mut().push(Instant::now());
                let mut unknown = self.unknown.borrow_mut();
                match unknown.get_mut(&block_hash) {
                    Some(0) | None => Ok(Vec::new()),
                    Some(left) => {
                        *left -= 1;
                        Err(ChainError::Rpc(format!(
                            "server returned an error response: error code -32001: block not found: {block_hash}"
                        )))
                    }
                }
            })
        }
    }

    fn follow_to_end(chain: &ReorgingChain) -> Vec<(u64, B256)> {
        det::run_simulated(async {
            let config = FollowerConfig {
                poll: Duration::from_millis(500),
                topics: vec![B256::repeat_byte(9)],
                start: Start::At(10),
                run_for: Some(Duration::from_secs(5)),
            };
            let (sender, mut receiver) = mpsc::channel(64);
            // A follower stuck retrying a dead hash never returns; fail instead of hanging.
            tokio::time::timeout(
                Duration::from_secs(60),
                follow_head(chain, &config, &sender),
            )
            .await
            .expect("the follower stalled")
            .unwrap();
            drop(sender);
            let mut blocks = Vec::new();
            while let Some(block) = receiver.recv().await {
                blocks.push((block.number, block.hash));
            }
            blocks
        })
    }

    #[test]
    fn a_block_replaced_between_its_header_and_logs_is_read_again() {
        let a11 = B256::repeat_byte(0xa1);
        let b11 = B256::repeat_byte(0xb1);
        let chain = ReorgingChain {
            headers: RefCell::new(
                [
                    (10, vec![hash(10)]),
                    (11, vec![a11, b11]),
                    (12, vec![hash(12)]),
                ]
                .into(),
            ),
            // 11a was reorged out after its header was read: its logs never come.
            unknown: RefCell::new([(a11, u32::MAX)].into()),
            head: 12,
            logs_at: RefCell::default(),
        };
        assert_eq!(
            follow_to_end(&chain),
            [(10, hash(10)), (11, b11), (12, hash(12))]
        );
    }

    #[test]
    fn a_lagging_node_still_delivers_every_block_in_order() {
        let chain = ReorgingChain {
            headers: RefCell::new(
                [
                    (10, vec![hash(10)]),
                    (11, vec![hash(11)]),
                    (12, vec![hash(12)]),
                ]
                .into(),
            ),
            // This node doesn't have 11 yet the first two times it's asked.
            unknown: RefCell::new([(hash(11), 2)].into()),
            head: 12,
            logs_at: RefCell::default(),
        };
        assert_eq!(
            follow_to_end(&chain),
            [(10, hash(10)), (11, hash(11)), (12, hash(12))]
        );
    }

    // A lagging node answers "block not found" until it catches up; the follower must not
    // hammer a rate-limited endpoint meanwhile.
    #[test]
    fn a_lagging_node_is_asked_again_with_backoff() {
        let chain = ReorgingChain {
            headers: RefCell::new([(10, vec![hash(10)])].into()),
            unknown: RefCell::new([(hash(10), 4)].into()),
            head: 10,
            logs_at: RefCell::default(),
        };
        follow_to_end(&chain);
        let at = chain.logs_at.borrow();
        let gaps: Vec<u128> = at.windows(2).map(|w| (w[1] - w[0]).as_millis()).collect();
        assert_eq!(gaps, [250, 500, 1_000, 2_000]);
    }

    #[test]
    fn delivers_every_block_in_order_through_skips_and_failures() {
        let blocks = det::run_simulated(async {
            let chain = FakeChain {
                // The head jumps from 10 to 13: blocks 11 and 12 were never seen as latest.
                heads: RefCell::new(vec![10, 13, 14]),
                failures_left: Cell::new(2),
                topic: B256::repeat_byte(9),
            };
            let config = FollowerConfig {
                poll: Duration::from_millis(500),
                topics: vec![chain.topic],
                start: Start::Latest,
                run_for: Some(Duration::from_secs(5)),
            };
            let (sender, mut receiver) = mpsc::channel(64);
            follow_head(&chain, &config, &sender).await.unwrap();
            drop(sender);
            let mut blocks = Vec::new();
            while let Some(block) = receiver.recv().await {
                blocks.push(block);
            }
            blocks
        });
        let numbers: Vec<u64> = blocks.iter().map(|b| b.number).collect();
        assert_eq!(numbers, [10, 11, 12, 13, 14]);
        let indexes: Vec<u64> = blocks[0].logs.iter().map(|l| l.log_index).collect();
        assert_eq!(indexes, [2, 5]);
    }
}
