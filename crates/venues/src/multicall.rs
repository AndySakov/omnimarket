//! Multicall3 (D13): many view calls in one `eth_call`, each allowed to fail on its own.

use alloy_primitives::{Address, Bytes};
use alloy_sol_types::{SolCall, sol};

sol! {
    struct Call3 {
        address target;
        bool allowFailure;
        bytes callData;
    }
    struct Result3 {
        bool success;
        bytes returnData;
    }
    function aggregate3(Call3[] calls) external payable returns (Result3[] returnData);
}

/// Deployed at the same address on every chain we support.
pub const ADDRESS: Address = alloy_primitives::address!("cA11bde05977b3631167028862bE2a173976CA11");

/// Encodes `calls` as one `aggregate3` call to `ADDRESS`.
pub fn encode(calls: &[(Address, Vec<u8>)]) -> Vec<u8> {
    let calls = calls
        .iter()
        .map(|(target, data)| Call3 {
            target: *target,
            allowFailure: true,
            callData: Bytes::from(data.clone()),
        })
        .collect();
    aggregate3Call { calls }.abi_encode()
}

/// Each call's return data, or `None` where that call reverted. `None` overall if the
/// response isn't an `aggregate3` result.
pub fn decode(returned: &[u8]) -> Option<Vec<Option<Bytes>>> {
    let results = aggregate3Call::abi_decode_returns(returned).ok()?;
    Some(
        results
            .into_iter()
            .map(|r| r.success.then_some(r.returnData))
            .collect(),
    )
}

/// The calls inside an `aggregate3` request: what a simulated node needs to answer one.
pub fn decode_calls(data: &[u8]) -> Option<Vec<(Address, Bytes)>> {
    let request = aggregate3Call::abi_decode(data).ok()?;
    Some(
        request
            .calls
            .into_iter()
            .map(|call| (call.target, call.callData))
            .collect(),
    )
}

/// An `aggregate3` response, as a node returns it: for simulated nodes and tests.
pub fn encode_results(results: &[Option<Vec<u8>>]) -> Vec<u8> {
    let results: Vec<Result3> = results
        .iter()
        .map(|r| Result3 {
            success: r.is_some(),
            returnData: Bytes::from(r.clone().unwrap_or_default()),
        })
        .collect();
    aggregate3Call::abi_encode_returns(&results)
}

#[cfg(test)]
mod tests {
    use alloy_sol_types::SolValue;

    use super::*;

    #[test]
    fn requests_and_results_round_trip() {
        let calls = vec![(ADDRESS, vec![1, 2]), (Address::ZERO, vec![])];
        let decoded = decode_calls(&encode(&calls)).unwrap();
        assert_eq!(decoded[0], (ADDRESS, Bytes::from(vec![1, 2])));
        let results = vec![Some(vec![9]), None];
        assert_eq!(
            decode(&encode_results(&results)),
            Some(vec![Some(Bytes::from(vec![9])), None])
        );
    }

    #[test]
    fn decodes_mixed_results() {
        let results = vec![
            Result3 {
                success: true,
                returnData: Bytes::from(vec![1, 2]),
            },
            Result3 {
                success: false,
                returnData: Bytes::new(),
            },
        ];
        let returned = results.abi_encode();
        assert_eq!(
            decode(&returned),
            Some(vec![Some(Bytes::from(vec![1, 2])), None])
        );
    }
}
