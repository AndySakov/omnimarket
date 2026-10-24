//! The intent a trade's signer signs (D42, D59): EIP-712 typed data with the trade's terms.
//! Until the router is deployed (M3) the domain's verifying contract is a placeholder, and every
//! intent is a demo intent (#74).

use alloy_primitives::{Address, B256, Signature, U256, address};
use alloy_sol_types::{Eip712Domain, SolStruct, eip712_domain, sol};
use k256::ecdsa::SigningKey;

sol! {
    /// D59's terms: at most `maxAmountIn` of `tokenIn` for at least `minAmountOut` of
    /// `tokenOut`, before `deadline` (Unix seconds), submitted by one of `submitters` (D58).
    #[derive(Debug, PartialEq, Eq)]
    struct Intent {
        address signer;
        address tokenIn;
        uint256 maxAmountIn;
        address tokenOut;
        uint256 minAmountOut;
        uint256 deadline;
        address[] submitters;
        bytes32 quoteId;
    }
}

/// The placeholder router: no contract lives here, so a demo intent can't execute anywhere.
pub const PLACEHOLDER_ROUTER: Address = address!("00000000000000000000000000000000000d3e70");

/// The executor set's placeholder submitter (D58), until executors exist (M3).
pub const DEMO_EXECUTOR: Address = address!("00000000000000000000000000000000000e0ec0");

pub fn domain(chain_id: u64) -> Eip712Domain {
    eip712_domain! {
        name: "OmniMarket Router",
        version: "0-demo",
        chain_id: chain_id,
        verifying_contract: PLACEHOLDER_ROUTER,
    }
}

/// The hash a signer signs, and the trade's natural key (D71).
pub fn hash(intent: &Intent, chain_id: u64) -> B256 {
    intent.eip712_signing_hash(&domain(chain_id))
}

/// The address whose key made `signature` over `hash`, or `None` if it doesn't recover.
pub fn signer_of(hash: B256, signature: &Signature) -> Option<Address> {
    signature.recover_address_from_prehash(&hash).ok()
}

/// The server's signer for guest accounts' intents (D5). Shadow mode only: its intents use the
/// placeholder domain, so nothing it signs can execute.
pub struct LocalSigner {
    key: SigningKey,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BadKey;

impl LocalSigner {
    pub fn from_bytes(key: &B256) -> Result<Self, BadKey> {
        SigningKey::from_bytes(&key.0.into())
            .map(|key| Self { key })
            .map_err(|_| BadKey)
    }

    pub fn address(&self) -> Address {
        Address::from_public_key(self.key.verifying_key())
    }

    /// ECDSA over `hash`, deterministic (RFC 6979): the same intent always gets the same
    /// signature.
    pub fn sign(&self, hash: B256) -> Signature {
        let (signature, recovery) = self
            .key
            .sign_prehash_recoverable(hash.as_slice())
            .expect("a 32-byte prehash always signs");
        Signature::from_signature_and_parity(signature, recovery.is_y_odd())
    }
}

/// An amount in whole tokens (a decimal string) in base units. `None` for more fractional
/// digits than `decimals`, or anything but digits and one point.
pub fn base_units(text: &str, decimals: u32) -> Option<U256> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty() && fraction.is_empty()
        || fraction.len() > decimals as usize
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction:0<width$}", width = decimals as usize);
    U256::from_str_radix(if digits.is_empty() { "0" } else { &digits }, 10).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent(signer: Address) -> Intent {
        Intent {
            signer,
            tokenIn: Address::repeat_byte(1),
            maxAmountIn: U256::from(5u64),
            tokenOut: Address::repeat_byte(2),
            minAmountOut: U256::from(4u64),
            deadline: U256::from(1_790_000_064u64),
            submitters: vec![DEMO_EXECUTOR],
            quoteId: B256::repeat_byte(7),
        }
    }

    #[test]
    fn a_signature_recovers_to_its_signer_and_no_other() {
        let signer = LocalSigner::from_bytes(&B256::repeat_byte(0x11)).unwrap();
        let hash = hash(&intent(signer.address()), 8453);
        let signature = signer.sign(hash);
        assert_eq!(signer_of(hash, &signature), Some(signer.address()));
        assert_eq!(signer.sign(hash), signature);
        let other = LocalSigner::from_bytes(&B256::repeat_byte(0x22)).unwrap();
        assert_ne!(signer_of(hash, &other.sign(hash)), Some(signer.address()));
    }

    #[test]
    fn the_hash_covers_every_term_and_the_chain() {
        let base = intent(Address::ZERO);
        let h = hash(&base, 8453);
        assert_ne!(hash(&base, 1), h);
        let mut lower = base.clone();
        lower.minAmountOut = U256::from(3u64);
        assert_ne!(hash(&lower, 8453), h);
        let mut later = base;
        later.deadline = U256::from(1_790_000_065u64);
        assert_ne!(hash(&later, 8453), h);
    }

    #[test]
    fn whole_token_amounts_become_base_units() {
        assert_eq!(base_units("1.5", 6), Some(U256::from(1_500_000u64)));
        assert_eq!(base_units("0.0000001", 6), None);
        assert_eq!(base_units("1e3", 6), None);
    }
}
