//! The WebSocket protocol (stream.proto): a snapshot first, then deltas numbered by seq.

use api::{Feed, Session, parse_topic, to_json};
use proto::api::v1::{ServerMessage, delta, server_message, snapshot, stream_error};
use proto::lineage::v1::Lineage;
use proto::price::v1::{PoolPrice, PriceUpdate, TokenMetadata};

const TOKEN: [u8; 20] = [0xab; 20];
const TOKEN_HEX: &str = "0xabababababababababababababababababababab";

fn price(block: u64, usd: f64) -> PriceUpdate {
    PriceUpdate {
        lineage: Some(Lineage {
            id: vec![block as u8; 16],
            caused_by: vec![],
        }),
        chain_id: 8453,
        token: TOKEN.to_vec(),
        block_number: block,
        block_hash: vec![1; 32],
        block_timestamp: 1_700_000_000 + 2 * block,
        metadata: Some(TokenMetadata {
            name: Some("Token".into()),
            symbol: Some("TKN".into()),
            decimals: Some(18),
            total_supply: Some(vec![0x0d, 0xe0, 0xb6, 0xb3, 0xa7, 0x64, 0x00, 0x00]),
        }),
        price_usd: usd,
        depth_usd: 5000.0,
        thin: false,
        main_pool: vec![0xcd; 20],
        quote_token: vec![0x42; 20],
        price_in_quote: usd / 2.0,
        fdv_usd: Some(usd),
        pools: vec![PoolPrice {
            pool: vec![0xcd; 20],
            quote_token: vec![0x42; 20],
            price_in_quote: usd / 2.0,
            price_usd: usd,
            depth_usd: 5000.0,
            counted: true,
        }],
    }
}

