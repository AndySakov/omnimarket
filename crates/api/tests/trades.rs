//! Shadow trades' bookkeeping (#84): each intent is its own trade, a quote is gone at its
//! expiry, and a trade that never finished can run again.

use std::sync::Arc;

use alloy_primitives::U256;
use api::server::{Shared, router};
use api::{Quotes, TradeError, Trades, hex};
use prost::Message as _;
use proto::api::v1::{Quote, QuoteRequest, TradeReceipt, TradeRequest, TradeStatus};
use proto::pool::v1::{PoolKey, PoolState, PoolUpdate, V2Reserves, pool_key, pool_state};
use proto::price::v1::{PriceUpdate, TokenMetadata};

const MEME: [u8; 20] = [0x30; 20];
const USDC: [u8; 20] = [
    0x83, 0x35, 0x89, 0xfc, 0xd6, 0xed, 0xb6, 0xe0, 0x8f, 0x4c, 0x7c, 0x32, 0xd4, 0xf7, 0x1b, 0x54,
    0xbd, 0xa0, 0x29, 0x13,
];

fn be(n: u128) -> Vec<u8> {
    U256::from(n).to_be_bytes_trimmed_vec()
}

/// MEME/USDC at $0.01, a million a side.
fn pool() -> PoolUpdate {
    PoolUpdate {
        chain_id: 8453,
        pool: [0xa1; 20].to_vec(),
        block_number: 100,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: be(100_000_000 * 10u128.pow(18)),
                reserve1: be(1_000_000 * 10u128.pow(6)),
            })),
        }),
        key: Some(PoolKey {
            venue: pool_key::Venue::UniswapV2.into(),
            token0: MEME.to_vec(),
            token1: USDC.to_vec(),
            fee_pips: 3_000,
            tick_spacing: 0,
        }),
        ..PoolUpdate::default()
    }
}

fn meme_price(timestamp: u64) -> PriceUpdate {
    PriceUpdate {
        chain_id: 8453,
        token: MEME.to_vec(),
        block_number: 100,
        block_timestamp: timestamp,
        price_usd: 0.01,
        metadata: Some(TokenMetadata {
            decimals: Some(18),
            ..TokenMetadata::default()
        }),
        ..PriceUpdate::default()
    }
}

fn request(id: &str, amount: &str) -> QuoteRequest {
    QuoteRequest {
        client_request_id: id.into(),
        chain_id: 8453,
        token_in: hex(&USDC),
        token_out: hex(&MEME),
        amount_in: amount.into(),
        slippage_bps: None,
    }
}

async fn http(address: &str, method: &str, path: &str, body: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn two_quotes_make_two_trades_each_with_its_own_receipt() {
    let shared: Arc<Shared> = Shared::new();
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    shared.apply(engine::POOL_UPDATES_TOPIC, &pool().encode_to_vec());
    shared.apply(
        engine::PRICES_TOPIC,
        &meme_price(1_790_000_000).encode_to_vec(),
    );

    let mut receipts = Vec::new();
    for (id, amount) in [("one", "10"), ("two", "20")] {
        let body = serde_json::to_string(&request(id, amount)).unwrap();
        let quote: Quote =
            serde_json::from_str(&http(&address, "POST", "/v1/quotes", &body).await).unwrap();
        let trade = TradeRequest {
            client_request_id: id.into(),
            quote_id: quote.quote_id,
            intent_signature: String::new(),
        };
        let body = serde_json::to_string(&trade).unwrap();
        let status: TradeStatus =
            serde_json::from_str(&http(&address, "POST", "/v1/trades", &body).await).unwrap();
        let receipt: TradeReceipt = serde_json::from_str(
            &http(
                &address,
                "GET",
                &format!("/v1/trades/{}/receipt", status.trade_id),
                "",
            )
            .await,
        )
        .unwrap();
        receipts.push((status.trade_id, receipt.amount_in, receipt.intent_hash));
    }
    assert_ne!(receipts[0].0, receipts[1].0);
    assert_ne!(receipts[0].2, receipts[1].2);
    assert_eq!(
        (receipts[0].1.as_str(), receipts[1].1.as_str()),
        ("10", "20")
    );
}

#[test]
fn a_quote_is_gone_at_its_expiry_and_an_abandoned_trade_can_run_again() {
    let mut quotes = Quotes::default();
    quotes.apply_pool_update(&pool());
    quotes.apply_price(&meme_price(1_790_000_000));
    let quote = quotes.quote(&request("q", "10")).unwrap();
    let trade = TradeRequest {
        client_request_id: "t".into(),
        quote_id: quote.quote_id.clone(),
        intent_signature: String::new(),
    };
    let signer = types::chain::Address::repeat_byte(1);
    let mut trades = Trades::default();
    let Ok(api::Prepared::Run(run)) = trades.prepare(&quotes, &trade, signer) else {
        panic!("a fresh trade runs");
    };
    // While it runs, the same intent waits; once abandoned, it can run again.
    assert!(matches!(
        trades.prepare(&quotes, &trade, signer),
        Err(TradeError::Running)
    ));
    trades.abandon(run.hash, &run.trade_id);
    assert!(matches!(
        trades.prepare(&quotes, &trade, signer),
        Ok(api::Prepared::Run(_))
    ));
    // The quote is good until its block time reaches the expiry, and not at it.
    quotes.apply_price(&meme_price(1_790_000_003));
    assert!(quotes.issued(&quote.quote_id).is_some());
    quotes.apply_price(&meme_price(1_790_000_004));
    assert!(quotes.issued(&quote.quote_id).is_none());
    assert_eq!(
        TradeError::UnknownQuote.to_string(),
        "no unexpired quote has this ID: quote again"
    );
    assert_eq!(
        TradeError::Running.to_string(),
        "this trade is still running"
    );
}
