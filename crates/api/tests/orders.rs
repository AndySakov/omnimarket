//! Trigger orders over the API (#86): a stop placed on a position fires once when a recorded
//! price path crosses it, sells through a shadow trade and books the fill. Only canonical
//! prices (`prices.base`) fire it, and a restart mid-path neither fires it again nor skips it.

use std::sync::Arc;

use alloy_primitives::U256;
use api::server::{Shared, router};
use api::{Demo, Sessions, Store, hex};
use prost::Message as _;
use proto::api::v1::order_level::Level;
use proto::api::v1::order_status::State as OrderState;
use proto::api::v1::{
    Order, OrderKind, OrderLevel, OrderList, PlaceOrderRequest, PositionList, Quote, QuoteRequest,
    Session, TradeHistory, TradeRequest,
};
use proto::pool::v1::{PoolKey, PoolState, PoolUpdate, V2Reserves, pool_key, pool_state};
use proto::price::v1::{PriceUpdate, TokenMetadata};

const MEME: [u8; 20] = [0x30; 20];
const WETH: [u8; 20] = [
    0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06,
];
const USDC: [u8; 20] = [
    0x83, 0x35, 0x89, 0xfc, 0xd6, 0xed, 0xb6, 0xe0, 0x8f, 0x4c, 0x7c, 0x32, 0xd4, 0xf7, 0x1b, 0x54,
    0xbd, 0xa0, 0x29, 0x13,
];
const MEME_WETH: [u8; 20] = [0xb1; 20];

fn be(n: u128) -> Vec<u8> {
    U256::from(n).to_be_bytes_trimmed_vec()
}

fn e(n: u32) -> u128 {
    10u128.pow(n)
}

fn pair(pool: [u8; 20], a: ([u8; 20], u128), b: ([u8; 20], u128), discovered: bool) -> PoolUpdate {
    let (first, second) = if a.0 < b.0 { (a, b) } else { (b, a) };
    PoolUpdate {
        chain_id: 8453,
        pool: pool.to_vec(),
        block_number: 100,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: be(first.1),
                reserve1: be(second.1),
            })),
        }),
        key: discovered.then(|| PoolKey {
            venue: pool_key::Venue::UniswapV2.into(),
            token0: first.0.to_vec(),
            token1: second.0.to_vec(),
            fee_pips: 3_000,
            tick_spacing: 0,
        }),
        ..PoolUpdate::default()
    }
}

#[allow(clippy::disallowed_methods)] // the test's own wall clock, as the server's
fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// A canonical block's price record for `token`, as the engine publishes it.
fn price(token: [u8; 20], symbol: &str, usd: f64, block: u64) -> PriceUpdate {
    PriceUpdate {
        chain_id: 8453,
        token: token.to_vec(),
        block_number: block,
        block_timestamp: now_s(),
        price_usd: usd,
        metadata: Some(TokenMetadata {
            symbol: Some(symbol.into()),
            decimals: Some(18),
            ..TokenMetadata::default()
        }),
        ..PriceUpdate::default()
    }
}

async fn server(demo: Demo) -> (Arc<Shared>, String) {
    let shared = Shared::with_demo(api::DiscoveryConfig::default(), None, demo);
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for update in [
        pair(
            MEME_WETH,
            (MEME, 100_000_000 * e(18)),
            (WETH, 400 * e(18)),
            true,
        ),
        pair(
            [0xc1; 20],
            (WETH, 40_000 * e(18)),
            (USDC, 100_000_000 * e(6)),
            true,
        ),
    ] {
        shared.apply(engine::POOL_UPDATES_TOPIC, &update.encode_to_vec());
    }
    feed_price(&shared, WETH, "WETH", 2_500.0, 100).await;
    feed_price(&shared, MEME, "MEME", 0.01, 100).await;
    (shared, address)
}

/// One `prices.base` record, then the firings it caused, as the consumer runs them.
async fn feed_price(shared: &Arc<Shared>, token: [u8; 20], symbol: &str, usd: f64, block: u64) {
    shared.apply(
        engine::PRICES_TOPIC,
        &price(token, symbol, usd, block).encode_to_vec(),
    );
    shared.run_firings().await;
}