fn subscribe(topic: &str) -> String {
    format!(r#"{{"subscribe":{{"topic":"{topic}"}}}}"#)
}

fn seq_of(message: &ServerMessage) -> (&'static str, u64) {
    match message.kind.as_ref().unwrap() {
        server_message::Kind::Snapshot(s) => ("snapshot", s.seq),
        server_message::Kind::Delta(d) => ("delta", d.seq),
        server_message::Kind::Heartbeat(_) => ("heartbeat", 0),
        server_message::Kind::Error(_) => ("error", 0),
    }
}

#[test]
fn a_subscriber_gets_a_snapshot_then_numbered_deltas() {
    let mut feed = Feed::default();
    feed.apply_price(&price(1, 2.0)).unwrap();
    let mut session = Session::default();
    let out = session.on_text(&subscribe(&format!("token:{TOKEN_HEX}")), &feed);
    assert_eq!(out.len(), 1);
    let server_message::Kind::Snapshot(snap) = out[0].kind.as_ref().unwrap() else {
        panic!("not a snapshot: {out:?}");
    };
    let Some(snapshot::Payload::Token(token)) = &snap.payload else {
        panic!("not a token snapshot");
    };
    assert_eq!(token.display_price_usd, "2");
    assert_eq!(token.total_supply, "1");
    assert_eq!(token.token.as_ref().unwrap().symbol, "TKN");
    assert_eq!(
        token.pools[0].address,
        "0xcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
    );

    let mut seqs = Vec::new();
    for block in 2..5 {
        for p in feed.apply_price(&price(block, block as f64)).unwrap() {
            let message = session.on_published(&p, &feed).unwrap();
            seqs.push(seq_of(&message));
            let server_message::Kind::Delta(d) = message.kind.unwrap() else {
                panic!("not a delta")
            };
            let Some(delta::Payload::TokenTick(tick)) = d.payload else {
                panic!("not a tick")
            };
            assert_eq!(tick.display_price_usd, block.to_string());
            assert_eq!(tick.block_time_ms, (1_700_000_000 + 2 * block) * 1000);
        }
    }
    assert_eq!(seqs, [("delta", 1), ("delta", 2), ("delta", 3)]);
}

#[test]
fn a_token_with_no_price_yet_gets_its_snapshot_with_its_first_tick() {
    let mut feed = Feed::default();
    let mut session = Session::default();
    // Upper-case hex is the same topic.
    let topic = format!("token:{}", TOKEN_HEX.to_uppercase().replacen("0X", "0x", 1));
    assert!(session.on_text(&subscribe(&topic), &feed).is_empty());
    let published = feed.apply_price(&price(1, 2.0)).unwrap();
    let message = session.on_published(&published[0], &feed).unwrap();
    assert_eq!(seq_of(&message), ("snapshot", 0));
}

#[test]
fn unsubscribed_and_stale_ticks_are_not_sent() {
    let mut feed = Feed::default();
    let mut session = Session::default();
    let early = feed.apply_price(&price(1, 2.0)).unwrap();
    // Not subscribed yet.
    assert!(session.on_published(&early[0], &feed).is_none());
    feed.apply_price(&price(2, 3.0)).unwrap();
    session.on_text(&subscribe(&format!("token:{TOKEN_HEX}")), &feed);
    // A tick already in the snapshot (block 1 < 2) is skipped.
    assert!(session.on_published(&early[0], &feed).is_none());
    session.on_text(
        &format!(r#"{{"unsubscribe":{{"topic":"token:{TOKEN_HEX}"}}}}"#),
        &feed,
    );
    let later = feed.apply_price(&price(3, 4.0)).unwrap();
    assert!(session.on_published(&later[0], &feed).is_none());
}

#[test]
fn a_slow_client_is_dropped_back_to_a_fresh_snapshot() {
    let mut feed = Feed::default();
    let mut session = Session::default();
    feed.apply_price(&price(1, 2.0)).unwrap();
    session.on_text(&subscribe(&format!("token:{TOKEN_HEX}")), &feed);
    let tick = feed.apply_price(&price(2, 3.0)).unwrap();
    assert_eq!(
        seq_of(&session.on_published(&tick[0], &feed).unwrap()),
        ("delta", 1)
    );
    feed.apply_price(&price(3, 4.0)).unwrap();
    let out = session.resync(&feed);
    assert_eq!(out.len(), 1);
    assert_eq!(seq_of(&out[0]), ("snapshot", 0));
    assert!(to_json(&out[0]).contains(r#""displayPriceUsd":"4""#));
}

#[test]
fn unknown_topics_and_bad_frames_get_errors() {
    let feed = Feed::default();
    let mut session = Session::default();
    for text in [
        subscribe("account"),
        subscribe("token:0x12"),
        "{not json".into(),
    ] {
        let out = session.on_text(&text, &feed);
        let server_message::Kind::Error(e) = out[0].kind.as_ref().unwrap() else {
            panic!("not an error: {out:?}");
        };
        if text.contains("subscribe") {
            assert_eq!(e.code(), stream_error::Code::UnknownTopic);
        }
    }
    assert!(parse_topic("token:0xabababababababababababababababababababag").is_none());
    assert!(parse_topic(&format!("token:{TOKEN_HEX}")).is_some());
}

#[test]
fn the_snapshot_names_its_quote_token_carries_lineage_and_prices_the_main_pool() {
    let mut feed = Feed::default();
    let mut quote = price(1, 3000.0);
    quote.token = vec![0x42; 20];
    quote.metadata.as_mut().unwrap().symbol = Some("WETH".into());
    feed.apply_price(&quote).unwrap();
    let mut update = price(2, 2.0);
    update.pools.insert(
        0,
        PoolPrice {
            pool: vec![0xee; 20],
            quote_token: vec![0x77; 20],
            price_usd: 1.5,
            ..PoolPrice::default()
        },
    );
    feed.apply_price(&update).unwrap();

    let model = feed.model();
    let addresses: Vec<&String> = model.token_addresses().collect();
    assert_eq!(
        addresses,
        ["0x4242424242424242424242424242424242424242", TOKEN_HEX]
    );
    let token = model.token(TOKEN_HEX).unwrap();
    // The quote token has its own price, so it's named; the other pool's quote isn't.
    assert_eq!(token.quote_token.as_ref().unwrap().symbol, "WETH");
    let unknown = token.pools[0].quote_token.as_ref().unwrap();
    assert_eq!(
        unknown.address,
        "0x7777777777777777777777777777777777777777"
    );
    assert_eq!(unknown.chain_id, 8453);
    // The main pool's price, not the first pool's.
    assert_eq!(token.main_pool_price_usd, "2");
    let lineage = token.lineage.as_ref().unwrap();
    assert_eq!(lineage.id.len(), 16);
    assert_eq!(lineage.caused_by, [vec![2u8; 16]]);
    let heartbeat = api::heartbeat(7, model);
    let Some(server_message::Kind::Heartbeat(beat)) = heartbeat.kind else {
        panic!("not a heartbeat")
    };
    assert_eq!((beat.server_time_ms, beat.head_block_number), (7, 2));
}

#[test]
fn a_token_tick_from_a_block_already_sent_is_skipped() {
    let mut feed = Feed::default();
    let mut session = Session::default();
    feed.apply_price(&price(1, 2.0)).unwrap();
    session.on_text(&subscribe(&format!("token:{TOKEN_HEX}")), &feed);
    let tick = feed.apply_price(&price(2, 3.0)).unwrap();
    assert!(session.on_published(&tick[0], &feed).is_some());
    // The same block again (a tick the throttle released late) is no news.
    assert!(session.on_published(&tick[0], &feed).is_none());
}
