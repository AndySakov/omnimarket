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
    use alloy_primitives::{Bytes, U256, address};
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
