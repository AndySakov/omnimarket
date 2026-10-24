//! A stop firing on live Base in shadow (#86), timed from the price record to the receipt: the
//! USDC/cbBTC 0.01% pool's state read at a recent block, a guest's buy of cbBTC, a stop just
//! above the price, and that block's price record, simulated through Uniswap's quoter at that
//! block. Needs the network: `cargo test -p api --test live_trigger -- --ignored --nocapture`.

use alloy_primitives::U256;
use api::hex;
use api::server::{Shared, router};
use prost::Message as _;
use proto::api::v1::order_level::Level;
use proto::api::v1::order_status::State as OrderState;
use proto::api::v1::{
    OrderKind, OrderLevel, OrderList, PlaceOrderRequest, Quote, QuoteRequest, Session,
    TradeHistory, TradeRequest,
};
use proto::pool::v1::{PoolKey, PoolState, PoolUpdate, V3State, V3Tick, pool_key, pool_state};
use proto::price::v1::{PriceUpdate, TokenMetadata};
use types::chain::{Address, Bytes, CallResult, EthCall};
use venues::v3::{self, BASE};

const POOL: Address = alloy_primitives::address!("E9e25E35aa99A2A60155010802b81A25C45bA185");
const USDC: Address = alloy_primitives::address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
const CBBTC: Address = alloy_primitives::address!("cbB7C0000aB88B473b1f5aFd9ef808440eed33Bf");

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

#[allow(clippy::disallowed_methods)] // the measurement's own wall clock
fn now() -> std::time::Instant {
    std::time::Instant::now()
}

