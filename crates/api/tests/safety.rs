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

#[test]
fn a_check_that_fails_replaces_an_earlier_pass_with_a_warning() {
    let mut safety = Safety::default();
    record(&mut safety, T, 10);
    assert_eq!(safety.summary(T).unwrap().verdict(), SafetyVerdict::Passed);
    safety.check_failed(T, "the probe reverted", 20);
    let report = safety.report(T).unwrap();
    assert_eq!(report.verdict(), SafetyVerdict::Warning);
    assert_eq!(
        (
            report.flags[0].code.as_str(),
            report.flags[0].detail.as_str(),
            report.checked_at_ms
        ),
        ("check_failed", "the probe reverted", 20)
    );
    assert!(report.round_trip.is_none());
}

#[test]
fn a_failed_check_keeps_a_honeypot_a_honeypot() {
    let mut safety = Safety::default();
    let honeypot = api::safety_report(
        T,
        &trip(false, 1_000_000, 0, 0),
        Address::ZERO,
        weth(),
        18,
        1,
        10,
    );
    safety.record(T, honeypot);
    safety.check_failed(T, "the endpoint didn't answer", 20);
    safety.check_failed(T, "the endpoint didn't answer", 30);
    let report = safety.report(T).unwrap();
    assert_eq!(report.verdict(), SafetyVerdict::Honeypot);
    assert_eq!(report.flags.len(), 1);
    assert_eq!(report.flags[0].code, "check_failed");
    assert!(report.round_trip.is_some());
}

#[test]
fn a_revert_reason_is_cut_to_a_panels_size() {
    let mut long = trip(false, 1_000_000, 0, 0);
    long.reason = "x".repeat(10_000);
    let report = api::safety_report(T, &long, Address::ZERO, weth(), 18, 1, 10);
    assert_eq!(
        report.round_trip.unwrap().revert_reason.len(),
        api::MAX_REASON
    );
}

#[test]
fn the_check_windows_are_five_and_thirty_minutes() {
    assert_eq!((RECHECK_MS, WATCH_MS), (300_000, 1_800_000));
}

#[test]
fn a_token_stays_watched_until_exactly_thirty_minutes_after_its_last_read() {
    let mut safety = Safety::default();
    safety.watch(T, 1);
    assert_eq!(safety.next_due(), Some(T));
    record(&mut safety, T, 1);
    // The last moment of the window: still watched, and its report is stale, so it's queued.
    safety.sweep(WATCH_MS);
    assert_eq!(safety.next_due(), Some(T));
    // The window has closed: a stale report is no longer rechecked.
    record(&mut safety, T, 2);
    safety.sweep(1 + WATCH_MS);
    assert_eq!(safety.next_due(), None);
}

#[test]
fn a_discovered_weth_pool_queues_its_token_and_another_pool_doesnt() {
    use proto::pool::v1::{
        PoolKey, PoolState, PoolUpdate, V2Reserves, V3State, pool_key, pool_state,
    };
    use proto::price::v1::{PriceUpdate, TokenMetadata};
    let weth: Address = "0x4200000000000000000000000000000000000006"
        .parse()
        .unwrap();
    let pool = |pool: u8, token0: Address, token1: Address| PoolUpdate {
        chain_id: 8453,
        pool: vec![pool; 20],
        block_number: 1,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: vec![1],
                reserve1: vec![1],
            })),
        }),
        key: Some(PoolKey {
            venue: pool_key::Venue::UniswapV2.into(),
            token0: token0.to_vec(),
            token1: token1.to_vec(),
            fee_pips: 3_000,
            tick_spacing: 0,
        }),
        ..PoolUpdate::default()
    };
    let mut feed = api::Feed::default();
    // WETH as token1, then as token0.
    feed.apply_pool_update(&pool(1, T, weth));
    feed.apply_pool_update(&pool(2, weth, U));
    // Two tokens against each other, neither WETH: nothing to probe through.
    feed.apply_pool_update(&pool(
        3,
        Address::repeat_byte(0x01),
        Address::repeat_byte(0x02),
    ));
    assert_eq!(feed.safety().next_due(), Some(T));
    assert_eq!(feed.safety().next_due(), Some(U));
    assert_eq!(feed.safety().next_due(), None);
    // The probe goes through the token's WETH pool, v2 here.
    assert_eq!(
        feed.quotes().weth_pool(T),
        Some((Address::from([1; 20]), false))
    );
    assert_eq!(feed.quotes().weth_pool(Address::repeat_byte(0x01)), None);

    // T's main pool is a v3 WETH pool, so the probe goes through that instead.
    let mut v3 = pool(4, weth, T);
    v3.after = Some(PoolState {
        state: Some(pool_state::State::V3(V3State {
            initialized: true,
            sqrt_price_x96: vec![1],
            liquidity: vec![1],
            ..V3State::default()
        })),
    });
    v3.key.as_mut().unwrap().venue = pool_key::Venue::UniswapV3.into();
    v3.key.as_mut().unwrap().tick_spacing = 60;
    feed.apply_pool_update(&v3);
    let priced = |token: Address, main: u8, decimals: u32| PriceUpdate {
        chain_id: 8453,
        token: token.to_vec(),
        block_number: 2,
        metadata: Some(TokenMetadata {
            decimals: Some(decimals),
            ..TokenMetadata::default()
        }),
        main_pool: vec![main; 20],
        ..PriceUpdate::default()
    };
    feed.apply_price(&priced(T, 4, 9)).unwrap();
    // U's main pool doesn't trade it against WETH: its WETH pool is used.
    feed.apply_price(&priced(U, 3, 6)).unwrap();
    let quotes = feed.quotes();
    assert_eq!(quotes.weth_pool(T), Some((Address::from([4; 20]), true)));
    assert_eq!(quotes.weth_pool(U), Some((Address::from([2; 20]), false)));
    assert_eq!(
        (
            quotes.token_decimals(T),
            quotes.token_decimals(weth),
            quotes.token_decimals(Address::repeat_byte(0x09))
        ),
        (Some(9), Some(18), None)
    );
}
