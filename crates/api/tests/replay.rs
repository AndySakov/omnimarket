//! A recorded input log through the engine and the API gives byte-identical API output every
//! time (build rule 1): the read models are pure functions of the records, timed by block time.

use api::{Feed, Filters, Session, to_json};
use det::Replay;
use engine::{Engine, EngineConfig, InMemoryOutbox};
use proto::pool::v1::PoolUpdate;
use proto::price::v1::PriceUpdate;
use proto::trade::v1::Trade;

enum Record {
    Pool(PoolUpdate),
    Trade(Trade),
    Price(PriceUpdate),
}

const INPUTS: &[u8] = include_bytes!("fixtures/priced-replay/inputs.pb.zst");

/// Replays a live Base recording with pricing on (`engine follow --record-to`, D99) through the engine, feeds its records to the
/// API with its pool updates and trades, and returns everything a client subscribed to
/// `discovery` and every token would have received, plus the discovery feed and each token's
/// REST snapshot at the end.
fn api_output() -> String {
    let recording = det::file::decode_log_file(INPUTS).expect("the fixture decodes");
    let replay = Replay::new(recording);
    let config = replay
        .config()
        .and_then(|bytes| EngineConfig::decode(&bytes))
        .expect("the fixture starts with an engine config");
    let outbox = InMemoryOutbox::default();
    let engine = Engine::new(
        config,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(outbox.clone()),
    );
    replay.run(engine.run()).expect("the replay finishes");

    let prices = outbox.prices();
    // The three topics, merged in block order as the API's consumer would read them: within a
    // block, pool updates, then trades, then prices, each in the engine's own order.
    let mut records: Vec<(u64, u8, usize, Record)> = Vec::new();
    for (i, u) in outbox.updates().iter().enumerate() {
        let u = u.to_proto();
        records.push((u.block_number, 0, i, Record::Pool(u)));
    }
    for (i, t) in outbox.trades().iter().enumerate() {
        let t = t.to_proto();
        records.push((t.block_number, 1, i, Record::Trade(t)));
    }
    for (i, p) in prices.iter().enumerate() {
        let p = p.to_proto();
        records.push((p.block_number, 2, i, Record::Price(p)));
    }
    records.sort_by_key(|r| (r.0, r.1, r.2));

    let mut feed = Feed::default();
    let mut session = Session::default();
    let mut out = String::new();
    for message in session.on_text(r#"{"subscribe":{"topic":"discovery"}}"#, &feed) {
        out.push_str(&to_json(&message));
        out.push('\n');
    }
    let mut discovery_deltas = 0;
    for (_, _, _, record) in records {
        let published = match record {
            Record::Pool(u) => feed.apply_pool_update(&u),
            Record::Trade(t) => feed.apply_trade(&t),
            Record::Price(update) => {
                let token = api::hex(update.token.as_slice());
                // Subscribe to each token as it first appears, before its first record.
                if feed.model().token(&token).is_none() {
                    let frame = format!(r#"{{"subscribe":{{"topic":"token:{token}"}}}}"#);
                    for message in session.on_text(&frame, &feed) {
                        out.push_str(&to_json(&message));
                        out.push('\n');
                    }
                }
                feed.apply_price(&update).expect("a valid record")
            }
        };
        for published in published {
            if let Some(message) = session.on_published(&published, &feed) {
                discovery_deltas += usize::from(published.topic == api::DISCOVERY_TOPIC);
                out.push_str(&to_json(&message));
                out.push('\n');
            }
        }
    }
    assert!(
        discovery_deltas > 0,
        "the recording moves the discovery feed"
    );
    out.push_str(&serde_json::to_string(&feed.discovery().feed(&Filters::default())).unwrap());
    out.push('\n');
    let tokens: Vec<String> = feed.model().token_addresses().cloned().collect();
    for token in tokens {
        let snapshot = feed.model().token(&token).unwrap();
        out.push_str(&serde_json::to_string(snapshot).unwrap());
        out.push('\n');
    }
    assert!(
        prices.len() > 10,
        "the recording prices tokens: {}",
        prices.len()
    );
    out
}

#[test]
fn a_recorded_session_gives_byte_identical_api_output() {
    let first = api_output();
    let second = api_output();
    assert!(first.contains(r#""delta""#), "the output has deltas");
    assert!(
        first == second,
        "the API output differs between two replays"
    );
}
