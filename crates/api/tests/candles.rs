//! Candles (#80): OHLCV in USD per token and interval, built from live and backfilled trade
//! records, each trade counted once, the same whatever order the records arrive in, and served
//! over `GET /v1/candles` and the `candles:<address>:<interval>` topic.

use std::sync::Arc;

use alloy_primitives::U256;
use api::server::{Shared, parse_candles_query, router};
use api::{Feed, Session, to_json};
use prost::Message as _;
use proto::api::v1::{Candle, CandleInterval, CandleSeries};
use proto::lineage::v1::Lineage;
use proto::price::v1::{PoolPrice, PriceUpdate, TokenMetadata};
use proto::trade::v1::Trade;

const LOW: [u8; 20] = [0x10; 20];
const WETH: [u8; 20] = hex20("4200000000000000000000000000000000000006");
const USDC: [u8; 20] = hex20("833589fcd6edb6e08f4c7c32d4f71b54bda02913");
const OTHER: [u8; 20] = [0x20; 20];
/// LOW/USDC, counted.
const A: [u8; 20] = [0xa1; 20];
/// LOW/WETH, counted.
const B: [u8; 20] = [0xb1; 20];
/// LOW/USDC, below the floor.
const X: [u8; 20] = [0xc1; 20];
/// WETH/USDC, counted.
const W: [u8; 20] = [0xd1; 20];
/// LOW/OTHER: OTHER isn't a quote asset.
const N: [u8; 20] = [0xe1; 20];

/// A minute boundary: 2026-09-21T14:14:00Z.
const T0: u64 = 1_790_000_040;
const MINUTE_MS: u64 = 60_000;

const fn hex20(text: &str) -> [u8; 20] {
    let digits = text.as_bytes();
    let mut out = [0u8; 20];
    let mut i = 0;
    while i < 20 {
        out[i] = nibble(digits[2 * i]) << 4 | nibble(digits[2 * i + 1]);
        i += 1;
    }
    out
}

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        _ => c - b'a' + 10,
    }
}

fn hex(bytes: &[u8]) -> String {
    api::hex(bytes)
}

fn e(n: u32) -> u128 {
    10u128.pow(n)
}

fn big_endian(n: u128) -> Vec<u8> {
    let bytes = n.to_be_bytes();
    let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    bytes[first..].to_vec()
}

fn price(
    token: [u8; 20],
    decimals: u32,
    pools: &[([u8; 20], bool)],
    main: [u8; 20],
) -> PriceUpdate {
    PriceUpdate {
        lineage: Some(Lineage {
            id: vec![token[0]; 16],
            caused_by: vec![],
        }),
        chain_id: 8453,
        token: token.to_vec(),
        block_number: 9,
        block_timestamp: T0 - 2,
        metadata: Some(TokenMetadata {
            decimals: Some(decimals),
            ..TokenMetadata::default()
        }),
        main_pool: main.to_vec(),
        pools: pools
            .iter()
            .map(|(pool, counted)| PoolPrice {
                pool: pool.to_vec(),
                counted: *counted,
                ..PoolPrice::default()
            })
            .collect(),
        ..PriceUpdate::default()
    }
}

/// LOW (18 decimals) counts A and B and not X; WETH (18) counts W.
fn prices() -> Vec<PriceUpdate> {
    vec![
        price(WETH, 18, &[(W, true)], W),
        price(LOW, 18, &[(A, true), (B, true), (X, false), (N, true)], A),
    ]
}

