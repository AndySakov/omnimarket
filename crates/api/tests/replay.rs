//! A recorded input log through the engine and the API gives byte-identical API output every
//! time (build rule 1): the read models are pure functions of the records, timed by block time.

use api::{Feed, Session, to_json};
use det::Replay;
use engine::{Engine, EngineConfig, InMemoryOutbox};

const INPUTS: &[u8] = include_bytes!("fixtures/priced-replay/inputs.pb.zst");

/// Replays a live Base recording with pricing on (`engine follow --record-to`, D99) through the engine, feeds its price records to the
/// API, and returns everything a client subscribed to every token would have received, plus
/// each token's REST snapshot at the end.
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
    let mut feed = Feed::default();
    let mut session = Session::default();
    let mut out = String::new();
    for update in &prices {
        let token = api::hex(update.token.as_slice());
        // Subscribe to each token as it first appears, before its first record.
        if feed.model().token(&token).is_none() {
            let frame = format!(r#"{{"subscribe":{{"topic":"token:{token}"}}}}"#);
            for message in session.on_text(&frame, feed.model()) {
                out.push_str(&to_json(&message));
                out.push('\n');
            }
        }
        for published in feed
            .apply_price(&update.to_proto())
            .expect("a valid record")
        {
            if let Some(message) = session.on_published(&published, feed.model()) {
                out.push_str(&to_json(&message));
                out.push('\n');
            }
        }
    }
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