async fn http(
    address: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

/// A guest who bought MEME with 500 USDC, and a stop on all of it 20% below their entry.
async fn guest_with_a_stop(address: &str, id: &str) -> (String, Order) {
    let (_, body) = http(address, "POST", "/v1/session", None, "").await;
    let token = serde_json::from_str::<Session>(&body)
        .unwrap()
        .session_token;
    let quote = QuoteRequest {
        client_request_id: format!("{id}-buy"),
        chain_id: 8453,
        token_in: hex(&USDC),
        token_out: hex(&MEME),
        amount_in: "500".into(),
        slippage_bps: None,
    };
    let (_, body) = http(
        address,
        "POST",
        "/v1/quotes",
        Some(&token),
        &serde_json::to_string(&quote).unwrap(),
    )
    .await;
    let quote: Quote = serde_json::from_str(&body).unwrap();
    let trade = TradeRequest {
        client_request_id: format!("{id}-buy"),
        quote_id: quote.quote_id,
        intent_signature: String::new(),
    };
    let (code, body) = http(
        address,
        "POST",
        "/v1/trades",
        Some(&token),
        &serde_json::to_string(&trade).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let place = PlaceOrderRequest {
        client_request_id: format!("{id}-stop"),
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
        address,
        "POST",
        "/v1/orders",
        Some(&token),
        &serde_json::to_string(&place).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    (token, serde_json::from_str(&body).unwrap())
}

async fn order(address: &str, token: &str) -> Order {
    let (_, body) = http(address, "GET", "/v1/orders", Some(token), "").await;
    serde_json::from_str::<OrderList>(&body)
        .unwrap()
        .orders
        .remove(0)
}

/// MEME's price per canonical block from 101: down through the stop at about $0.0081, on
/// down, and back up.
const PATH: [f64; 6] = [0.0095, 0.0088, 0.0080, 0.0070, 0.0060, 0.0090];

#[tokio::test]
async fn a_stop_fires_once_on_a_recorded_price_path_and_sells_the_position() {
    let (shared, address) = server(Demo::new(Sessions::new(b"s"), None)).await;
    let (token, placed) = guest_with_a_stop(&address, "g").await;
    // $500 for 49,179.005 MEME: entry $0.0101669; 20% below is $0.00813.
    let level: f64 = placed.level_price_usd.parse().unwrap();
    assert!((0.00813..0.00814).contains(&level), "{level}");
    assert!(placed.exit_guarantee, "stops get out by default (D60)");

    for (i, usd) in PATH.iter().enumerate() {
        feed_price(&shared, MEME, "MEME", *usd, 101 + i as u64).await;
        let state = order(&address, &token).await.status.unwrap();
        let expected = if i < 2 {
            OrderState::Open
        } else {
            OrderState::Filled
        };
        assert_eq!(state.state(), expected, "after {usd}: {}", state.detail);
    }
    let fired = order(&address, &token).await.status.unwrap();
    assert_eq!(fired.firing_count, 1);
    assert!(fired.detail.starts_with("trade "), "{}", fired.detail);

    // The position is gone, sold through a shadow trade carrying the firing.
    let (_, body) = http(&address, "GET", "/v1/positions", Some(&token), "").await;
    let positions: PositionList = serde_json::from_str(&body).unwrap();
    assert!(positions.positions[0].closed);
    let (_, body) = http(&address, "GET", "/v1/history", Some(&token), "").await;
    let history: TradeHistory = serde_json::from_str(&body).unwrap();
    assert_eq!(history.receipts.len(), 2);
    assert_eq!(history.receipts[0].firing_id, fired.last_firing_id);
}

#[tokio::test]
async fn only_a_canonical_price_fires_an_order() {
    let (shared, address) = server(Demo::new(Sessions::new(b"s"), None)).await;
    let (token, _) = guest_with_a_stop(&address, "p").await;
    // A pool update that halves MEME's price in its pool, and a swap at that price, but no
    // price record for the block: nothing fires. Triggers read `prices.base` alone, which
    // the engine publishes per canonical block (D77).
    shared.apply(
        engine::POOL_UPDATES_TOPIC,
        &pair(
            MEME_WETH,
            (MEME, 200_000_000 * e(18)),
            (WETH, 400 * e(18)),
            false,
        )
        .encode_to_vec(),
    );
    shared.run_firings().await;
    assert_eq!(
        order(&address, &token).await.status.unwrap().state(),
        OrderState::Open
    );
    // The block's price record does.
    feed_price(&shared, MEME, "MEME", 0.005, 102).await;
    assert_eq!(
        order(&address, &token).await.status.unwrap().state(),
        OrderState::Filled
    );
}

#[tokio::test]
async fn a_cancelled_order_never_fires_and_another_account_cant_touch_it() {
    let (shared, address) = server(Demo::new(Sessions::new(b"s"), None)).await;
    let (token, placed) = guest_with_a_stop(&address, "c").await;
    let (_, other) = http(&address, "POST", "/v1/session", None, "").await;
    let other = serde_json::from_str::<Session>(&other)
        .unwrap()
        .session_token;
    let path = format!("/v1/orders/{}", placed.order_id);
    assert_eq!(
        http(&address, "DELETE", &path, Some(&other), "").await.0,
        404
    );
    let (code, body) = http(&address, "DELETE", &path, Some(&token), "").await;
    assert_eq!(code, 200, "{body}");
    feed_price(&shared, MEME, "MEME", 0.001, 101).await;
    let state = order(&address, &token).await.status.unwrap();
    assert_eq!(
        (state.state(), state.firing_count),
        (OrderState::Cancelled, 0)
    );
    assert_eq!(
        http(&address, "DELETE", &path, Some(&token), "").await.0,
        409
    );
}

/// Needs Postgres: OMNIMARKET_TEST_POSTGRES=postgres://… (CI's service container).
#[tokio::test]
#[ignore = "needs Postgres: set OMNIMARKET_TEST_POSTGRES, then --ignored"]
async fn a_restart_mid_path_neither_fires_the_stop_again_nor_skips_it() {
    let url = std::env::var("OMNIMARKET_TEST_POSTGRES").expect("OMNIMARKET_TEST_POSTGRES");
    let restart = || async {
        let store = Store::connect(&url).await.unwrap();
        server(
            Demo::new(Sessions::new(b"s"), None)
                .with_store(store)
                .await
                .unwrap(),
        )
        .await
    };
    let (shared, address) = restart().await;
    let (token, _) = guest_with_a_stop(&address, &format!("r{}", now_s())).await;
    // Down to just above the stop, then a restart.
    for (i, usd) in PATH[..2].iter().enumerate() {
        feed_price(&shared, MEME, "MEME", *usd, 101 + i as u64).await;
    }
    drop(shared);
    let (shared, address) = restart().await;
    assert_eq!(
        order(&address, &token).await.status.unwrap().state(),
        OrderState::Open
    );
    // The rest of the path: it fires once, at the crossing.
    for (i, usd) in PATH[2..].iter().enumerate() {
        feed_price(&shared, MEME, "MEME", *usd, 103 + i as u64).await;
    }
    let fired = order(&address, &token).await.status.unwrap();
    assert_eq!((fired.state(), fired.firing_count), (OrderState::Filled, 1));
    // And after another restart, the same path again fires nothing more.
    drop(shared);
    let (shared, address) = restart().await;
    for (i, usd) in PATH.iter().enumerate() {
        feed_price(&shared, MEME, "MEME", *usd, 110 + i as u64).await;
    }
    let after = order(&address, &token).await.status.unwrap();
    assert_eq!((after.state(), after.firing_count), (OrderState::Filled, 1));
    let (_, body) = http(&address, "GET", "/v1/history", Some(&token), "").await;
    assert_eq!(
        serde_json::from_str::<TradeHistory>(&body)
            .unwrap()
            .receipts
            .len(),
        2
    );
}

fn limit_buy(id: usize) -> PlaceOrderRequest {
    PlaceOrderRequest {
        client_request_id: format!("b{id}"),
        chain_id: 8453,
        token: hex(&MEME),
        kind: OrderKind::LimitBuy.into(),
        level: Some(OrderLevel {
            level: Some(Level::PriceUsd("0.005".into())),
        }),
        spend_amount: "1".into(),
        spend_token: hex(&USDC),
        ..PlaceOrderRequest::default()
    }
}

fn token_ref(address: [u8; 20], symbol: &str, decimals: u32) -> proto::api::v1::TokenRef {
    proto::api::v1::TokenRef {
        chain_id: 8453,
        address: hex(&address),
        symbol: symbol.into(),
        decimals,
        ..Default::default()
    }
}

#[test]
fn an_account_holds_a_bounded_number_of_orders() {
    use proto::api::v1::account::Kind;
    let account = api::AccountState::new("guest-1".into(), Kind::Guest, None, 0);
    let mut orders = api::Orders::default();
    let place = |orders: &mut api::Orders, i: usize| {
        orders.place(
            &account,
            &limit_buy(i),
            format!("o{i}"),
            token_ref(MEME, "MEME", 18),
            Some(token_ref(USDC, "USDC", 6)),
            0,
        )
    };
    // 50 open; cancelling them makes room for more, up to 500 in all.
    for i in 0..api::MAX_ORDERS {
        place(&mut orders, i).unwrap();
        if i % api::MAX_OPEN_ORDERS == api::MAX_OPEN_ORDERS - 1 {
            for j in i + 1 - api::MAX_OPEN_ORDERS..=i {
                orders.cancel("guest-1", &format!("o{j}"), 0).unwrap();
            }
        }
    }
    assert_eq!(
        place(&mut orders, api::MAX_ORDERS),
        Err(api::OrderError::TooMany)
    );
    // And 50 open at once is the limit, whatever the total.
    let mut orders = api::Orders::default();
    for i in 0..api::MAX_OPEN_ORDERS {
        place(&mut orders, i).unwrap();
    }
    assert_eq!(place(&mut orders, 99), Err(api::OrderError::TooMany));
}

#[test]
fn a_wallet_accounts_order_waits_for_signed_arming() {
    use proto::api::v1::account::Kind;
    let account = api::AccountState::new(
        "privy-1".into(),
        Kind::Privy,
        Some(types::chain::Address::repeat_byte(7)),
        0,
    );
    let refused = api::Orders::default().place(
        &account,
        &limit_buy(0),
        "o".into(),
        token_ref(MEME, "MEME", 18),
        Some(token_ref(USDC, "USDC", 6)),
        0,
    );
    assert_eq!(refused, Err(api::OrderError::NeedsArming));
}

/// A guest holding 50,000 MEME bought for $500.
fn holder() -> api::AccountState {
    use proto::api::v1::account::Kind;
    let mut account = api::AccountState::new("guest-h".into(), Kind::Guest, None, 0);
    account
        .apply(api::Fill {
            trade_id: "buy".into(),
            token_in: token_ref(USDC, "USDC", 6),
            amount_in: U256::from(500_000_000u64),
            token_out: token_ref(MEME, "MEME", 18),
            amount_out: U256::from(50_000u64) * U256::from(10u64).pow(U256::from(18u64)),
            value_usd: 500.0,
            at_ms: 0,
            receipt: None,
        })
        .unwrap();
    account
}

fn exit(id: &str, kind: OrderKind, pct: &str) -> PlaceOrderRequest {
    PlaceOrderRequest {
        client_request_id: id.into(),
        chain_id: 8453,
        token: hex(&MEME),
        kind: kind.into(),
        level: Some(OrderLevel {
            level: Some(Level::PriceUsd("0.008".into())),
        }),
        sell_pct: pct.into(),
        ..PlaceOrderRequest::default()
    }
}

fn place(
    orders: &mut api::Orders,
    account: &api::AccountState,
    request: &PlaceOrderRequest,
    spend: [u8; 20],
) -> Result<Order, api::OrderError> {
    orders.place(
        account,
        request,
        request.client_request_id.clone(),
        token_ref(MEME, "MEME", 18),
        Some(token_ref(spend, "SPEND", 18)),
        0,
    )
}

#[test]
fn an_order_is_refused_unless_its_terms_hold() {
    use api::OrderError::Bad;
    let account = holder();
    let mut orders = api::Orders::default();
    // A share of the position in (0, 100].
    for pct in ["0", "100.01", "-5", "x"] {
        assert_eq!(
            place(
                &mut orders,
                &account,
                &exit(pct, OrderKind::StopLoss, pct),
                USDC
            ),
            Err(Bad("sell_pct")),
            "{pct}"
        );
    }
    assert!(
        place(
            &mut orders,
            &account,
            &exit("all", OrderKind::StopLoss, "100"),
            USDC
        )
        .is_ok()
    );
    // A limit buy spends a positive amount of a quote asset.
    for (amount, spend) in [("0", USDC), ("-1", USDC), ("10", MEME)] {
        let request = PlaceOrderRequest {
            spend_amount: amount.into(),
            ..limit_buy(7)
        };
        assert_eq!(
            place(&mut orders, &account, &request, spend),
            Err(Bad("spend_amount"))
        );
    }
    // A level is a positive price.
    for level in ["0", "-1", "x"] {
        let request = PlaceOrderRequest {
            level: Some(OrderLevel {
                level: Some(Level::PriceUsd(level.into())),
            }),
            ..exit("l", OrderKind::TakeProfit, "50")
        };
        assert_eq!(
            place(&mut orders, &account, &request, USDC),
            Err(Bad("level"))
        );
    }
    // Selling from a position that isn't there.
    let empty = api::AccountState::new(
        "guest-e".into(),
        proto::api::v1::account::Kind::Guest,
        None,
        0,
    );
    assert_eq!(
        place(
            &mut orders,
            &empty,
            &exit("n", OrderKind::StopLoss, "50"),
            USDC
        ),
        Err(api::OrderError::NoPosition)
    );
}

#[test]
fn only_a_stop_gets_out_by_default() {
    let account = holder();
    let mut orders = api::Orders::default();
    let stop = place(
        &mut orders,
        &account,
        &exit("s", OrderKind::StopLoss, "50"),
        USDC,
    )
    .unwrap();
    let take = place(
        &mut orders,
        &account,
        &exit("t", OrderKind::TakeProfit, "50"),
        USDC,
    )
    .unwrap();
    let buy = place(&mut orders, &account, &limit_buy(1), USDC).unwrap();
    assert_eq!(
        (stop.exit_guarantee, take.exit_guarantee, buy.exit_guarantee),
        (true, false, false)
    );
    // The firing reads the same.
    assert!(orders.exit_guarantee("s"));
    assert!(!orders.exit_guarantee("t"));
    assert!(!orders.exit_guarantee("nope"));
    // Set either way, it's kept.
    let off = PlaceOrderRequest {
        exit_guarantee: Some(false),
        ..exit("s2", OrderKind::StopLoss, "50")
    };
    assert!(
        !place(&mut orders, &account, &off, USDC)
            .unwrap()
            .exit_guarantee
    );
}

#[test]
fn an_edit_moves_an_open_order_and_a_done_one_refuses() {
    let account = holder();
    let mut orders = api::Orders::default();
    place(
        &mut orders,
        &account,
        &exit("e", OrderKind::StopLoss, "50"),
        USDC,
    )
    .unwrap();
    let edited = orders
        .update(
            &account,
            &proto::api::v1::UpdateOrderRequest {
                order_id: "e".into(),
                level: Some(OrderLevel {
                    level: Some(Level::PriceUsd("0.007".into())),
                }),
                sell_pct: Some("25".into()),
                exit_guarantee: Some(false),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    assert_eq!(
        (
            edited.level_price_usd.as_str(),
            edited.sell_pct.as_str(),
            edited.exit_guarantee
        ),
        ("0.007", "25", false)
    );
    assert_eq!(edited.status.unwrap().updated_at_ms, 5);
    orders.finish("e", triggers::State::Filled, "trade t", 6);
    assert_eq!(
        orders.update(
            &account,
            &proto::api::v1::UpdateOrderRequest {
                order_id: "e".into(),
                ..Default::default()
            },
            7,
        ),
        Err(api::OrderError::NotOpen)
    );
    assert_eq!(
        orders.update(
            &account,
            &proto::api::v1::UpdateOrderRequest {
                order_id: "nope".into(),
                ..Default::default()
            },
            7,
        ),
        Err(api::OrderError::UnknownOrder)
    );
}
