//! Uniswap v2 pairs and forks that share its bytecode.

use alloy_primitives::{Address, B256, keccak256};
use alloy_sol_types::{SolCall, SolEvent, sol};
use types::chain::Log;

sol! {
    event PairCreated(address indexed token0, address indexed token1, address pair, uint256);
    event Sync(uint112 reserve0, uint112 reserve1);
    function token0() external view returns (address);
    function token1() external view returns (address);
    function getReserves() external view returns (uint112 reserve0, uint112 reserve1, uint32 blockTimestampLast);
}

/// One deployment of v2: a factory and the init code hash its pairs are created with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deployment {
    pub factory: Address,
    pub init_code_hash: B256,
}

/// Uniswap v2 on Base (D80: checked against a live pair in the tests).
pub const BASE: Deployment = Deployment {
    factory: alloy_primitives::address!("8909Dc15e40173Ff4699343b6eB8132c65e18eC6"),
    init_code_hash: alloy_primitives::b256!(
        "96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f"
    ),
};

impl Deployment {
    /// The address the factory creates the pair of two tokens at (CREATE2). A pair whose
    /// address doesn't match isn't this deployment's, whatever it claims.
    pub fn pair_address(&self, token0: Address, token1: Address) -> Address {
        let salt = keccak256([token0.as_slice(), token1.as_slice()].concat());
        self.factory.create2(salt, self.init_code_hash)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reserves {
    pub reserve0: u128,
    pub reserve1: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub address: Address,
    pub token0: Address,
    pub token1: Address,
    pub reserves: Reserves,
}

/// A new pair, from its factory's `PairCreated`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Created {
    pub pair: Address,
    pub token0: Address,
    pub token1: Address,
}

pub fn is_sync(log: &Log) -> bool {
    log.topics.first() == Some(&Sync::SIGNATURE_HASH)
}

pub fn is_pair_created(log: &Log) -> bool {
    log.topics.first() == Some(&PairCreated::SIGNATURE_HASH)
}

/// `Sync` carries the pair's full reserves after every change, so it alone sets the state.
pub fn decode_sync(log: &Log) -> Option<Reserves> {
    let sync = Sync::decode_raw_log(log.topics.iter().copied(), &log.data).ok()?;
    Some(Reserves {
        reserve0: sync.reserve0.to(),
        reserve1: sync.reserve1.to(),
    })
}

pub fn decode_pair_created(log: &Log) -> Option<Created> {
    let created = PairCreated::decode_raw_log(log.topics.iter().copied(), &log.data).ok()?;
    Some(Created {
        pair: created.pair,
        token0: created.token0,
        token1: created.token1,
    })
}

pub fn token0_call() -> Vec<u8> {
    token0Call {}.abi_encode()
}

pub fn token1_call() -> Vec<u8> {
    token1Call {}.abi_encode()
}

pub fn get_reserves_call() -> Vec<u8> {
    getReservesCall {}.abi_encode()
}

pub fn decode_address(returned: &[u8]) -> Option<Address> {
    token0Call::abi_decode_returns(returned).ok()
}

/// A `token0()` / `token1()` answer, for simulated nodes and tests.
pub fn encode_address(address: Address) -> Vec<u8> {
    token0Call::abi_encode_returns(&address)
}

/// A `getReserves()` answer, for simulated nodes and tests.
pub fn encode_reserves(reserves: Reserves) -> Vec<u8> {
    getReservesCall::abi_encode_returns(&getReservesReturn {
        reserve0: alloy_primitives::aliases::U112::from(reserves.reserve0),
        reserve1: alloy_primitives::aliases::U112::from(reserves.reserve1),
        blockTimestampLast: 0,
    })
}

pub fn decode_reserves(returned: &[u8]) -> Option<Reserves> {
    let reserves = getReservesCall::abi_decode_returns(returned).ok()?;
    Some(Reserves {
        reserve0: reserves.reserve0.to(),
        reserve1: reserves.reserve1.to(),
    })
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Bytes, U256, address, b256, hex};
    use alloy_sol_types::SolValue;

    use super::*;

    const WETH: Address = address!("4200000000000000000000000000000000000006");
    const USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");

    // Read from the factory's getPair(WETH, USDC) on Base, 2026-09-30.
    #[test]
    fn computes_the_live_weth_usdc_pair() {
        assert_eq!(
            BASE.pair_address(WETH, USDC),
            address!("88a43bbdf9d098eec7bceda4e2494615dfd9bb9c")
        );
    }

