//! The engine's status (#79) through the API: `GET /v1/status` and the `status` topic serve the
//! latest `status.base` record, in the contract's shape.

use std::sync::Arc;

use api::server::{Shared, router};
use api::{Feed, Session, to_json};
use prost::Message as _;
use proto::lineage::v1::Lineage;
use proto::status::v1::EngineStatus;
use proto::status::v1::engine_status::Mode;

fn status(block: u64) -> EngineStatus {
    EngineStatus {
        lineage: Some(Lineage {
            id: vec![block as u8; 16],
            caused_by: vec![],
        }),
        chain_id: 8453,
        block_number: block,
        block_hash: vec![0xab; 32],
        block_timestamp: 1_790_000_000 + 2 * block,
        lag_blocks: Some(1),
        lag_ms: 2_310,
        v2_pools: 1_312,
        v3_pools: 904,
        shadow_checks: 5_120,
        shadow_check_mismatches: 3,
        uptime_ms: 5_400_000,
        mode: Mode::Live.into(),
        recording: true,
        core_instance: "base-00000000000000ff".into(),
    }
}

const HASH_HEX: &str = "0xabababababababababababababababababababababababababababababababab";

/// What the contract's `EngineStatus` for `status(block)` looks like in proto3 JSON.
fn wire(block: u64) -> String {
    format!(
        concat!(
            r#"{{"lineage":{{"id":"{id}"}},"chainId":"8453","mode":"MODE_LIVE","#,
            r#""headBlockNumber":"{block}","headBlockHash":"{hash}","#,
            r#""headBlockTimeMs":"{time}","lagBlocks":"1","lagMs":"2310","#,
            r#""pools":[{{"venue":"uniswap-v2","known":"1312","active":"1312"}},"#,
            r#"{{"venue":"uniswap-v3","known":"904","active":"904"}}],"#,
            r#""shadowChecks":"5120","shadowCheckMismatches":"3","recording":true,"#,
            r#""coreInstance":"base-00000000000000ff","uptimeMs":"5400000"}}"#
        ),
        id = base64(&[block as u8; 16]),
        block = block,
        hash = HASH_HEX,
        time = (1_790_000_000 + 2 * block) * 1000,
    )
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
        out.push_str(&"=".repeat(3 - chunk.len()));
    }
    out
}

#[test]
fn a_status_record_becomes_the_contracts_engine_status() {
    let mut feed = Feed::default();
    assert_eq!(feed.status(), None);
    feed.apply_status(&status(7)).unwrap();
    assert_eq!(
        serde_json::to_string(feed.status().unwrap()).unwrap(),
        wire(7)
    );
}

#[test]
fn a_record_without_the_chain_head_shows_no_lag_in_blocks() {
    let mut feed = Feed::default();
    feed.apply_status(&EngineStatus {
        lag_blocks: None,
        ..status(7)
    })
    .unwrap();
    assert_eq!(feed.status().unwrap().lag_blocks, 0);
}

#[test]
fn a_record_with_a_short_hash_is_refused() {
    let mut feed = Feed::default();
    let refused = feed.apply_status(&EngineStatus {
        block_hash: vec![1, 2],
        ..status(7)
    });
    assert_eq!(refused, Err(api::ModelError::BadHash { len: 2 }));
    assert_eq!(feed.status(), None);
}

#[test]
fn a_subscriber_gets_the_latest_status_then_each_new_one() {
    let mut feed = Feed::default();
    let mut session = Session::default();
    // Before the engine's first status there's nothing to send; the first one is the snapshot.
    assert!(
        session
            .on_text(r#"{"subscribe":{"topic":"status"}}"#, &feed)
            .is_empty()
    );
    let mut out = Vec::new();
    for block in [7, 8] {
        for published in feed.apply_status(&status(block)).unwrap() {
            out.extend(session.on_published(&published, &feed));
        }
    }
    let out: Vec<String> = out.iter().map(to_json).collect();
    assert_eq!(
        out,
        [
            format!(
                r#"{{"snapshot":{{"topic":"status","status":{}}}}}"#,
                wire(7)
            ),
            format!(
                r#"{{"delta":{{"topic":"status","seq":"1","status":{}}}}}"#,
                wire(8)
            ),
        ]
    );

    // A later subscriber starts from the latest.
    let mut late = Session::default();
    let snapshot: Vec<String> = late
        .on_text(r#"{"subscribe":{"topic":"status"}}"#, &feed)
        .iter()
        .map(to_json)
        .collect();
    assert_eq!(
        snapshot,
        [format!(
            r#"{{"snapshot":{{"topic":"status","status":{}}}}}"#,
            wire(8)
        )]
    );
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
async fn rest_serves_the_latest_status_from_the_status_topic() {
    let shared: Arc<Shared> = Shared::new();
    let app = router(shared.clone(), "http://localhost:5173").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    #[allow(clippy::disallowed_methods)] // the test's own server task
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let before = http_get(&address, "/v1/status").await;
    assert!(before.starts_with("HTTP/1.1 503"), "{before}");
    shared.apply(engine::STATUS_TOPIC, &status(7).encode_to_vec());
    shared.apply(engine::STATUS_TOPIC, &status(8).encode_to_vec());
    let after = http_get(&address, "/v1/status").await;
    assert!(after.starts_with("HTTP/1.1 200"), "{after}");
    assert!(after.ends_with(&wire(8)), "{after}");
}