/// A swap of `token_amount` of `token` for `quote_amount` of `quote`, in base units, at block
/// `block` (whose hash is derived from its number), `seconds` after `T0`.
fn trade(
    pool: [u8; 20],
    (token, quote): ([u8; 20], [u8; 20]),
    (block, log_index): (u64, u64),
    seconds: i64,
    token_amount: u128,
    quote_amount: u128,
) -> Trade {
    let price = (token_amount > 0).then(|| {
        U256::from(quote_amount) * U256::from(10u64).pow(U256::from(36u64))
            / U256::from(token_amount)
    });
    Trade {
        lineage: Some(Lineage {
            id: [block.to_le_bytes(), log_index.to_le_bytes()].concat(),
            caused_by: vec![],
        }),
        chain_id: 8453,
        pool: pool.to_vec(),
        token: token.to_vec(),
        quote: quote.to_vec(),
        token_amount: big_endian(token_amount),
        quote_amount: big_endian(quote_amount),
        price_e36: price.map(|p| {
            let bytes = p.to_be_bytes::<32>();
            let first = bytes.iter().position(|&b| b != 0).unwrap_or(32);
            bytes[first..].to_vec()
        }),
        block_number: block,
        block_hash: [block.to_be_bytes().as_slice(), &[0u8; 24]].concat(),
        block_timestamp: T0.checked_add_signed(seconds).unwrap(),
        log_index,
        ..Trade::default()
    }
}

/// In the first minute: WETH trades at $2,048; LOW at $0.50 on A, at 0.0003125 WETH ($0.64) on
/// B, and at $0.40 on A; and at $100 on X and 0.1 OTHER on N, neither of which counts.
fn first_minute() -> Vec<Trade> {
    vec![
        trade(W, (WETH, USDC), (10, 0), 0, e(18), 2_048 * e(6)),
        trade(A, (LOW, USDC), (10, 1), 0, 100 * e(18), 50 * e(6)),
        trade(B, (LOW, WETH), (11, 0), 2, 200 * e(18), 625 * e(14)),
        trade(A, (LOW, USDC), (12, 0), 4, 10 * e(18), 4 * e(6)),
        trade(X, (LOW, USDC), (12, 1), 4, e(18), 100 * e(6)),
        trade(N, (LOW, OTHER), (12, 2), 4, e(18), e(17)),
    ]
}

fn feed_with(trades: &[Trade]) -> Feed {
    let mut feed = Feed::default();
    for update in prices() {
        feed.apply_price(&update).unwrap();
    }
    for t in trades {
        feed.apply_trade(t);
    }
    feed
}

fn series(feed: &Feed, token: [u8; 20], interval: CandleInterval) -> CandleSeries {
    feed.candles().series(
        Some(8453),
        token.as_slice().try_into().unwrap(),
        interval,
        0,
        u64::MAX,
    )
}

/// (open time, open, high, low, close, volume, trades, closed)
type Row = (u64, String, String, String, String, String, u64, bool);

fn rows(series: &CandleSeries) -> Vec<Row> {
    series
        .candles
        .iter()
        .map(|c| {
            (
                c.open_time_ms,
                c.open_usd.clone(),
                c.high_usd.clone(),
                c.low_usd.clone(),
                c.close_usd.clone(),
                c.volume_usd.clone(),
                c.trades,
                c.closed,
            )
        })
        .collect()
}

fn row(open: u64, ohlc: [&str; 4], volume: &str, trades: u64, closed: bool) -> Row {
    (
        open,
        ohlc[0].into(),
        ohlc[1].into(),
        ohlc[2].into(),
        ohlc[3].into(),
        volume.into(),
        trades,
        closed,
    )
}

#[test]
fn a_tokens_candle_merges_its_counted_pools_in_usd() {
    let feed = feed_with(&first_minute());
    // Open from A's trade at (10, 1), close from A's at (12, 0), high from B's; X and N left
    // out. Volume: $50 + 0.0625 WETH at $2,048 + $4.
    assert_eq!(
        rows(&series(&feed, LOW, CandleInterval::CandleInterval1m)),
        vec![row(
            T0 * 1000,
            ["0.5", "0.64", "0.4", "0.4"],
            "182",
            3,
            false
        )]
    );
    assert_eq!(
        rows(&series(&feed, WETH, CandleInterval::CandleInterval1m)),
        vec![row(
            T0 * 1000,
            ["2048", "2048", "2048", "2048"],
            "2048",
            1,
            false
        )]
    );
    // One 1s candle per second traded in.
    assert_eq!(
        rows(&series(&feed, LOW, CandleInterval::CandleInterval1s)),
        vec![
            row(T0 * 1000, ["0.5", "0.5", "0.5", "0.5"], "50", 1, true),
            row(
                T0 * 1000 + 2_000,
                ["0.64", "0.64", "0.64", "0.64"],
                "128",
                1,
                true
            ),
            row(
                T0 * 1000 + 4_000,
                ["0.4", "0.4", "0.4", "0.4"],
                "4",
                1,
                false
            ),
        ]
    );
}

