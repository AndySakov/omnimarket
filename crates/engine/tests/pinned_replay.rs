//! A real Base recording, pinned in the repository, replays through today's engine to the
//! summary it was recorded with (D96). A change to the engine's decisions moves its digests:
//! re-record the fixture (docs/spec/observability.md) and say in the PR why they moved.

use det::Replay;
use engine::{Engine, EngineConfig, InMemoryOutbox};

const INPUTS: &[u8] = include_bytes!("fixtures/base-replay/inputs.pb.zst");
const SUMMARY: &str = include_str!("fixtures/base-replay/summary.txt");

#[test]
fn the_pinned_base_recording_replays_to_its_summary() {
    let recording = det::file::decode_log_file(INPUTS).expect("the fixture decodes");
    let replay = Replay::new(recording);
    let config = replay
        .config()
        .and_then(|bytes| EngineConfig::decode(&bytes))
        .expect("the fixture starts with an engine config");
    let engine = Engine::new(
        config,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(InMemoryOutbox::default()),
    );
    let summary = replay.run(engine.run()).expect("the replay finishes");
    assert_eq!(
        summary.to_string(),
        SUMMARY,
        "the engine's decisions on the pinned Base recording changed: if that's intended, \
         re-record the fixture (docs/spec/observability.md) and explain in the PR why"
    );
}
