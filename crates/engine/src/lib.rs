//! The Chain Engine core (D6): one task that follows a chain's canonical blocks (D10, D77).
//! It reaches the outside only through `det`, so every run can be recorded and replayed.

use std::collections::VecDeque;

use det::{Clock, EventSource};
use types::chain::{B256, Block};

/// Base's input log (D72): one partition, keyed by core instance.
pub const INPUT_TOPIC: &str = "inputs.base";

/// First topics of the logs M1 follows: Uniswap v2 and v3 pool events.
pub const M1_TOPICS: [B256; 6] = [
    // PairCreated(address,address,address,uint256)
    B256::new(hex(
        "0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9",
    )),
    // Sync(uint112,uint112)
    B256::new(hex(
        "1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1",
    )),
    // PoolCreated(address,address,uint24,int24,address)
    B256::new(hex(
        "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
    )),
    // Swap(address,address,int256,int256,uint160,uint128,int24)
    B256::new(hex(
        "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
    )),
    // Mint(address,address,int24,int24,uint128,uint256,uint256)
    B256::new(hex(
        "7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",
    )),
    // Burn(address,int24,int24,uint128,uint256,uint256)
    B256::new(hex(
        "0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c",
    )),
];

/// Decodes 64 hex digits at compile time.
const fn hex(digits: &str) -> [u8; 32] {
    let digits = digits.as_bytes();
    assert!(digits.len() == 64, "a topic is 32 bytes");
    let mut out = [0; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = nibble(digits[2 * i]) << 4 | nibble(digits[2 * i + 1]);
        i += 1;
    }
    out
}

const fn nibble(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        _ => panic!("topics are lowercase hex"),
    }
}

/// How many recent block hashes the engine keeps, to find where a reorg forked.
const RECENT_BLOCKS: usize = 128;

#[derive(Debug, PartialEq, Eq)]
pub enum EngineError {
    /// The follower skipped a block. It fills gaps by number, so this is a bug upstream.
    Gap { expected: u64, received: u64 },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gap { expected, received } => {
                write!(f, "block gap: expected {expected}, received {received}")
            }
        }
    }
}

impl std::error::Error for EngineError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub head: Option<(u64, B256)>,
    pub blocks: u64,
    pub logs: u64,
    /// Blocks whose parent wasn't the block before them.
    pub reorgs_detected: u64,
    /// Over every block applied, in order: number and hash.
    pub digest: blake3::Hash,
}

pub struct Engine {
    clock: Box<dyn Clock>,
    blocks: Box<dyn EventSource<Event = Block>>,
    recent: VecDeque<(u64, B256)>,
    applied: u64,
    logs: u64,
    reorgs_detected: u64,
    digest: blake3::Hasher,
}

impl Engine {
    pub fn new(clock: Box<dyn Clock>, blocks: Box<dyn EventSource<Event = Block>>) -> Self {
        Self {
            clock,
            blocks,
            recent: VecDeque::with_capacity(RECENT_BLOCKS),
            applied: 0,
            logs: 0,
            reorgs_detected: 0,
            digest: blake3::Hasher::new(),
        }
    }

    /// Applies blocks until the source ends.
    pub async fn run(mut self) -> Result<Summary, EngineError> {
        while let Some(block) = self.blocks.next().await {
            self.apply(&block)?;
        }
        Ok(Summary {
            head: self.recent.back().copied(),
            blocks: self.applied,
            logs: self.logs,
            reorgs_detected: self.reorgs_detected,
            digest: self.digest.finalize(),
        })
    }

