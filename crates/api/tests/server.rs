//! The server end to end, in process: REST, `/health` and the WebSocket stream, fed the way
//! the Kafka consumer feeds it.

use std::sync::Arc;

use api::server::{Shared, router};
use futures::{SinkExt, StreamExt};
use prost::Message as _;
use proto::lineage::v1::Lineage;
use proto::price::v1::{PriceUpdate, TokenMetadata};
use tokio_tungstenite::tungstenite::Message;

const TOKEN_HEX: &str = "0xabababababababababababababababababababab";

fn price(block: u64, usd: f64) -> Vec<u8> {
    PriceUpdate {
        lineage: Some(Lineage {
            id: vec![block as u8; 16],
            caused_by: vec![],
        }),
        chain_id: 8453,
        token: vec![0xab; 20],
        block_number: block,
        block_timestamp: 1_700_000_000 + 2 * block,
        metadata: Some(TokenMetadata {
            symbol: Some("TKN".into()),
            ..TokenMetadata::default()
        }),
        price_usd: usd,
        ..PriceUpdate::default()
    }
    .encode_to_vec()
}

async fn start() -> (Arc<Shared>, String) {
    let shared = Shared::new();
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (shared, address)
}

async fn http_get(address: &str, path: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://localhost:5173\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn rest_serves_health_and_token_snapshots_with_cors() {
    let (shared, address) = start().await;
    assert!(http_get(&address, "/health").await.ends_with("ok"));
    let missing = http_get(&address, &format!("/v1/tokens/8453/{TOKEN_HEX}")).await;
    assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
    shared.apply_price(&price(1, 2.5));
    let found = http_get(&address, &format!("/v1/tokens/8453/{TOKEN_HEX}")).await;
    assert!(found.starts_with("HTTP/1.1 200"), "{found}");
    assert!(found.contains(r#""displayPriceUsd":"2.5""#), "{found}");
    assert!(
        found
            .to_ascii_lowercase()
            .contains("access-control-allow-origin: http://localhost:5173"),
        "{found}"
    );
    let other_chain = http_get(&address, &format!("/v1/tokens/56/{TOKEN_HEX}")).await;
    assert!(other_chain.starts_with("HTTP/1.1 404"), "{other_chain}");
}

#[tokio::test]
async fn a_websocket_subscriber_gets_a_snapshot_live_deltas_and_heartbeats() {
    let (shared, address) = start().await;
    shared.apply_price(&price(1, 2.0));
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{address}/v1/stream"))
        .await
        .unwrap();
    ws.send(Message::text(format!(
        r#"{{"subscribe":{{"topic":"token:{TOKEN_HEX}"}}}}"#
    )))
    .await
    .unwrap();

    let mut received = Vec::new();
    let mut fed = false;
    while received.len() < 4 {
        let Some(Ok(Message::Text(text))) = ws.next().await else {
            panic!("the stream ended");
        };
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        if json.get("heartbeat").is_some() {
            received.push("heartbeat".to_string());
            continue;
        }
        if let Some(snapshot) = json.get("snapshot") {
            assert_eq!(snapshot["token"]["displayPriceUsd"], "2");
            received.push("snapshot".to_string());
        }
        if let Some(delta) = json.get("delta") {
            received.push(format!("delta {}", delta["seq"].as_str().unwrap()));
        }
        if !fed {
            shared.apply_price(&price(2, 3.0));
            shared.apply_price(&price(3, 4.0));
            fed = true;
        }
    }
    let data: Vec<&String> = received.iter().filter(|m| *m != "heartbeat").collect();
    assert_eq!(data[..3], ["snapshot", "delta 1", "delta 2"]);
}

#[tokio::test]
async fn rest_serves_the_discovery_feed_built_from_all_three_topics() {
    use proto::pool::v1::{PoolState, PoolUpdate, V2Reserves, pool_state};
    use proto::trade::v1::{Trade, trade};
    let (shared, address) = start().await;
    let pool = PoolUpdate {
        chain_id: 8453,
        pool: vec![0xc1; 20],
        block_number: 1,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves::default())),
        }),
        ..PoolUpdate::default()
    };
    shared.apply(engine::POOL_UPDATES_TOPIC, &pool.encode_to_vec());
    let buy = Trade {
        chain_id: 8453,
        pool: vec![0xc1; 20],
        venue: trade::Venue::UniswapV2.into(),
        token: vec![0xab; 20],
        side: trade::Side::Buy.into(),
        block_number: 1,
        block_timestamp: 1_700_000_002,
        ..Trade::default()
    };
    shared.apply(engine::TRADES_TOPIC, &buy.encode_to_vec());
    // Block 1 is whole once every topic reaches block 2.
    let quiet_pool = PoolUpdate {
        pool: vec![0x77; 20],
        block_number: 2,
        after: Some(PoolState {
            state: Some(pool_state::State::V2(V2Reserves {
                reserve0: vec![1],
                reserve1: vec![1],
            })),
        }),
        ..pool
    };
    shared.apply(engine::POOL_UPDATES_TOPIC, &quiet_pool.encode_to_vec());
    let other_trade = Trade {
        pool: vec![0x98; 20],
        token: vec![0x99; 20],
        block_number: 2,
        ..buy
    };
    shared.apply(engine::TRADES_TOPIC, &other_trade.encode_to_vec());
    shared.apply_price(&price(2, 2.5));
    let feed = http_get(&address, "/v1/discovery?list=new&max_age_ms=60000").await;
    assert!(feed.starts_with("HTTP/1.1 200"), "{feed}");
    assert!(
        feed.contains(&format!(r#""address":"{TOKEN_HEX}""#)),
        "{feed}"
    );
    assert!(feed.contains(r#""venue":"uniswap-v2""#), "{feed}");
    assert!(feed.contains(r#""buys":"1""#), "{feed}");
    let bad = http_get(&address, "/v1/discovery?list=hot").await;
    assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");
}
