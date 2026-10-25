//! Explanations (#87): a stop's firing, explained from the swap that moved MEME's price to the
//! receipt of the sale it made, by following the records' lineage IDs; a manual trade's, from
//! its route; and the same explanation again when the same records replay into a fresh server
//! on the same clock.

use std::sync::Arc;

use alloy_primitives::U256;
use api::server::{Shared, router};
use api::{Demo, Sessions, hex};
use prost::Message as _;
use proto::api::v1::firing_explanation::Kind;
use proto::api::v1::order_level::Level;
use proto::api::v1::{
    FiringExplanation, Order, OrderKind, OrderLevel, OrderList, PlaceOrderRequest, Quote,
    QuoteRequest, TradeRequest,
};
use proto::lineage::v1::Lineage;
use proto::pool::v1::{PoolKey, PoolState, PoolUpdate, V2Reserves, pool_key, pool_state};
use proto::price::v1::{PoolPrice, PriceUpdate, TokenMetadata};
use proto::trade::v1::{Trade, trade};
use types::Timestamp;

const MEME: [u8; 20] = [0x30; 20];
const WETH: [u8; 20] = [
    0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06,
];
const USDC: [u8; 20] = [
    0x83, 0x35, 0x89, 0xfc, 0xd6, 0xed, 0xb6, 0xe0, 0x8f, 0x4c, 0x7c, 0x32, 0xd4, 0xf7, 0x1b, 0x54,
    0xbd, 0xa0, 0x29, 0x13,
];
const MEME_WETH: [u8; 20] = [0xb1; 20];
const WETH_USDC: [u8; 20] = [0xc1; 20];
/// A shallower WETH/USDC pair: the route through it loses.
const WETH_USDC_THIN: [u8; 20] = [0xc2; 20];

/// Block 100's time; a block every two seconds.
const BLOCK_100_S: u64 = 1_790_000_000;
/// The server's clock, fixed: every session, order and trade step reads it.
const NOW_MS: u64 = 1_790_000_010_000;

struct Fixed;

impl det::Clock for Fixed {
    fn now(&self) -> Timestamp {
        Timestamp {
            unix_nanos: NOW_MS * 1_000_000,
        }
    }
}

fn e(n: u32) -> u128 {
    10u128.pow(n)
}

fn be(n: u128) -> Vec<u8> {
    U256::from(n).to_be_bytes_trimmed_vec()
}

fn lineage(id: u8, caused_by: &[u8]) -> Option<Lineage> {
    Some(Lineage {
        id: vec![id; 16],
        caused_by: caused_by.iter().map(|c| vec![*c; 16]).collect(),
    })
}

/// A v2 pair's update in `block`, caused by chain event `event`.
fn pair(
    pool: [u8; 20],
    a: ([u8; 20], u128),
    b: ([u8; 20], u128),
    block: u64,
    ids: (u8, u8),
) -> PoolUpdate {
    let (first, second) = if a.0 < b.0 { (a, b) } else { (b, a) };
    PoolUpdate {
        lineage: lineage(ids.0, &[ids.1]),
        chain_id: 8453,
        pool: pool.to_vec(),
        block_number: block,
        log_index: 7,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: be(first.1),
                reserve1: be(second.1),
            })),
        }),
        key: (block == 100).then(|| PoolKey {
            venue: pool_key::Venue::UniswapV2.into(),
            token0: first.0.to_vec(),
            token1: second.0.to_vec(),
            fee_pips: 3_000,
            tick_spacing: 0,
        }),
        ..PoolUpdate::default()
    }
}

fn price(
    token: [u8; 20],
    symbol: &str,
    usd: f64,
    block: u64,
    ids: (u8, &[u8]),
    pools: Vec<PoolPrice>,
) -> PriceUpdate {
    PriceUpdate {
        lineage: lineage(ids.0, ids.1),
        chain_id: 8453,
        token: token.to_vec(),
        block_number: block,
        block_timestamp: BLOCK_100_S + 2 * (block - 100),
        price_usd: usd,
        metadata: Some(TokenMetadata {
            symbol: Some(symbol.into()),
            decimals: Some(if token == USDC { 6 } else { 18 }),
            ..TokenMetadata::default()
        }),
        main_pool: pools.first().map(|p| p.pool.clone()).unwrap_or_default(),
        pools,
        ..PriceUpdate::default()
    }
}