    fn apply(&mut self, block: &Block) -> Result<(), EngineError> {
        let now = self.clock.now();
        if let Some(&(number, hash)) = self.recent.back() {
            if block.number != number + 1 {
                return Err(EngineError::Gap {
                    expected: number + 1,
                    received: block.number,
                });
            }
            if block.parent_hash != hash {
                // Undoing the replaced blocks' state arrives with tiered undo (D12). Until
                // then the engine re-anchors on the new chain.
                self.reorgs_detected += 1;
                tracing::warn!(block.number, %block.parent_hash, held = %hash, "reorg detected");
                self.recent.clear();
            }
        }
        if self.recent.len() == RECENT_BLOCKS {
            self.recent.pop_front();
        }
        self.recent.push_back((block.number, block.hash));
        self.applied += 1;
        self.logs += block.logs.len() as u64;
        self.digest.update(&block.number.to_le_bytes());
        self.digest.update(block.hash.as_slice());

        // Seconds-resolution timestamps make this an upper bound, like the measured delay.
        let lag_ms = (now.unix_nanos / 1_000_000).saturating_sub(block.timestamp * 1_000);
        tracing::info!(block.number, logs = block.logs.len(), lag_ms, "block");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use alloy_primitives::keccak256;
    use det::{
        InMemorySink, Recorder, RecordingClock, RecordingEventSource, Replay, SimClock,
        SimEventSource,
    };
    use types::Timestamp;

    use super::*;

    #[test]
    fn topics_are_the_event_signatures() {
        let signatures = [
            "PairCreated(address,address,address,uint256)",
            "Sync(uint112,uint112)",
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
        ];
        for (topic, signature) in M1_TOPICS.iter().zip(signatures) {
            assert_eq!(*topic, keccak256(signature), "{signature}");
        }
    }

    fn hash(tag: u8, number: u64) -> B256 {
        let mut bytes = [tag; 32];
        bytes[24..].copy_from_slice(&number.to_be_bytes());
        B256::new(bytes)
    }

    /// Blocks 100..=104 on chain `a`, then 105 whose parent is 104 on chain `b`: a reorg.
    fn chain_with_a_reorg() -> Vec<(Duration, Block)> {
        (100..=105)
            .map(|n| {
                let parent_tag = if n == 105 { b'b' } else { b'a' };
                let block = Block {
                    number: n,
                    hash: hash(b'a', n),
                    parent_hash: hash(parent_tag, n - 1),
                    timestamp: 1_767_225_600 + n * 2,
                    logs: Vec::new(),
                };
                (Duration::from_secs(2 * (n - 99)), block)
            })
            .collect()
    }

    const START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);

    #[test]
    fn follows_blocks_and_detects_a_reorg() {
        let summary = det::run_simulated(async {
            let engine = Engine::new(
                Box::new(SimClock::starting_at(START)),
                Box::new(SimEventSource::new(chain_with_a_reorg())),
            );
            engine.run().await.unwrap()
        });
        assert_eq!(summary.blocks, 6);
        assert_eq!(summary.reorgs_detected, 1);
        assert_eq!(summary.head, Some((105, hash(b'a', 105))));
    }

    #[test]
    fn a_gap_stops_the_engine() {
        let mut blocks = chain_with_a_reorg();
        blocks.remove(2);
        let result = det::run_simulated(async {
            let engine = Engine::new(
                Box::new(SimClock::starting_at(START)),
                Box::new(SimEventSource::new(blocks)),
            );
            engine.run().await
        });
        assert_eq!(
            result,
            Err(EngineError::Gap {
                expected: 102,
                received: 103
            })
        );
    }

    #[test]
    fn a_recorded_run_replays_to_the_same_summary() {
        let sink = InMemorySink::default();
        let live = det::run_simulated(async {
            let clock = SimClock::starting_at(START);
            let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock.clone()));
            let engine = Engine::new(
                Box::new(RecordingClock::new(Box::new(clock), recorder.clone())),
                Box::new(RecordingEventSource::new(
                    Box::new(SimEventSource::new(chain_with_a_reorg())),
                    recorder,
                )),
            );
            engine.run().await.unwrap()
        });
        let replay = Replay::new(sink.records());
        let engine = Engine::new(Box::new(replay.clock()), Box::new(replay.events()));
        assert_eq!(replay.run(engine.run()).unwrap(), live);
    }
}
