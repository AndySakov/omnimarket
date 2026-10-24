//! Quotes (#83) on pool state mirrored from `pool-updates.base`: the best of the direct pools
//! and two hops through WETH or USDC, net of our 1% fee (D28), with D27's slippage defaults.

use std::sync::Arc;

use alloy_primitives::U256;
use api::server::{Shared, router};
use api::{Quotes, hex};
use prost::Message as _;
use proto::api::v1::{Quote, QuoteRequest};
use proto::lineage::v1::Lineage;
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
/// MEME/USDC, shallow: $10,000 a side.
const DIRECT: [u8; 20] = [0xa1; 20];
/// MEME/WETH and WETH/USDC, deep.
const MEME_WETH: [u8; 20] = [0xb1; 20];
const WETH_USDC: [u8; 20] = [0xc1; 20];

fn e(n: u32) -> u128 {
    10u128.pow(n)
}

fn be(n: u128) -> Vec<u8> {
    U256::from(n).to_be_bytes_trimmed_vec()
}

/// A v2 pair's discovery at block 100, with token0 the lower address.
fn pair(pool: [u8; 20], a: ([u8; 20], u128), b: ([u8; 20], u128)) -> PoolUpdate {
    let (first, second) = if a.0 < b.0 { (a, b) } else { (b, a) };
    PoolUpdate {
        lineage: Some(Lineage {
            id: vec![pool[0]; 16],
            caused_by: vec![],
        }),
        chain_id: 8453,
        pool: pool.to_vec(),
        block_number: 100,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: be(first.1),
                reserve1: be(second.1),
            })),
        }),
        key: Some(PoolKey {
            venue: pool_key::Venue::UniswapV2.into(),
            token0: first.0.to_vec(),
            token1: second.0.to_vec(),
            fee_pips: 3_000,
            tick_spacing: 0,
        }),
        ..PoolUpdate::default()
    }
}

fn price(token: [u8; 20], symbol: &str, decimals: u32, usd: f64, thin: bool) -> PriceUpdate {
    PriceUpdate {
        chain_id: 8453,
        token: token.to_vec(),
        block_number: 100,
        block_timestamp: 1_790_000_000,
        price_usd: usd,
        thin,
        metadata: Some(TokenMetadata {
            symbol: Some(symbol.into()),
            decimals: Some(decimals),
            ..TokenMetadata::default()
        }),
        ..PriceUpdate::default()
    }
}

/// MEME at $0.01 everywhere, WETH at $2,500.
fn quotes(meme_thin: bool) -> Quotes {
    let mut quotes = Quotes::default();
    for update in [
        pair(DIRECT, (MEME, 1_000_000 * e(18)), (USDC, 10_000 * e(6))),
        pair(MEME_WETH, (MEME, 100_000_000 * e(18)), (WETH, 400 * e(18))),
        pair(
            WETH_USDC,
            (WETH, 40_000 * e(18)),
            (USDC, 100_000_000 * e(6)),
        ),
    ] {
        quotes.apply_pool_update(&update);
    }
    quotes.apply_price(&price(WETH, "WETH", 18, 2_500.0, false));
    quotes.apply_price(&price(MEME, "MEME", 18, 0.01, meme_thin));
    quotes
}

fn request(id: &str, token_in: [u8; 20], token_out: [u8; 20], amount: &str) -> QuoteRequest {
    QuoteRequest {
        client_request_id: id.into(),
        chain_id: 8453,
        token_in: hex(&token_in),
        token_out: hex(&token_out),
        amount_in: amount.into(),
        slippage_bps: None,
    }
}

fn route(quote: &Quote) -> Vec<(String, String, String)> {
    quote
        .route
        .iter()
        .map(|l| (l.pool.clone(), l.amount_in.clone(), l.amount_out.clone()))
        .collect()
}

#[test]
fn a_buy_through_weth_beats_the_shallow_direct_pool_and_pays_the_fee_from_its_input() {
    let quote = quotes(false)
        .quote(&request("buy", USDC, MEME, "5000"))
        .unwrap();
    // 4,950 USDC routed after the 50 USDC fee. Direct would pay 330,438.59 MEME; through WETH,
    // 1.97 WETH and then 489,601.28 MEME (v2's 0.3% at each hop, rounded down as the pairs do).
    assert_eq!(
        route(&quote),
        vec![
            (
                hex(&WETH_USDC),
                "4950".into(),
                "1.973962581985635135".into()
            ),
            (
                hex(&MEME_WETH),
                "1.973962581985635135".into(),
                "489601.285425744753500141".into()
            ),
        ]
    );
    assert_eq!(quote.amount_in, "5000");
    assert_eq!(quote.amount_out, "489601.285425744753500141");
    let fee = quote.fee.as_ref().unwrap();
    assert_eq!(
        (
            fee.fee_bps,
            fee.amount.as_str(),
            fee.token.as_ref().unwrap().symbol.as_str()
        ),
        (100, "50", "USDC")
    );
    // Established (not thin): 3%.
    let slippage = quote.slippage.as_ref().unwrap();
    assert_eq!(
        (slippage.bps, slippage.reason.as_str(), slippage.user_set),
        (300, "established pair: 3%", false)
    );
    assert_eq!(quote.min_amount_out, "474913.246862972410895136");
    assert_eq!(quote.block_number, 100);
    assert_eq!(quote.expires_at_ms, 1_790_000_004_000);
    assert_eq!(quote.route[1].venue, "uniswap-v2");
    assert_eq!(quote.route[1].pool_fee_bps, 30);
}