fn meme_pool(usd: f64, depth_usd: f64) -> PoolPrice {
    PoolPrice {
        pool: MEME_WETH.to_vec(),
        quote_token: WETH.to_vec(),
        price_usd: usd,
        depth_usd,
        counted: true,
        ..PoolPrice::default()
    }
}

/// The records of blocks 100 and 101, as the engine would publish them, in order. Block 101's
/// swap sells 11.8M MEME into the MEME/WETH pair, moving MEME from $0.01 to $0.008.
fn records() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            engine::POOL_UPDATES_TOPIC,
            pair(
                MEME_WETH,
                (MEME, 100_000_000 * e(18)),
                (WETH, 400 * e(18)),
                100,
                (0xa0, 0xe0),
            )
            .encode_to_vec(),
        ),
        (
            engine::POOL_UPDATES_TOPIC,
            pair(
                WETH_USDC,
                (WETH, 40_000 * e(18)),
                (USDC, 100_000_000 * e(6)),
                100,
                (0xa1, 0xe1),
            )
            .encode_to_vec(),
        ),
        (
            engine::POOL_UPDATES_TOPIC,
            pair(
                WETH_USDC_THIN,
                (WETH, 400 * e(18)),
                (USDC, 1_000_000 * e(6)),
                100,
                (0xa3, 0xe3),
            )
            .encode_to_vec(),
        ),
        (
            engine::PRICES_TOPIC,
            price(WETH, "WETH", 2_500.0, 100, (0xb0, &[0xa1]), vec![]).encode_to_vec(),
        ),
        (
            engine::PRICES_TOPIC,
            price(
                MEME,
                "MEME",
                0.01,
                100,
                (0xb1, &[0xa0, 0xb0]),
                vec![meme_pool(0.01, 2_000_000.0)],
            )
            .encode_to_vec(),
        ),
    ]
}

fn block_101() -> Vec<(&'static str, Vec<u8>)> {
    let swap = Trade {
        lineage: lineage(0xd2, &[0xe2]),
        chain_id: 8453,
        pool: MEME_WETH.to_vec(),
        venue: trade::Venue::UniswapV2.into(),
        token: MEME.to_vec(),
        quote: WETH.to_vec(),
        side: trade::Side::Sell.into(),
        token_amount: be(11_803_399 * e(18)),
        quote_amount: be(42_229_124 * e(12)),
        sender: vec![0x51; 20],
        recipient: vec![0x52; 20],
        tx_hash: vec![0xdd; 32],
        block_number: 101,
        block_hash: vec![0x01; 32],
        block_timestamp: BLOCK_100_S + 2,
        log_index: 7,
        ..Trade::default()
    };
    vec![
        (
            engine::POOL_UPDATES_TOPIC,
            pair(
                MEME_WETH,
                (MEME, 111_803_399 * e(18)),
                (WETH, 357_770_876 * e(12)),
                101,
                (0xa2, 0xe2),
            )
            .encode_to_vec(),
        ),
        (engine::TRADES_TOPIC, swap.encode_to_vec()),
        (
            engine::PRICES_TOPIC,
            price(
                MEME,
                "MEME",
                0.008,
                101,
                (0xb2, &[0xa2, 0xb0]),
                vec![meme_pool(0.008, 1_788_854.0)],
            )
            .encode_to_vec(),
        ),
    ]
}

async fn http(address: &str, method: &str, path: &str, token: &str, body: &str) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let code = response[9..12].parse().unwrap();
    (
        code,
        response
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or_default()
            .to_string(),
    )
}