#[allow(clippy::disallowed_methods)]
fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the network: cargo test -p api --test live_trigger -- --ignored --nocapture"]
async fn a_stop_fires_on_live_base_state_and_its_time_is_measured() {
    let url = std::env::var("BASE_RPC").unwrap_or_else(|_| "https://mainnet.base.org".into());
    let worker = {
        let url = url.clone();
        tokio::task::spawn_blocking(move || chain_io::spawn_call_worker(&url, 5).unwrap())
            .await
            .unwrap()
    };
    let rpc = det::ChannelRpc::new(worker.requests.clone());
    let call = move |data: Vec<u8>, to: Address, block: u64| {
        let answer = futures::executor::block_on(det::Rpc::call(
            &rpc,
            EthCall {
                to,
                data: Bytes::from(data),
                block,
            },
        ));
        match answer {
            CallResult::Returned(bytes) => bytes.to_vec(),
            CallResult::Failed(e) => panic!("{e}"),
        }
    };
    let latest = {
        let chain = chain_io::HttpChain::new(&url).unwrap();
        chain_io::ChainReader::latest_number(&chain).await.unwrap()
    };
    let block = latest - 3;
    let (price, liquidity, ticks) = tokio::task::spawn_blocking(move || {
        let price = v3::decode_slot0(&call(v3::slot0_call(), POOL, block)).unwrap();
        let liquidity = v3::decode_liquidity(&call(v3::liquidity_call(), POOL, block)).unwrap();
        let word = (price.tick >> 8) as i16;
        let mut ticks = Vec::new();
        for w in word - 2..=word + 2 {
            let answer = call(v3::populated_ticks_call(POOL, w), BASE.tick_lens, block);
            ticks.extend(v3::decode_populated_ticks(&answer).unwrap());
        }
        (price, liquidity, ticks)
    })
    .await
    .unwrap();
    // USDC (6 decimals) is token0, cbBTC (8) token1: (sqrtP / 2^96)^2 is cbBTC base units per
    // USDC base unit, so a whole cbBTC is worth 100 / that, in dollars.
    let raw = (f64::from(price.sqrt_price_x96) / 2f64.powi(96)).powi(2);
    let cbbtc_usd = 100.0 / raw;

    let shared = Shared::with_calls(
        api::DiscoveryConfig::default(),
        Some(worker.requests.clone()),
    );
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    shared.apply(
        engine::POOL_UPDATES_TOPIC,
        &PoolUpdate {
            chain_id: 8453,
            pool: POOL.to_vec(),
            block_number: block,
            after: Some(PoolState {
                state: Some(pool_state::State::V3(V3State {
                    initialized: true,
                    sqrt_price_x96: price.sqrt_price_x96.to_be_bytes_trimmed_vec(),
                    tick: price.tick,
                    liquidity: U256::from(liquidity).to_be_bytes_trimmed_vec(),
                    ticks: ticks
                        .iter()
                        .map(|(t, l)| V3Tick {
                            tick: *t,
                            liquidity_gross: U256::from(l.gross).to_be_bytes_trimmed_vec(),
                            liquidity_net: l.net.to_be_bytes().to_vec(),
                        })
                        .collect(),
                })),
            }),
            key: Some(PoolKey {
                venue: pool_key::Venue::UniswapV3.into(),
                token0: USDC.to_vec(),
                token1: CBBTC.to_vec(),
                fee_pips: 100,
                tick_spacing: 1,
            }),
            ..PoolUpdate::default()
        }
        .encode_to_vec(),
    );
    let price_record = PriceUpdate {
        chain_id: 8453,
        token: CBBTC.to_vec(),
        block_number: block,
        block_timestamp: now_s(),
        price_usd: cbbtc_usd,
        metadata: Some(TokenMetadata {
            symbol: Some("cbBTC".into()),
            decimals: Some(8),
            ..TokenMetadata::default()
        }),
        ..PriceUpdate::default()
    };
    shared.apply(engine::PRICES_TOPIC, &price_record.encode_to_vec());

    // A guest buys $100 of cbBTC, then sets a stop 1% above the price, so the block's price
    // record crosses it.
    let (_, body) = http(&address, "POST", "/v1/session", None, "").await;
    let token = serde_json::from_str::<Session>(&body)
        .unwrap()
        .session_token;
    let quote = QuoteRequest {
        client_request_id: "live-buy".into(),
        chain_id: 8453,
        token_in: hex(USDC.as_slice()),
        token_out: hex(CBBTC.as_slice()),
        amount_in: "100".into(),
        slippage_bps: None,
    };
    let (_, body) = http(
        &address,
        "POST",
        "/v1/quotes",
        Some(&token),
        &serde_json::to_string(&quote).unwrap(),
    )
    .await;
    let quote: Quote = serde_json::from_str(&body).unwrap();
    let trade = TradeRequest {
        client_request_id: "live-buy".into(),
        quote_id: quote.quote_id,
        intent_signature: String::new(),
    };
    let (code, body) = http(
        &address,
        "POST",
        "/v1/trades",
        Some(&token),
        &serde_json::to_string(&trade).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let place = PlaceOrderRequest {
        client_request_id: "live-stop".into(),
        chain_id: 8453,
        token: hex(CBBTC.as_slice()),
        kind: OrderKind::StopLoss.into(),
        level: Some(OrderLevel {
            level: Some(Level::PriceUsd(format!("{}", cbbtc_usd * 1.01))),
        }),
        sell_pct: "100".into(),
        ..PlaceOrderRequest::default()
    };
    let (code, body) = http(
        &address,
        "POST",
        "/v1/orders",
        Some(&token),
        &serde_json::to_string(&place).unwrap(),
    )
    .await;
    assert_eq!(code, 200, "{body}");

    // The price record arrives: from here to the receipt is the trigger's path.
    let started = now();
    shared.apply(engine::PRICES_TOPIC, &price_record.encode_to_vec());
    let evaluated = started.elapsed();
    shared.run_firings().await;
    let receipted = started.elapsed();

    let (_, body) = http(&address, "GET", "/v1/orders", Some(&token), "").await;
    let status = serde_json::from_str::<OrderList>(&body)
        .unwrap()
        .orders
        .remove(0)
        .status
        .unwrap();
    assert_eq!(status.state(), OrderState::Filled, "{}", status.detail);
    let (_, body) = http(&address, "GET", "/v1/history", Some(&token), "").await;
    let receipt = serde_json::from_str::<TradeHistory>(&body)
        .unwrap()
        .receipts
        .remove(0);
    assert!(!receipt.simulated_in_memory);
    println!(
        "block {block}, cbBTC ${cbbtc_usd:.2}: stop at ${:.2} fired; price record applied and \
         checked in {evaluated:?}, receipt after {receipted:?}; sold {} cbBTC for {} USDC; steps (ms) {:?}",
        cbbtc_usd * 1.01,
        receipt.amount_in,
        receipt.amount_out,
        receipt
            .steps
            .iter()
            .map(|s| s.finished_at_ms - s.started_at_ms)
            .collect::<Vec<_>>(),
    );
}