#[test]
fn a_candle_carries_its_lineage_and_key() {
    let feed = feed_with(&first_minute());
    let series = series(&feed, LOW, CandleInterval::CandleInterval5m);
    let candle: &Candle = &series.candles[0];
    assert_eq!(
        (candle.chain_id, candle.token.as_str(), candle.interval),
        (
            8453,
            hex(&LOW).as_str(),
            CandleInterval::CandleInterval5m as i32
        )
    );
    // 14:14 opens the 5m candle at 14:10.
    assert_eq!(candle.open_time_ms, T0 * 1000 - 4 * MINUTE_MS);
    // Caused by its latest trade, A's at (12, 0).
    let latest = [12u64.to_le_bytes(), 0u64.to_le_bytes()].concat();
    let lineage = candle.lineage.as_ref().unwrap();
    assert_eq!(lineage.caused_by, vec![latest.clone()]);
    let mut key = 8453u64.to_le_bytes().to_vec();
    key.extend(LOW);
    key.extend(300_000u64.to_le_bytes());
    key.extend((T0 * 1000 - 4 * MINUTE_MS).to_le_bytes());
    key.extend(&latest);
    assert_eq!(
        lineage.id,
        types::LineageId::from_natural_key("candle", &key)
            .as_bytes()
            .to_vec()
    );
    assert_eq!(
        series.lineage.as_ref().unwrap().caused_by,
        vec![lineage.id.clone()]
    );
}

#[test]
fn candles_open_on_their_intervals_and_close_with_the_chain() {
    // 14:14:00 plus 3,599s is 15:13:59; the next trade is 61s later, at 15:15:00.
    let feed = feed_with(&[
        trade(A, (LOW, USDC), (10, 0), 3_599, e(18), e(6)),
        trade(W, (WETH, USDC), (40, 0), 3_660, e(18), 2_000 * e(6)),
    ]);
    let opens: Vec<(u64, bool)> = api::INTERVALS
        .iter()
        .map(|(interval, _, _)| {
            let c = &series(&feed, LOW, *interval).candles[0];
            (c.open_time_ms / 1000, c.closed)
        })
        .collect();
    // 1s, 1m, 5m, 15m, 1h, 4h and 1d candles opening at 15:13:59, 15:13, 15:10, 15:00, 15:00,
    // 12:00 and midnight; by 15:15:00 the first four have ended.
    let t = T0 + 3_599;
    assert_eq!(
        opens,
        vec![
            (t, true),
            (t - 59, true),
            (t - 239, true),
            (t - 839, true),
            (t - 839, false),
            (t - 11_639, false),
            (t - 54_839, false),
        ]
    );
}

#[test]
fn a_trade_counts_once_however_often_it_arrives() {
    // The backfill-to-live overlap: the same trade, by block hash and log index, live and from
    // history, in either order and more than once.
    let swap = trade(A, (LOW, USDC), (10, 1), 0, 100 * e(18), 50 * e(6));
    let once = vec![row(T0 * 1000, ["0.5", "0.5", "0.5", "0.5"], "50", 1, false)];
    let mut history_first = feed_with(&[]);
    history_first.apply_history(&swap).unwrap();
    history_first.apply_trade(&swap);
    history_first.apply_history(&swap).unwrap();
    assert_eq!(
        rows(&series(
            &history_first,
            LOW,
            CandleInterval::CandleInterval1m
        )),
        once
    );
    let mut live_first = feed_with(&[swap.clone(), swap.clone()]);
    live_first.apply_history(&swap).unwrap();
    assert_eq!(
        rows(&series(&live_first, LOW, CandleInterval::CandleInterval1m)),
        once
    );
    // Another log in the same block is another trade.
    let mut next = swap.clone();
    next.log_index = 2;
    live_first.apply_history(&next).unwrap();
    assert_eq!(
        series(&live_first, LOW, CandleInterval::CandleInterval1m).candles[0].trades,
        2
    );
}