#[test]
fn a_sell_pays_the_fee_from_its_proceeds() {
    let quote = quotes(true)
        .quote(&request("sell", MEME, USDC, "100000"))
        .unwrap();
    // Through WETH, 993.009099 USDC; less 9.930090 USDC.
    assert_eq!(route(&quote).len(), 2);
    assert_eq!(quote.amount_out, "983.079009");
    let fee = quote.fee.as_ref().unwrap();
    assert_eq!(
        (fee.amount.as_str(), fee.value_usd.as_str()),
        ("9.93009", "9.93009")
    );
    // A thin token: 15%.
    let slippage = quote.slippage.as_ref().unwrap();
    assert_eq!(
        (slippage.bps, slippage.reason.as_str()),
        (1500, "new or thin pair: 15%")
    );
    assert_eq!(quote.min_amount_out, "835.617157");
}

#[test]
fn a_repeated_request_gets_the_same_quote_and_a_set_slippage_is_kept() {
    let mut quotes = quotes(false);
    let mut asked = request("same", USDC, MEME, "5000");
    asked.slippage_bps = Some(75);
    let first = quotes.quote(&asked).unwrap();
    // The pools move; the same client request ID still gets the first quote.
    quotes.apply_pool_update(&PoolUpdate {
        key: None,
        block_number: 101,
        ..pair(DIRECT, (MEME, 10 * e(18)), (USDC, 10_000 * e(6)))
    });
    assert_eq!(quotes.quote(&asked).unwrap(), first);
    let slippage = first.slippage.unwrap();
    assert_eq!(
        (slippage.bps, slippage.reason.as_str(), slippage.user_set),
        (75, "set by you: 0.75%", true)
    );
    assert_eq!(first.quote_id.len(), 34);
    // Another request reusing the ID gets its own quote, not the first one.
    let other = quotes.quote(&request("same", USDC, MEME, "10")).unwrap();
    assert_eq!(other.amount_in, "10");
    // Once the quote has expired, a repeat is quoted afresh, on the moved pools.
    quotes.apply_price(&PriceUpdate {
        block_timestamp: 1_790_000_010,
        ..price(WETH, "WETH", 18, 2_500.0, false)
    });
    asked.client_request_id = "later".into();
    let fresh = quotes.quote(&asked).unwrap();
    assert_eq!(fresh.expires_at_ms, 1_790_000_014_000);
    assert_eq!(
        quotes
            .quote(&request(&"x".repeat(65), USDC, MEME, "1"))
            .unwrap_err(),
        api::QuoteError::Bad("client_request_id")
    );
}

#[test]
fn a_pair_without_a_quote_asset_or_a_route_is_refused() {
    let mut quotes = quotes(false);
    let other = [0x50; 20];
    assert_eq!(
        quotes.quote(&request("a", MEME, other, "1")).unwrap_err(),
        api::QuoteError::UnknownToken
    );
    quotes.apply_price(&price(other, "OTHER", 18, 1.0, false));
    assert_eq!(
        quotes.quote(&request("b", MEME, other, "1")).unwrap_err(),
        api::QuoteError::NoQuoteAsset
    );
    assert_eq!(
        quotes.quote(&request("c", other, WETH, "1")).unwrap_err(),
        api::QuoteError::NoRoute
    );
    assert_eq!(
        quotes
            .quote(&request("d", USDC, MEME, "1.0000001"))
            .unwrap_err(),
        api::QuoteError::Bad("amount_in")
    );
}

async fn http_post(address: &str, path: &str, body: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn rest_quotes_on_the_pool_updates_topic() {
    let shared: Arc<Shared> = Shared::new();
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for update in [
        pair(MEME_WETH, (MEME, 100_000_000 * e(18)), (WETH, 400 * e(18))),
        pair(
            WETH_USDC,
            (WETH, 40_000 * e(18)),
            (USDC, 100_000_000 * e(6)),
        ),
    ] {
        shared.apply(engine::POOL_UPDATES_TOPIC, &update.encode_to_vec());
    }
    shared.apply(
        engine::PRICES_TOPIC,
        &price(MEME, "MEME", 18, 0.01, false).encode_to_vec(),
    );

    let body = serde_json::to_string(&request("http", USDC, MEME, "5000")).unwrap();
    let ok = http_post(&address, "/v1/quotes", &body).await;
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    let quote: Quote = serde_json::from_str(ok.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(quote.amount_out, "489601.285425744753500141");

    let bad = http_post(&address, "/v1/quotes", "{\"tokenIn\": 7}").await;
    assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");
    let other = serde_json::to_string(&request("x", MEME, [0x50; 20], "1")).unwrap();
    let refused = http_post(&address, "/v1/quotes", &other).await;
    assert!(refused.starts_with("HTTP/1.1 422"), "{refused}");
}
