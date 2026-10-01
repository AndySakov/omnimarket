//! ERC-20 metadata: `name`, `symbol`, `decimals` and `totalSupply`, read in one multicall per
//! batch of tokens, with fallbacks for tokens that don't follow the standard.

use alloy_primitives::{Address, U256};
use alloy_sol_types::{SolCall, SolValue, sol};

sol! {
    function name() external view returns (string);
    function symbol() external view returns (string);
    function decimals() external view returns (uint8);
    function totalSupply() external view returns (uint256);
}

/// What a token told us about itself. A field is `None` when its call reverted or returned
/// something that isn't the standard's answer; the token is still tracked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenMetadata {
    pub name: Option<String>,
    pub symbol: Option<String>,
    /// Without decimals a token can't be scaled, so it isn't priced.
    pub decimals: Option<u8>,
    /// Raw units. Re-read periodically: mints and burns change it (D20).
    pub total_supply: Option<U256>,
}

/// The four reads, in the order `decode` expects their answers.
pub fn calls(token: Address) -> [(Address, Vec<u8>); 4] {
    [
        (token, nameCall {}.abi_encode()),
        (token, symbolCall {}.abi_encode()),
        (token, decimalsCall {}.abi_encode()),
        (token, totalSupplyCall {}.abi_encode()),
    ]
}

pub fn total_supply_call(token: Address) -> (Address, Vec<u8>) {
    (token, totalSupplyCall {}.abi_encode())
}

/// The answers to `calls`, `None` where a call reverted.
pub fn decode(answers: &[Option<&[u8]>; 4]) -> TokenMetadata {
    TokenMetadata {
        name: answers[0].and_then(decode_text),
        symbol: answers[1].and_then(decode_text),
        decimals: answers[2].and_then(decode_decimals),
        total_supply: answers[3].and_then(decode_total_supply),
    }
}

/// An ABI string, or a `bytes32` (early tokens such as MKR). Invalid UTF-8 is replaced and
/// control characters, zero padding included, are dropped; empty text is `None`.
pub fn decode_text(returned: &[u8]) -> Option<String> {
    let text = match String::abi_decode(returned) {
        Ok(text) => text,
        // Its zero padding goes with the control characters below.
        Err(_) if returned.len() == 32 => String::from_utf8_lossy(returned).into_owned(),
        Err(_) => return None,
    };
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// A `uint8`. Some tokens return a wider integer; anything above 77 (10^77 is the largest
/// power of ten a uint256 holds) isn't a usable scale.
pub fn decode_decimals(returned: &[u8]) -> Option<u8> {
    let value = U256::abi_decode(returned).ok()?;
    u8::try_from(value).ok().filter(|&d| d <= 77)
}

pub fn decode_total_supply(returned: &[u8]) -> Option<U256> {
    U256::abi_decode(returned).ok()
}

/// ABI answers, for tests that play a token.
pub mod encode {
    use alloy_primitives::{B256, U256};
    use alloy_sol_types::SolValue;

    pub fn text(text: &str) -> Vec<u8> {
        text.to_owned().abi_encode()
    }

    pub fn bytes32(text: &str) -> Vec<u8> {
        B256::right_padding_from(text.as_bytes()).to_vec()
    }

    pub fn uint(value: U256) -> Vec<u8> {
        value.abi_encode()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_standard_answers() {
        let name = encode::text("Wrapped Ether");
        let symbol = encode::text("WETH");
        let decimals = encode::uint(U256::from(18));
        let supply = encode::uint(U256::from(5));
        let metadata = decode(&[Some(&name), Some(&symbol), Some(&decimals), Some(&supply)]);
        assert_eq!(
            metadata,
            TokenMetadata {
                name: Some("Wrapped Ether".into()),
                symbol: Some("WETH".into()),
                decimals: Some(18),
                total_supply: Some(U256::from(5)),
            }
        );
    }

    #[test]
    fn reads_bytes32_names() {
        assert_eq!(decode_text(&encode::bytes32("Maker")), Some("Maker".into()));
        assert_eq!(decode_text(&encode::bytes32("MKR")), Some("MKR".into()));
    }

    #[test]
    fn falls_back_to_none_when_a_token_reverts_or_answers_nonsense() {
        let metadata = decode(&[None, Some(&[1, 2, 3]), None, Some(&[])]);
        assert_eq!(metadata, TokenMetadata::default());
        assert_eq!(decode_text(&encode::text("")), None);
        assert_eq!(decode_text(&encode::bytes32("")), None);
        // Readable bytes that are neither an ABI string nor a bytes32.
        assert_eq!(decode_text(b"thirty-three bytes of plain text!"), None);
        assert_eq!(decode_decimals(&encode::uint(U256::from(78))), None);
        assert_eq!(decode_decimals(&encode::uint(U256::from(256))), None);
        assert_eq!(decode_decimals(&encode::uint(U256::from(77))), Some(77));
    }

    #[test]
    fn strips_control_characters_and_padding() {
        assert_eq!(
            decode_text(&encode::text(" \u{0}PEPE\n ")),
            Some("PEPE".into())
        );
    }
}