/// Every series of every token and interval, as JSON.
fn everything(feed: &Feed) -> String {
    let mut out = String::new();
    for token in [LOW, WETH] {
        for (interval, _, _) in api::INTERVALS {
            out.push_str(&serde_json::to_string(&series(feed, token, interval)).unwrap());
            out.push('\n');
        }
    }
    out
}

#[test]
fn candles_are_the_same_whatever_order_the_records_arrive_in() {
    // Two minutes of trades, the first minute's from history (and A's last trade from both).
    let history = first_minute();
    let live = vec![
        trade(A, (LOW, USDC), (12, 0), 4, 10 * e(18), 4 * e(6)),
        trade(W, (WETH, USDC), (40, 0), 61, e(18), 1_024 * e(6)),
        trade(B, (LOW, WETH), (41, 0), 63, 100 * e(18), 125 * e(14)),
        trade(A, (LOW, USDC), (41, 1), 63, 100 * e(18), 20 * e(6)),
    ];
    let build = |history_first: bool, reversed: bool, prices_last: bool| {
        let mut feed = Feed::default();
        let mut records: Vec<(bool, &Trade)> = history
            .iter()
            .map(|t| (true, t))
            .chain(live.iter().map(|t| (false, t)))
            .collect();
        if !history_first {
            records.rotate_left(history.len());
        }
        if reversed {
            records.reverse();
        }
        if !prices_last {
            for update in prices() {
                feed.apply_price(&update).unwrap();
            }
        }
        for (from_history, t) in records {
            if from_history {
                feed.apply_history(t).unwrap();
            } else {
                feed.apply_trade(t);
            }
        }
        if prices_last {
            for update in prices() {
                feed.apply_price(&update).unwrap();
            }
        }
        everything(&feed)
    };
    let first = build(true, false, false);
    for (history_first, reversed, prices_last) in [
        (false, false, false),
        (true, true, false),
        (false, true, true),
        (true, false, true),
    ] {
        assert!(
            build(history_first, reversed, prices_last) == first,
            "the candles depend on the order: history first {history_first}, reversed {reversed}, prices last {prices_last}"
        );
    }
    // The second minute: LOW at 0.000125 WETH ($0.128) and at $0.20; WETH at $1,024.
    let feed = {
        let mut feed = feed_with(&live);
        for t in &history {
            feed.apply_history(t).unwrap();
        }
        feed
    };
    assert_eq!(
        rows(&series(&feed, LOW, CandleInterval::CandleInterval1m))[1],
        row(
            T0 * 1000 + MINUTE_MS,
            ["0.128", "0.2", "0.128", "0.2"],
            "32.8",
            2,
            false
        )
    );
}

#[test]
fn a_weth_quoted_candle_takes_weths_close_or_the_nearest_one() {
    // WETH trades in the second and fourth minutes only; LOW on B in the first, third and fifth.
    let feed = feed_with(&[
        trade(W, (WETH, USDC), (20, 0), 60, e(18), 2_048 * e(6)),
        trade(W, (WETH, USDC), (21, 0), 61, e(18), 4_096 * e(6)),
        trade(W, (WETH, USDC), (40, 0), 180, e(18), 1_024 * e(6)),
        trade(B, (LOW, WETH), (10, 0), 0, e(18), 25 * e(13)),
        trade(B, (LOW, WETH), (30, 0), 120, e(18), 25 * e(13)),
        trade(B, (LOW, WETH), (50, 0), 240, e(18), 25 * e(13)),
    ]);
    let closes: Vec<(u64, String)> = series(&feed, LOW, CandleInterval::CandleInterval1m)
        .candles
        .iter()
        .map(|c| {
            (
                (c.open_time_ms - T0 * 1000) / MINUTE_MS,
                c.close_usd.clone(),
            )
        })
        .collect();
    // 0.00025 WETH: before WETH's first candle, at its earliest ($2,048 then $4,096: it closed at
    // $4,096); then at the latest earlier close ($4,096, then $1,024).
    assert_eq!(
        closes,
        vec![
            (0, "1.024".into()),
            (2, "1.024".into()),
            (4, "0.256".into())
        ]
    );
}