/// A server on the fixed clock, fed block 100, with a guest who bought MEME with 500 USDC and
/// put a stop on all of it 20% below their entry; then block 101, which fires it.
async fn fired() -> (String, String, String, Order) {
    let demo = Demo::new(Sessions::new(b"s"), None);
    let session = demo
        .open(None, "guest-explained".into(), None, NOW_MS)
        .await
        .unwrap();
    let shared = Shared::with_clock(api::DiscoveryConfig::default(), None, demo, Arc::new(Fixed));
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for (topic, payload) in records() {
        shared.apply(topic, &payload);
    }
    let token = session.token;
    let quote = QuoteRequest {
        client_request_id: "buy".into(),
        chain_id: 8453,
        token_in: hex(&USDC),
        token_out: hex(&MEME),
        amount_in: "500".into(),
        slippage_bps: None,
    };
    let (_, body) = http(
        &address,
        "POST",
        "/v1/quotes",
        &token,
        &serde_json::to_string(&quote).unwrap(),
    )
    .await;
    let quote: Quote = serde_json::from_str(&body).unwrap();
    let trade = TradeRequest {
        client_request_id: "buy".into(),
        quote_id: quote.quote_id,
        intent_signature: String::new(),
    };
    let (code, body) = http(
        &address,
        "POST",
        "/v1/trades",
        &token,
        &serde_json::to_string(&trade).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let bought: proto::api::v1::TradeStatus = serde_json::from_str(&body).unwrap();
    let place = PlaceOrderRequest {
        client_request_id: "stop".into(),
        chain_id: 8453,
        token: hex(&MEME),
        kind: OrderKind::StopLoss.into(),
        level: Some(OrderLevel {
            level: Some(Level::PctFromEntry("-20".into())),
        }),
        sell_pct: "100".into(),
        ..PlaceOrderRequest::default()
    };
    let (code, body) = http(
        &address,
        "POST",
        "/v1/orders",
        &token,
        &serde_json::to_string(&place).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    for (topic, payload) in block_101() {
        shared.apply(topic, &payload);
    }
    shared.run_firings().await;
    let (_, body) = http(&address, "GET", "/v1/orders", &token, "").await;
    let order = serde_json::from_str::<OrderList>(&body)
        .unwrap()
        .orders
        .remove(0);
    (address, token, bought.trade_id, order)
}

async fn explanation(address: &str, token: &str, path: &str) -> (u16, String) {
    http(address, "GET", path, token, "").await
}

fn kinds(explanation: &FiringExplanation) -> Vec<Kind> {
    explanation.steps.iter().map(|s| s.kind()).collect()
}

fn unhex(value: &str) -> Vec<u8> {
    let digits = value.trim_start_matches("0x");
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
        .collect()
}

#[tokio::test]
async fn a_firing_is_explained_from_the_swap_that_moved_the_price_to_its_receipt() {
    let (address, token, _, order) = fired().await;
    let status = order.status.unwrap();
    assert_eq!(status.firing_count, 1, "{}", status.detail);
    let (code, body) = explanation(
        &address,
        &token,
        &format!("/v1/firings/{}/explanation", status.last_firing_id),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let explained: FiringExplanation = serde_json::from_str(&body).unwrap();

    let firing = explained.firing.clone().unwrap();
    let receipt = explained.receipt.clone().unwrap();
    let firing_lineage = firing.lineage.clone().unwrap().id;
    let receipt_lineage = receipt.lineage.clone().unwrap().id;
    let steps: Vec<(Kind, u64, Vec<u8>, &str)> = explained
        .steps
        .iter()
        .map(|s| (s.kind(), s.at_ms, s.lineage_id.clone(), s.summary.as_str()))
        .collect();
    let block_101_ms = (BLOCK_100_S + 2) * 1000;
    let fired_summary = format!(
        "Order {} fired, firing 1 ({})",
        order.order_id, status.last_firing_id
    );
    assert_eq!(
        steps,
        [
            // The swap's chain event, which the pool update and the swap record both name.
            (
                Kind::ChainEvent,
                block_101_ms,
                vec![0xe2; 16],
                "Swap in 0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd at block 101, log 7"
            ),
            (
                Kind::PoolUpdate,
                block_101_ms,
                vec![0xa2; 16],
                "Pool 0xb1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1 moved from $0.01 to $0.008"
            ),
            (
                Kind::PriceUpdate,
                block_101_ms,
                vec![0xb2; 16],
                "Display price moved from $0.01 to $0.008, across $0.008133552107484017"
            ),
            (
                Kind::TriggerCheck,
                block_101_ms,
                firing_lineage.clone(),
                fired_summary.as_str()
            ),
            (
                Kind::RouteDecision,
                NOW_MS,
                unhex(&receipt.quote_id),
                "Route through 2 pools, over 1 other route"
            ),
            (
                Kind::Simulation,
                NOW_MS,
                steps_lineage(&explained, Kind::Simulation),
                "Simulated at block 101: 386.993008 out"
            ),
            (
                Kind::Receipt,
                NOW_MS,
                receipt_lineage.clone(),
                "Shadow fill: 49179.005029296297437105 MEME for 386.993008 USDC"
            ),
        ]
    );
    let swap = explained.moving_trade.clone().unwrap();
    assert_eq!(
        (
            swap.side(),
            swap.token_amount.as_str(),
            swap.quote_amount.as_str(),
            swap.lineage.unwrap().id
        ),
        (
            proto::api::v1::trade::Side::Sell,
            "11803399",
            "42.229124",
            vec![0xd2; 16]
        )
    );
    let pool = explained.pool_change.clone().unwrap();
    assert_eq!(
        (
            pool.price_before_usd.as_str(),
            pool.price_after_usd.as_str(),
            pool.depth_before_usd.as_str(),
            pool.depth_after_usd.as_str()
        ),
        ("0.01", "0.008", "2000000", "1788854")
    );
    // The route it took beat the one through the thinner WETH/USDC pair.
    let decision = explained.route_decision.clone().unwrap();
    let pools = |c: &proto::api::v1::firing_explanation::Candidate| {
        c.route.iter().map(|l| l.pool.clone()).collect::<Vec<_>>()
    };
    assert_eq!(
        pools(decision.chosen.as_ref().unwrap()),
        [hex(&MEME_WETH), hex(&WETH_USDC)]
    );
    assert_eq!(
        decision
            .alternatives
            .iter()
            .map(|c| (pools(c), c.amount_out.as_str(), c.rejected_because.as_str()))
            .collect::<Vec<_>>(),
        [(
            vec![hex(&MEME_WETH), hex(&WETH_USDC_THIN)],
            "390.750811",
            "0.151217 USDC less after gas"
        )]
    );
    let simulation = explained.simulation.clone().unwrap();
    assert_eq!(
        (
            simulation.block_number,
            simulation.succeeded,
            simulation.in_memory
        ),
        (101, true, true)
    );
    assert_eq!(
        (firing.trade_id.as_str(), receipt.firing_id.as_str()),
        (explained.trade_id.as_str(), status.last_firing_id.as_str())
    );
    // Its lineage is caused by each step's record.
    assert_eq!(
        explained.lineage.unwrap().caused_by,
        explained
            .steps
            .iter()
            .map(|s| s.lineage_id.clone())
            .collect::<Vec<_>>()
    );

    // The trade it made explains the same way.
    let (code, trade_body) = explanation(
        &address,
        &token,
        &format!("/v1/trades/{}/explanation", firing.trade_id),
    )
    .await;
    assert_eq!((code, trade_body), (200, body));
}

fn steps_lineage(explained: &FiringExplanation, kind: Kind) -> Vec<u8> {
    explained
        .steps
        .iter()
        .find(|s| s.kind() == kind)
        .unwrap()
        .lineage_id
        .clone()
}

#[tokio::test]
async fn a_manual_trade_is_explained_from_its_route_and_only_to_its_account() {
    let (address, token, bought, order) = fired().await;
    let (code, body) = explanation(
        &address,
        &token,
        &format!("/v1/trades/{bought}/explanation"),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let explained: FiringExplanation = serde_json::from_str(&body).unwrap();
    assert_eq!(
        kinds(&explained),
        [Kind::RouteDecision, Kind::Simulation, Kind::Receipt]
    );
    assert_eq!(
        (explained.firing_id.as_str(), explained.firing.is_none()),
        ("", true)
    );
    assert_eq!(
        explained.steps[2].summary,
        "Shadow fill: 500 USDC for 49179.005029296297437105 MEME"
    );

    // Another account sees neither the trade's explanation nor the firing's.
    let (_, session) = http(&address, "POST", "/v1/session", "", "").await;
    let other = serde_json::from_str::<proto::api::v1::Session>(&session)
        .unwrap()
        .session_token;
    let firing_id = order.status.unwrap().last_firing_id;
    for path in [
        format!("/v1/trades/{bought}/explanation"),
        format!("/v1/firings/{firing_id}/explanation"),
    ] {
        assert_eq!(explanation(&address, &other, &path).await.0, 404, "{path}");
        assert_eq!(
            explanation(&address, "not-a-session", &path).await.0,
            401,
            "{path}"
        );
    }
}

#[tokio::test]
async fn a_replayed_firing_is_explained_exactly_as_the_live_one() {
    let (live, live_token, _, live_order) = fired().await;
    let (replayed, replay_token, _, replay_order) = fired().await;
    let firing_id = live_order.status.unwrap().last_firing_id;
    assert_eq!(replay_order.status.unwrap().last_firing_id, firing_id);
    let path = format!("/v1/firings/{firing_id}/explanation");
    let (code, live_body) = explanation(&live, &live_token, &path).await;
    assert_eq!(code, 200);
    assert_eq!(
        explanation(&replayed, &replay_token, &path).await,
        (200, live_body)
    );
}
