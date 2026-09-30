//! The Chain Engine core (D6): one task that follows a chain's canonical blocks (D10, D77)
//! and keeps pool state from their logs. It reaches the outside only through `det`, so every
//! run can be recorded and replayed.

mod core;
mod outbox;
mod state;
mod v2;
mod v3;

use types::chain::B256;

pub use crate::core::Engine;
pub use crate::outbox::{InMemoryOutbox, KafkaOutbox, Outbox, PoolState, PoolUpdate, V3State};
pub use crate::state::{EngineConfig, EngineError, Stats, Summary};

/// Base's input log (D72): one partition, keyed by core instance.
pub const INPUT_TOPIC: &str = "inputs.base";

/// Base's pool updates, before and after (D12, D41), keyed by pool.
pub const POOL_UPDATES_TOPIC: &str = "pool-updates.base";

/// First topics of the logs M1 follows: Uniswap v2 and v3 pool events.
pub const M1_TOPICS: [B256; 7] = [
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
    // Initialize(uint160,int24)
    B256::new(hex(
        "98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95",
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

#[cfg(test)]
mod tests {
    use alloy_primitives::keccak256;

    use super::*;

    #[test]
    fn topics_are_the_event_signatures() {
        let signatures = [
            "PairCreated(address,address,address,uint256)",
            "Sync(uint112,uint112)",
            "PoolCreated(address,address,uint24,int24,address)",
            "Initialize(uint160,int24)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
        ];
        for (topic, signature) in M1_TOPICS.iter().zip(signatures) {
            assert_eq!(*topic, keccak256(signature), "{signature}");
        }
    }
}