#[test]
fn a_weth_candle_without_a_price_is_passed_over() {
    // WETH's only trades in the first and third minutes moved no WETH (dust), so they have no
    // price; it trades at $4,096 in the second.
    let feed = feed_with(&[
        trade(W, (WETH, USDC), (10, 0), 0, 0, e(6)),
        trade(W, (WETH, USDC), (20, 0), 60, e(18), 4_096 * e(6)),
        trade(W, (WETH, USDC), (30, 0), 120, 0, e(6)),
        trade(B, (LOW, WETH), (10, 1), 0, e(18), 25 * e(13)),
        trade(B, (LOW, WETH), (30, 1), 120, e(18), 25 * e(13)),
    ]);
    let closes: Vec<(u64, String)> = series(&feed, LOW, CandleInterval::CandleInterval1m)
        .candles
        .iter()
        .map(|c| {
            (
                (c.open_time_ms - T0 * 1000) / MINUTE_MS,
                c.close_usd.clone(),
            )
        })
        .collect();
    // Both take the second minute's $4,096: the first looking forward, the third looking back.
    assert_eq!(closes, vec![(0, "1.024".into()), (2, "1.024".into())]);
}

#[test]
fn a_trade_without_a_price_counts_but_moves_no_price() {
    // The pool took 5 USDC in and paid no LOW out: a txn and volume, but no price.
    let feed = feed_with(&[
        trade(A, (LOW, USDC), (10, 0), 0, 100 * e(18), 50 * e(6)),
        trade(A, (LOW, USDC), (11, 0), 2, 0, 5 * e(6)),
    ]);
    assert_eq!(
        rows(&series(&feed, LOW, CandleInterval::CandleInterval1m)),
        vec![row(T0 * 1000, ["0.5", "0.5", "0.5", "0.5"], "55", 2, false)]
    );
    let only_unpriced = feed_with(&[trade(A, (LOW, USDC), (11, 0), 2, 0, 5 * e(6))]);
    assert_eq!(
        rows(&series(
            &only_unpriced,
            LOW,
            CandleInterval::CandleInterval1m
        )),
        vec![row(T0 * 1000, ["", "", "", ""], "5", 1, false)]
    );
}

#[test]
fn a_thin_token_takes_its_main_pool_and_its_latest_price_update_counts() {
    let mut feed = feed_with(&first_minute());
    // Nothing counted: the main pool, X, alone.
    feed.apply_price(&price(LOW, 18, &[(A, false), (X, false)], X))
        .unwrap();
    assert_eq!(
        rows(&series(&feed, LOW, CandleInterval::CandleInterval1m)),
        vec![row(
            T0 * 1000,
            ["100", "100", "100", "100"],
            "100",
            1,
            false
        )]
    );
    // Without decimals there's no price to scale, so no candles.
    let mut no_decimals = price(LOW, 18, &[(A, true)], A);
    no_decimals.metadata = None;
    feed.apply_price(&no_decimals).unwrap();
    assert!(
        series(&feed, LOW, CandleInterval::CandleInterval1m)
            .candles
            .is_empty()
    );
}

