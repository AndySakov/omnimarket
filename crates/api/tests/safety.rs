//! Token safety (#89): which tokens get checked and when, and how a round trip reads on the
//! contract.

use alloy_primitives::U256;
use api::{RECHECK_MS, Safety, WATCH_MS};
use execution::safety::RoundTrip;
use proto::api::v1::{SafetyVerdict, TokenRef};
use types::chain::Address;

const T: Address = Address::repeat_byte(0x30);
const U: Address = Address::repeat_byte(0x31);

fn weth() -> TokenRef {
    TokenRef {
        chain_id: 8453,
        address: "0x4200000000000000000000000000000000000006".into(),
        symbol: "WETH".into(),
        decimals: 18,
        ..TokenRef::default()
    }
}

fn trip(sold: bool, received: u64, pool_received: u64, returned: u64) -> RoundTrip {
    RoundTrip {
        bought: true,
        sold,
        spent: U256::from(10u64).pow(U256::from(16u64)),
        expected_tokens: U256::from(1_000_000u64),
        received_tokens: U256::from(received),
        tokens_sold: U256::from(received),
        pool_received: U256::from(pool_received),
        returned: U256::from(returned) * U256::from(10u64).pow(U256::from(13u64)),
        reason: if sold {
            String::new()
        } else {
            "sells are disabled".into()
        },
    }
}

fn record(safety: &mut Safety, token: Address, at_ms: u64) {
    let report = api::safety_report(
        token,
        &trip(true, 1_000_000, 1_000_000, 994),
        Address::ZERO,
        weth(),
        18,
        1,
        at_ms,
    );
    safety.record(token, report);
}

#[test]
fn a_watched_token_is_checked_again_once_its_report_is_stale() {
    let mut safety = Safety::default();
    safety.watch(T, 0);
    // Reading the page twice queues it once.
    safety.watch(T, 1);
    assert_eq!((safety.next_due(), safety.next_due()), (Some(T), None));
    record(&mut safety, T, 10);
    // Fresh: a sweep queues nothing; stale: it's queued again.
    safety.sweep(10 + RECHECK_MS - 1);
    assert_eq!(safety.next_due(), None);
    safety.sweep(10 + RECHECK_MS);
    assert_eq!(safety.next_due(), Some(T));
    // Unread for WATCH_MS, it's no longer watched.
    record(&mut safety, T, 20);
    safety.sweep(1 + WATCH_MS + RECHECK_MS);
    assert_eq!(safety.next_due(), None);
}

#[test]
fn a_discovered_pool_checks_its_token_once() {
    let mut safety = Safety::default();
    safety.discovered(U);
    safety.discovered(U);
    assert_eq!((safety.next_due(), safety.next_due()), (Some(U), None));
    record(&mut safety, U, 5);
    safety.discovered(U);
    assert_eq!(safety.next_due(), None);
}

#[test]
fn a_round_trip_reads_as_its_verdict_and_evidence() {
    let passed = api::safety_report(
        T,
        &trip(true, 1_000_000, 1_000_000, 994),
        Address::ZERO,
        weth(),
        18,
        7,
        9,
    );
    assert_eq!(passed.verdict(), SafetyVerdict::Passed);
    let evidence = passed.round_trip.unwrap();
    assert_eq!(
        (
            evidence.amount_spent.as_str(),
            evidence.amount_returned.as_str(),
            evidence.buy_tax_pct.as_str(),
            evidence.sell_tax_pct.as_str()
        ),
        ("0.01", "0.00994", "0", "0")
    );
    assert_eq!((passed.checked_block, passed.checked_at_ms), (7, 9));
    let taxed = api::safety_report(
        T,
        &trip(true, 950_000, 902_500, 890),
        Address::ZERO,
        weth(),
        18,
        7,
        9,
    );
    assert_eq!(taxed.verdict(), SafetyVerdict::Warning);
    let evidence = taxed.round_trip.unwrap();
    assert_eq!(
        (
            evidence.buy_tax_pct.as_str(),
            evidence.sell_tax_pct.as_str()
        ),
        ("5", "5")
    );
    let honeypot = api::safety_report(
        T,
        &trip(false, 1_000_000, 0, 0),
        Address::ZERO,
        weth(),
        18,
        7,
        9,
    );
    assert_eq!(honeypot.verdict(), SafetyVerdict::Honeypot);
    let evidence = honeypot.round_trip.unwrap();
    assert!(!evidence.sell_succeeded);
    assert_eq!(
        (
            evidence.sell_tax_pct.as_str(),
            evidence.revert_reason.as_str()
        ),
        ("", "sells are disabled")
    );
    // The badge's summary carries the verdict and the taxes.
    let mut safety = Safety::default();
    safety.record(T, taxed_report());
    let summary = safety.summary(T).unwrap();
    assert_eq!(
        (summary.verdict(), summary.buy_tax_pct.as_str()),
        (SafetyVerdict::Warning, "5")
    );
}

fn taxed_report() -> proto::api::v1::SafetyReport {
    api::safety_report(
        T,
        &trip(true, 950_000, 902_500, 890),
        Address::ZERO,
        weth(),
        18,
        7,
        9,
    )
}
