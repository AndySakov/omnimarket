//! D29's first layer (#89): a round trip against a token's live pool, in one `eth_call`. The
//! safety probe (`contracts/src/SafetyProbe.sol`) is put at a throwaway address with some ETH by a
//! state override, buys the token with WETH, sells it all back, and reports what each leg lost.

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolCall, sol};

sol! {
    struct ProbeResult {
        bool bought;
        bool sold;
        uint256 expectedTokens;
        uint256 receivedTokens;
        uint256 tokensSold;
        uint256 poolReceived;
        uint256 returned;
        string reason;
    }
    function probe(address weth, address token, address pool, bool v3, uint256 amountIn) external returns (ProbeResult r);
}

/// The probe's runtime code, as `forge inspect SafetyProbe deployedBytecode` prints it;
/// `scripts/verify.sh` checks it still matches the source.
const CODE_HEX: &str = include_str!("safety_probe.hex");

/// The ETH the probe holds for the call: enough for any round trip it makes.
pub fn probe_balance() -> U256 {
    U256::from(10u64).pow(U256::from(18u64))
}

pub fn probe_code() -> Bytes {
    CODE_HEX
        .trim()
        .parse()
        .expect("the committed probe code is hex")
}

/// The call that runs a round trip of `amount_in` WETH through `pool`.
pub fn calldata(weth: Address, token: Address, pool: Address, v3: bool, amount_in: U256) -> Bytes {
    probeCall {
        weth,
        token,
        pool,
        v3,
        amountIn: amount_in,
    }
    .abi_encode()
    .into()
}

/// What a round trip showed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundTrip {
    pub bought: bool,
    pub sold: bool,
    pub spent: U256,
    pub expected_tokens: U256,
    pub received_tokens: U256,
    pub tokens_sold: U256,
    pub pool_received: U256,
    pub returned: U256,
    pub reason: String,
}

/// D29's verdict on a round trip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Passed,
    /// A tax of 1% or more, or a buy that failed: warn, never block.
    Warning,
    /// The sell reverted or came back as dust: buys are blocked.
    Honeypot,
}

/// A tax or fee as parts per million, the share of `expected` that didn't arrive.
pub fn loss_ppm(got: U256, expected: U256) -> u64 {
    if expected.is_zero() || got >= expected {
        return 0;
    }
    ((expected - got) * U256::from(1_000_000u64) / expected).saturating_to()
}

impl RoundTrip {
    /// The probe's answer for a round trip that spent `spent`. `None` if it doesn't decode.
    pub fn decode(returned: &[u8], spent: U256) -> Option<Self> {
        let r = probeCall::abi_decode_returns(returned).ok()?;
        Some(Self {
            bought: r.bought,
            sold: r.sold,
            spent,
            expected_tokens: r.expectedTokens,
            received_tokens: r.receivedTokens,
            tokens_sold: r.tokensSold,
            pool_received: r.poolReceived,
            returned: r.returned,
            reason: r.reason,
        })
    }

    pub fn buy_tax_ppm(&self) -> u64 {
        loss_ppm(self.received_tokens, self.expected_tokens)
    }

    pub fn sell_tax_ppm(&self) -> u64 {
        loss_ppm(self.pool_received, self.tokens_sold)
    }

    /// Honeypot when the sell reverts or returns under half of what was spent (dust, for the
    /// small amounts the probe trades); a warning for a failed buy or a 1% tax either way.
    pub fn verdict(&self) -> Verdict {
        if self.bought && (!self.sold || self.returned * U256::from(2u64) < self.spent) {
            return Verdict::Honeypot;
        }
        if !self.bought || self.buy_tax_ppm() >= 10_000 || self.sell_tax_ppm() >= 10_000 {
            return Verdict::Warning;
        }
        Verdict::Passed
    }
}

#[cfg(test)]
mod tests {
    use alloy_sol_types::SolValue;

    use super::*;

    fn trip(sold: bool, received: u64, pool_received: u64, returned: u64) -> RoundTrip {
        RoundTrip {
            bought: true,
            sold,
            spent: U256::from(1_000u64),
            expected_tokens: U256::from(1_000_000u64),
            received_tokens: U256::from(received),
            tokens_sold: U256::from(received),
            pool_received: U256::from(pool_received),
            returned: U256::from(returned),
            reason: String::new(),
        }
    }

    #[test]
    fn a_verdict_follows_the_sell_and_the_taxes() {
        assert_eq!(
            trip(true, 1_000_000, 1_000_000, 994).verdict(),
            Verdict::Passed
        );
        // 5% taxed on the way in: a warning, not a block.
        assert_eq!(
            trip(true, 950_000, 950_000, 890).verdict(),
            Verdict::Warning
        );
        assert_eq!(trip(true, 950_000, 950_000, 890).buy_tax_ppm(), 50_000);
        // Taxed on the way out only.
        assert_eq!(trip(true, 1_000_000, 989_000, 980).sell_tax_ppm(), 11_000);
        assert_eq!(
            trip(true, 1_000_000, 989_000, 980).verdict(),
            Verdict::Warning
        );
        // The sell reverted, or came back as dust.
        assert_eq!(trip(false, 1_000_000, 0, 0).verdict(), Verdict::Honeypot);
        assert_eq!(
            trip(true, 1_000_000, 1_000_000, 499).verdict(),
            Verdict::Honeypot
        );
        assert_eq!(
            trip(true, 1_000_000, 1_000_000, 500).verdict(),
            Verdict::Passed
        );
        // Just under 1% passes.
        assert_eq!(trip(true, 990_001, 990_001, 900).verdict(), Verdict::Passed);
        let unbought = RoundTrip {
            bought: false,
            ..trip(false, 0, 0, 0)
        };
        assert_eq!(unbought.verdict(), Verdict::Warning);
    }

    #[test]
    fn the_probes_answer_decodes() {
        let answer = (ProbeResult {
            bought: true,
            sold: false,
            expectedTokens: U256::from(7u64),
            receivedTokens: U256::from(6u64),
            tokensSold: U256::from(6u64),
            poolReceived: U256::ZERO,
            returned: U256::ZERO,
            reason: "sells are disabled".into(),
        },)
            .abi_encode_params();
        let trip = RoundTrip::decode(&answer, U256::from(1u64)).unwrap();
        assert_eq!(
            (
                trip.bought,
                trip.sold,
                trip.reason.as_str(),
                trip.received_tokens
            ),
            (true, false, "sells are disabled", U256::from(6u64))
        );
        assert_eq!(RoundTrip::decode(&[1, 2, 3], U256::ZERO), None);
        assert!(probe_code().len() > 1_000);
    }
}