#[test]
fn a_series_holds_the_newest_candles_in_range() {
    let trades: Vec<Trade> = (0..(api::MAX_SERIES as u64 + 2))
        .map(|i| trade(A, (LOW, USDC), (10 + i, 0), i as i64, e(18), e(6)))
        .collect();
    let feed = feed_with(&trades);
    let token = LOW.as_slice().try_into().unwrap();
    let all = series(&feed, LOW, CandleInterval::CandleInterval1s);
    assert_eq!(all.candles.len(), api::MAX_SERIES);
    assert_eq!(all.candles[0].open_time_ms, (T0 + 2) * 1000);
    let some = feed.candles().series(
        Some(8453),
        token,
        CandleInterval::CandleInterval1s,
        (T0 + 5) * 1000,
        (T0 + 7) * 1000,
    );
    let opens: Vec<u64> = some
        .candles
        .iter()
        .map(|c| c.open_time_ms / 1000 - T0)
        .collect();
    assert_eq!(opens, vec![5, 6, 7]);
    let elsewhere = feed.candles().series(
        Some(1),
        token,
        CandleInterval::CandleInterval1s,
        0,
        u64::MAX,
    );
    assert!(elsewhere.candles.is_empty());
    assert_eq!(elsewhere.chain_id, 1);
}

