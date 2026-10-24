//! A recorded input log through the engine and the API gives byte-identical API output every
//! time (build rule 1): the read models are pure functions of the records, timed by block time,
//! not of how the consumer interleaves the topics (build rule 8).

mod interleave;

use api::{Feed, Filters, Session, to_json};
use det::Replay;
use engine::{Engine, EngineConfig, InMemoryOutbox, Mode, Run};
use proto::pool::v1::PoolUpdate;
use proto::price::v1::PriceUpdate;
use proto::status::v1::EngineStatus;
use proto::trade::v1::Trade;

enum Record {
    Pool(PoolUpdate),
    Trade(Trade),
    Price(PriceUpdate),
    Status(EngineStatus),
}

impl Record {
    fn block_number(&self) -> u64 {
        match self {
            Record::Pool(u) => u.block_number,
            Record::Trade(t) => t.block_number,
            Record::Price(p) => p.block_number,
            Record::Status(s) => s.block_number,
        }
    }
}

const INPUTS: &[u8] = include_bytes!("fixtures/priced-replay/inputs.pb.zst");

/// Seeded random interleavings an interleaving test checks, besides the extremes.
const RANDOM_INTERLEAVINGS: u64 = 8;

/// Replays a live Base recording with pricing on (`engine follow --record-to`, D99) through the
/// engine, and returns what it published on each topic the API reads, each in the engine's own
/// order: pool updates, trades, prices and statuses.
fn recorded_topics() -> Vec<Vec<Record>> {
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
    assert!(
        prices.len() > 10,
        "the recording prices tokens: {}",
        prices.len()
    );
    let run = Run {
        mode: Mode::Replay,
        core_instance: "base-00000000000000ff".into(),
        recording: true,
    };
    vec![
        outbox
            .updates()
            .iter()
            .map(|u| Record::Pool(u.to_proto()))
            .collect(),
        outbox
            .trades()
            .iter()
            .map(|t| Record::Trade(t.to_proto()))
            .collect(),
        prices.iter().map(|p| Record::Price(p.to_proto())).collect(),
        outbox
            .statuses()
            .iter()
            .map(|s| Record::Status(s.to_proto(&run)))
            .collect(),
    ]
}

/// The records as the consumer reads them live: block by block, by each topic's position, then
/// topic by topic. A topic's records can be up to tens of blocks behind its latest (a pool is
/// published once proven or read), so its position is the highest block it has reached.
fn by_block(topics: &[Vec<Record>]) -> Vec<&Record> {
    let mut records: Vec<(u64, usize, usize, &Record)> = Vec::new();
    for (topic, list) in topics.iter().enumerate() {
        let mut reached = 0;
        for (i, record) in list.iter().enumerate() {
            reached = reached.max(record.block_number());
            records.push((reached, topic, i, record));
        }
    }
    records.sort_by_key(|r| (r.0, r.1, r.2));
    records.into_iter().map(|r| r.3).collect()
}

/// Every interleaving a test checks: the live order first, then the extremes and the random ones.
fn every_order(topics: &[Vec<Record>]) -> Vec<(String, Vec<&Record>)> {
    let mut orders = vec![("block by block".to_string(), by_block(topics))];
    orders.extend(interleave::interleavings(topics, RANDOM_INTERLEAVINGS));
    orders
}

/// What the API makes of `records`.
struct Output {
    /// Everything a client subscribed to `discovery`, `status` and every token would have
    /// received, then the discovery feed and each token's REST snapshot at the end.
    all: String,
    /// Just the discovery messages, then the final feed.
    discovery: String,
    /// Just the status messages, then the final status.
    status: String,
    /// Every token's candles at the end, at every interval.
    candles: String,
}

fn api_output(records: &[&Record]) -> Output {
    let mut feed = Feed::default();
    let mut session = Session::default();
    let mut out = String::new();
    for topic in ["discovery", "status"] {
        let frame = format!(r#"{{"subscribe":{{"topic":"{topic}"}}}}"#);
        for message in session.on_text(&frame, &feed) {
            out.push_str(&to_json(&message));
            out.push('\n');
        }
    }
    let mut discovery = String::new();
    let mut status = String::new();
    for record in records {
        let published = match record {
            Record::Pool(u) => feed.apply_pool_update(u),
            Record::Trade(t) => feed.apply_trade(t),
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
                feed.apply_price(update).expect("a valid record")
            }
            Record::Status(s) => feed.apply_status(s).expect("a valid record"),
        };
        for published in published {
            if let Some(message) = session.on_published(&published, &feed) {
                let json = to_json(&message);
                if published.topic == api::DISCOVERY_TOPIC {
                    discovery.push_str(&json);
                    discovery.push('\n');
                }
                if published.topic == api::STATUS_TOPIC {
                    status.push_str(&json);
                    status.push('\n');
                }
                out.push_str(&json);
                out.push('\n');
            }
        }
    }
    status.push_str(&serde_json::to_string(&feed.status()).unwrap());
    let final_feed = serde_json::to_string(&feed.discovery().feed(&Filters::default())).unwrap();
    discovery.push_str(&final_feed);
    out.push_str(&final_feed);
    out.push('\n');
    let tokens: Vec<String> = feed.model().token_addresses().cloned().collect();
    let mut candles = String::new();
    for token in tokens {
        let snapshot = feed.model().token(&token).unwrap();
        out.push_str(&serde_json::to_string(snapshot).unwrap());
        out.push('\n');
        for (interval, _, _) in api::INTERVALS {
            let series =
                feed.candles()
                    .series(Some(8453), token.parse().unwrap(), interval, 0, u64::MAX);
            candles.push_str(&serde_json::to_string(&series).unwrap());
            candles.push('\n');
        }
    }
    Output {
        all: out,
        discovery,
        status,
        candles,
    }
}

#[test]
fn the_discovery_feed_is_the_same_however_the_topics_interleave() {
    let topics = recorded_topics();
    let discovery = interleave::assert_same_output(&every_order(&topics), |records| {
        api_output(records).discovery
    });
    let rows = discovery.matches(r#""discoveryRow""#).count();
    assert!(
        rows > 10,
        "the recording moves the discovery feed: {rows} rows"
    );
}

#[test]
fn the_status_feed_is_the_same_however_the_topics_interleave() {
    let topics = recorded_topics();
    let status =
        interleave::assert_same_output(&every_order(&topics), |records| api_output(records).status);
    let deltas = status.matches(r#""delta""#).count();
    assert!(deltas > 10, "the recording has statuses: {deltas}");
}

#[test]
fn a_recorded_session_gives_byte_identical_api_output() {
    let first = api_output(&by_block(&recorded_topics())).all;
    let second = api_output(&by_block(&recorded_topics())).all;
    assert!(first.contains(r#""delta""#), "the output has deltas");
    assert!(
        first == second,
        "the API output differs between two replays"
    );
}

// Live, the API reads the topics block by block; rebuilt after a restart, it may read one topic
// far ahead of the others.
#[test]
fn candles_rebuilt_from_a_recorded_session_match_the_live_ones() {
    let topics = recorded_topics();
    let live = interleave::assert_same_output(&every_order(&topics), |records| {
        api_output(records).candles
    });
    let candles = live.matches("openTimeMs").count();
    assert!(candles > 100, "the recording makes candles: {candles}");
}
