//! Trade history backfill (#80, D15 thin): when a pool becomes tracked, the engine reads its
//! Swaps over the window before it was first seen with ranged log reads, and publishes them as
//! trade records on `trade-history.<chain>`, so a chart opens with hours of candles. The reads
//! are recorded inputs (`det::Source::Logs`), so a replay publishes the same history.

use std::collections::{BTreeMap, VecDeque};

use types::chain::{Address, B256, BlockLog, LogsQuery, LogsResult};

use crate::state::{Effects, Stats};
use crate::trades::{SwapSeen, Trade, Venue};

/// v2's and v3's `Swap`.
const SWAP_TOPICS: [B256; 2] = [crate::M1_TOPICS[2], crate::M1_TOPICS[5]];

/// Pools read together: one read's addresses.
const BATCH_POOLS: usize = 50;

/// Blocks per read at first. Base's public endpoint takes at most 2,000 (D80); a read refused as
/// too large is split in two.
const FIRST_SPAN: u64 = 1_000;

/// A pool whose history is wanted: its Swaps in `from..=to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Wanted {
    venue: Venue,
    tokens: (Address, Address),
    from: u64,
    to: u64,
}

/// Pools being read together, and the block ranges still to read, newest first.
struct Batch {
    pools: BTreeMap<Address, Wanted>,
    ranges: VecDeque<(u64, u64)>,
}

/// A log read the engine is waiting on.
pub(crate) struct Read {
    from: u64,
    to: u64,
}

pub(crate) struct History {
    chain_id: u64,
    quote_assets: Vec<Address>,
    /// How many blocks before a pool was first seen to read.
    window: u64,
    queue: VecDeque<(Address, Wanted)>,
    batch: Option<Batch>,
    reading: bool,
}

impl History {
    pub fn new(chain_id: u64, quote_assets: Vec<Address>, window: u64) -> Self {
        Self {
            chain_id,
            quote_assets,
            window,
            queue: VecDeque::new(),
            batch: None,
            reading: false,
        }
    }

    /// A pool became tracked at `first_seen`: its trades from that block on are published live,
    /// so its history is the window before it.
    pub fn want(
        &mut self,
        pool: Address,
        venue: Venue,
        tokens: (Address, Address),
        first_seen: u64,
        stats: &mut Stats,
    ) {
        let Some(to) = first_seen.checked_sub(1) else {
            return;
        };
        let from = first_seen.saturating_sub(self.window);
        self.queue.push_back((
            pool,
            Wanted {
                venue,
                tokens,
                from,
                to,
            },
        ));
        stats.history_pools += 1;
    }

    /// Starts the next read, if none is in flight and any is left.
    pub fn next(&mut self, effects: &mut Effects) {
        if self.reading {
            return;
        }
        if self.batch.as_ref().is_none_or(|b| b.ranges.is_empty()) {
            self.batch = self.next_batch();
        }
        let Some(batch) = &mut self.batch else {
            return;
        };
        let Some((from, to)) = batch.ranges.pop_front() else {
            return;
        };
        self.reading = true;
        effects.log_reads.push((
            Read { from, to },
            LogsQuery {
                addresses: batch.pools.keys().copied().collect(),
                topics: SWAP_TOPICS.to_vec(),
                from_block: from,
                to_block: to,
            },
        ));
    }

    fn next_batch(&mut self) -> Option<Batch> {
        let take = self.queue.len().min(BATCH_POOLS);
        if take == 0 {
            return None;
        }
        let pools: BTreeMap<Address, Wanted> = self.queue.drain(..take).collect();
        let from = pools.values().map(|w| w.from).min()?;
        let to = pools.values().map(|w| w.to).max()?;
        // Newest first, so the most recent candles fill first.
        let mut ranges = VecDeque::new();
        let mut end = to;
        loop {
            let start = end.saturating_sub(FIRST_SPAN - 1).max(from);
            ranges.push_back((start, end));
            if start == from {
                break;
            }
            end = start - 1;
        }
        Some(Batch { pools, ranges })
    }

    pub fn on_answer(
        &mut self,
        read: Read,
        result: LogsResult,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        self.reading = false;
        stats.history_reads += 1;
        const IN_FLIGHT: &str = "a read in flight belongs to the current batch";
        match result {
            LogsResult::Returned(logs) => {
                let batch = self.batch.as_ref().expect(IN_FLIGHT);
                for log in &logs {
                    if let Some(trade) = self.trade(batch, log) {
                        effects.history.push(trade);
                    }
                }
            }
            LogsResult::Failed(error) => {
                stats.history_read_failures += 1;
                let batch = self.batch.as_mut().expect(IN_FLIGHT);
                if read.from < read.to {
                    // Split in two, newer half first, and read both before going further back.
                    let middle = read.from + (read.to - read.from) / 2;
                    batch.ranges.push_front((read.from, middle));
                    batch.ranges.push_front((middle + 1, read.to));
                } else {
                    tracing::warn!(block = read.from, %error, "a one-block history read failed");
                    stats.history_blocks_dropped += 1;
                }
            }
        }
    }

    /// The trade a backfilled log records, if it's a Swap of a pool in the batch within the
    /// range wanted for it.
    fn trade(&self, batch: &Batch, read: &BlockLog) -> Option<Trade> {
        let wanted = batch.pools.get(&read.log.address)?;
        if !(wanted.from..=wanted.to).contains(&read.block_number) {
            return None;
        }
        let (sender, recipient, deltas) = match wanted.venue {
            Venue::UniswapV2 => {
                let swap = venues::v2::decode_swap(&read.log)?;
                (swap.sender, swap.recipient, swap.pool_deltas()?)
            }
            Venue::UniswapV3 => {
                let swap = venues::v3::decode_swap(&read.log)?;
                (swap.sender, swap.recipient, (swap.amount0, swap.amount1))
            }
        };
        let seen = SwapSeen {
            block_number: read.block_number,
            block_hash: read.block_hash,
            block_timestamp: read.block_timestamp,
            log_index: read.log.log_index,
            tx_hash: read.log.transaction_hash,
            sender,
            recipient,
            amount0: deltas.0,
            amount1: deltas.1,
        };
        Some(Trade::new(
            self.chain_id,
            wanted.venue,
            read.log.address,
            wanted.tokens,
            &self.quote_assets,
            &seen,
        ))
    }
}