#[test]
fn a_subscriber_gets_the_series_then_each_candle_and_a_fresh_series_after_history() {
    let mut feed = feed_with(&[]);
    let mut session = Session::default();
    let name = format!("candles:{}:1m", hex(&LOW));
    let mut out: Vec<String> = session
        .on_text(&format!(r#"{{"subscribe":{{"topic":"{name}"}}}}"#), &feed)
        .iter()
        .map(to_json)
        .collect();
    let mut send = |feed: &mut Feed, apply: &dyn Fn(&mut Feed) -> Vec<api::Published>| {
        for published in apply(feed) {
            if let Some(message) = session.on_published(&published, feed) {
                out.push(to_json(&message));
            }
        }
    };
    let first = trade(A, (LOW, USDC), (10, 0), 0, 100 * e(18), 50 * e(6));
    let earlier = trade(A, (LOW, USDC), (5, 0), -10, 100 * e(18), 30 * e(6));
    let next = trade(A, (LOW, USDC), (11, 0), 2, 100 * e(18), 60 * e(6));
    send(&mut feed, &|f| f.apply_trade(&first));
    // Counted already: nothing.
    send(&mut feed, &|f| f.apply_trade(&first));
    // History waits for the next live trade, and then the series goes out whole, with that
    // trade in it already, so its own delta is skipped.
    send(&mut feed, &|f| {
        f.apply_history(&earlier).unwrap();
        Vec::new()
    });
    send(&mut feed, &|f| f.apply_trade(&next));
    let kinds: Vec<&str> = out
        .iter()
        .map(|m| {
            if m.starts_with(r#"{"snapshot""#) {
                "snapshot"
            } else {
                "delta"
            }
        })
        .collect();
    assert_eq!(kinds, vec!["snapshot", "delta", "snapshot"]);
    assert!(
        out[0].starts_with(&format!(r#"{{"snapshot":{{"topic":"{name}","candles":{{"#)),
        "{}",
        out[0]
    );
    assert!(
        !out[0].contains("openTimeMs"),
        "an empty series: {}",
        out[0]
    );
    assert!(
        out[1].contains(r#""seq":"1""#) && out[1].contains(r#""closeUsd":"0.5""#),
        "{}",
        out[1]
    );
    // The fresh series holds the history's candle, then the first minute's two trades.
    assert_eq!(out[2].matches("openTimeMs").count(), 2, "{}", out[2]);
    assert!(out[2].contains(r#""closeUsd":"0.3""#), "{}", out[2]);
    assert!(
        out[2].contains(r#""closeUsd":"0.6","volumeUsd":"110","trades":"2""#),
        "{}",
        out[2]
    );
}

#[test]
fn a_trade_on_a_pool_that_doesnt_count_changes_no_candle() {
    let mut feed = feed_with(&[trade(A, (LOW, USDC), (10, 0), 0, 100 * e(18), 50 * e(6))]);
    // X is below the floor: its trade counts toward nothing LOW's candles show.
    let published = feed.apply_trade(&trade(X, (LOW, USDC), (11, 0), 2, e(18), 100 * e(6)));
    assert!(
        published.iter().all(|p| !p.topic.starts_with("candles:")),
        "{published:?}"
    );
}

#[test]
fn a_delta_the_snapshot_already_counted_is_skipped() {
    let mut feed = feed_with(&[]);
    let first = trade(A, (LOW, USDC), (10, 0), 0, 100 * e(18), 50 * e(6));
    let published = feed.apply_trade(&first);
    // A client that subscribes after the trade was counted, before its delta reaches it.
    let mut session = Session::default();
    let name = format!("candles:{}:1m", hex(&LOW));
    let snapshot = session.on_text(&format!(r#"{{"subscribe":{{"topic":"{name}"}}}}"#), &feed);
    assert_eq!(snapshot.len(), 1);
    let late: Vec<_> = published
        .iter()
        .filter_map(|p| session.on_published(p, &feed))
        .collect();
    assert!(late.is_empty());
}

#[test]
fn candle_topics_name_a_token_and_an_interval() {
    use api::Topic;
    let address = hex(&LOW);
    assert_eq!(
        api::parse_topic(&format!("candles:{address}:15m")),
        Some(Topic::Candles(
            address.clone(),
            CandleInterval::CandleInterval15m
        ))
    );
    assert_eq!(api::parse_topic(&format!("candles:{address}:2h")), None);
    assert_eq!(api::parse_topic(&format!("candles:{address}")), None);
    assert_eq!(api::parse_topic("candles:0x12:1m"), None);
}

async fn http_get(address: &str, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn rest_serves_candles_from_live_and_history_topics() {
    let shared: Arc<Shared> = Shared::new();
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    for update in prices() {
        shared.apply(engine::PRICES_TOPIC, &update.encode_to_vec());
    }
    let history = trade(A, (LOW, USDC), (5, 0), 0, 100 * e(18), 30 * e(6));
    let live = trade(A, (LOW, USDC), (10, 0), 60, 100 * e(18), 50 * e(6));
    shared.apply(engine::TRADE_HISTORY_TOPIC, &history.encode_to_vec());
    shared.apply(engine::TRADES_TOPIC, &live.encode_to_vec());

    let token = hex(&LOW);
    let ok = http_get(
        &address,
        &format!("/v1/candles?chain_id=8453&token={token}&interval=1m"),
    )
    .await;
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    let body = ok.split("\r\n\r\n").nth(1).unwrap();
    let served: CandleSeries = serde_json::from_str(body).unwrap();
    assert_eq!(
        rows(&served),
        vec![
            row(T0 * 1000, ["0.3", "0.3", "0.3", "0.3"], "30", 1, true),
            row(
                T0 * 1000 + MINUTE_MS,
                ["0.5", "0.5", "0.5", "0.5"],
                "50",
                1,
                false
            ),
        ]
    );
    let from = http_get(
        &address,
        &format!(
            "/v1/candles?chain_id=8453&token={token}&interval=1m&from_ms={}",
            T0 * 1000 + 1
        ),
    )
    .await;
    assert!(
        from.contains(r#""closeUsd":"0.5""#) && !from.contains(r#""closeUsd":"0.3""#),
        "{from}"
    );
    for bad in [
        format!("/v1/candles?chain_id=8453&token={token}&interval=2m"),
        "/v1/candles?chain_id=8453&interval=1m".to_string(),
        format!("/v1/candles?chain_id=base&token={token}&interval=1m"),
        "/v1/candles?chain_id=8453&token=0x12&interval=1m".to_string(),
        format!("/v1/candles?chain_id=8453&token={token}&interval=1m&to_ms=soon"),
    ] {
        let refused = http_get(&address, &bad).await;
        assert!(refused.starts_with("HTTP/1.1 400"), "{bad}: {refused}");
    }
}

#[test]
fn a_candles_query_defaults_its_range() {
    let params = [
        ("chain_id", "8453"),
        ("token", "0x1010101010101010101010101010101010101010"),
        ("interval", "4h"),
        ("from_ms", ""),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let query = parse_candles_query(&params).unwrap();
    assert_eq!(
        (query.chain_id, query.interval, query.from_ms, query.to_ms),
        (8453, CandleInterval::CandleInterval4h, 0, u64::MAX)
    );
    assert_eq!(query.token.as_slice(), LOW);
}