    fn log(topics: Vec<B256>, data: Vec<u8>) -> Log {
        Log {
            address: Address::ZERO,
            topics,
            data: Bytes::from(data),
            log_index: 0,
            transaction_hash: B256::ZERO,
        }
    }

    #[test]
    fn decodes_sync() {
        let data = (U256::from(5u64), U256::from(7u64)).abi_encode();
        let sync = log(vec![Sync::SIGNATURE_HASH], data);
        assert!(is_sync(&sync));
        assert_eq!(
            decode_sync(&sync),
            Some(Reserves {
                reserve0: 5,
                reserve1: 7
            })
        );
    }

    // The logs below are real, from Base, read with eth_getLogs on 2026-10-01. Unlike the tests
    // above, they don't come from this crate's own encoders, so they also check the event
    // signatures and the layout against the deployed contracts.

    /// The WETH/USDC pair's `Sync` in block 52,035,610 (log 930).
    fn base_sync() -> Log {
        Log {
            address: address!("88a43bbdf9d098eec7bceda4e2494615dfd9bb9c"),
            topics: vec![b256!(
                "1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1"
            )],
            data: Bytes::from_static(&hex!(
                "00000000000000000000000000000000000000000000000d740ccf1a3f94faf1"
                "0000000000000000000000000000000000000000000000000000009b817f992d"
            )),
            log_index: 930,
            transaction_hash: b256!(
                "fb1b28b2eb7b1aa9fce2052983e17143434844098d0c019888f601f6fe0c9632"
            ),
        }
    }

    /// The factory's `PairCreated` for the WETH/USDC pair, its first pair, in block 10,526,493
    /// (log 469).
    fn base_pair_created() -> Log {
        Log {
            address: BASE.factory,
            topics: vec![
                b256!("0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9"),
                b256!("0000000000000000000000004200000000000000000000000000000000000006"),
                b256!("000000000000000000000000833589fcd6edb6e08f4c7c32d4f71b54bda02913"),
            ],
            data: Bytes::from_static(&hex!(
                "00000000000000000000000088a43bbdf9d098eec7bceda4e2494615dfd9bb9c"
                "0000000000000000000000000000000000000000000000000000000000000001"
            )),
            log_index: 469,
            transaction_hash: b256!(
                "e537cba808a2646f59c4993e79e798231653333cf08d43b69d95b57171bb3548"
            ),
        }
    }

    #[test]
    fn a_sync_from_base_is_a_sync_and_carries_the_pairs_reserves() {
        let sync = base_sync();
        assert!(is_sync(&sync));
        assert!(!is_pair_created(&sync));
        let reserves = Reserves {
            reserve0: 248_169_959_277_987_166_961,
            reserve1: 667_892_554_029,
        };
        assert_eq!(decode_sync(&sync), Some(reserves));
        // getReserves() on the pair answered the same reserves at the time.
        let answer = hex!(
            "00000000000000000000000000000000000000000000000d740ccf1a3f94faf1"
            "0000000000000000000000000000000000000000000000000000009b817f992d"
            "000000000000000000000000000000000000000000000000000000006abe5d17"
        );
        assert_eq!(decode_reserves(&answer), Some(reserves));
    }

    #[test]
    fn a_pair_created_from_base_is_a_creation_and_not_a_sync() {
        let created = base_pair_created();
        assert!(is_pair_created(&created));
        assert!(!is_sync(&created));
        assert_eq!(
            decode_pair_created(&created),
            Some(Created {
                pair: address!("88a43bbdf9d098eec7bceda4e2494615dfd9bb9c"),
                token0: WETH,
                token1: USDC
            })
        );
    }

    // Each selector is the first four bytes of the keccak-256 of the function's signature,
    // computed outside this crate. The live pair answered them with WETH, USDC and its
    // reserves on 2026-10-01.
    #[test]
    fn calls_use_the_pairs_abi_selectors() {
        assert_eq!(token0_call(), hex!("0dfe1681"));
        assert_eq!(token1_call(), hex!("d21220a7"));
        assert_eq!(get_reserves_call(), hex!("0902f1ac"));
    }

    #[test]
    fn decodes_pair_created() {
        let pair = address!("88a43bbdf9d098eec7bceda4e2494615dfd9bb9c");
        let created = log(
            vec![
                PairCreated::SIGNATURE_HASH,
                WETH.into_word(),
                USDC.into_word(),
            ],
            (pair, U256::from(1u64)).abi_encode(),
        );
        assert!(is_pair_created(&created));
        assert_eq!(
            decode_pair_created(&created),
            Some(Created {
                pair,
                token0: WETH,
                token1: USDC
            })
        );
    }
}
