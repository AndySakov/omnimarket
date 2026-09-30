//! The M0 demo: a simulated run is reproducible from its seed, and replays exactly from its
//! recording.

use det::{Recordable, Source};
use sim::Quote;

#[test]
fn same_seed_gives_the_same_digest() {
    for seed in 0..100 {
        assert_eq!(sim::run(seed).digest, sim::run(seed).digest, "seed {seed}");
    }
}

#[test]
fn a_recorded_run_replays_to_the_same_digest() {
    for seed in 0..100 {
        let run = sim::run(seed);
        let replayed = sim::replay(run.recording);
        assert_eq!(replayed, run.decisions, "seed {seed}");
        assert_eq!(sim::digest(&replayed), run.digest, "seed {seed}");
    }
}

#[test]
fn lineage_ids_survive_replay() {
    let run = sim::run(5);
    let ids: Vec<_> = run.decisions.iter().map(|d| (d.id, d.caused_by)).collect();
    let replayed: Vec<_> = sim::replay(run.recording)
        .iter()
        .map(|d| (d.id, d.caused_by))
        .collect();
    assert_eq!(replayed, ids);
    assert_eq!(sim::run(5).decisions, run.decisions);
}

#[test]
fn different_seeds_give_different_digests() {
    assert_ne!(sim::run(1).digest, sim::run(2).digest);
}

#[test]
fn a_changed_input_changes_the_digest() {
    let run = sim::run(1);
    let mut recording = run.recording;
    let quote = recording
        .iter_mut()
        .find(|r| r.source == Source::Rpc)
        .unwrap();
    // An RPC payload is the call number (8 bytes), then the response.
    let changed = Quote {
        price: Quote::decode(&quote.payload[8..]).unwrap().price + 1,
    };
    quote.payload.truncate(8);
    quote.payload.extend(changed.encode());
    assert_ne!(sim::digest(&sim::replay(recording)), run.digest);
}

#[test]
#[should_panic(expected = "replay diverged")]
fn reordered_inputs_are_caught() {
    let mut recording = sim::run(1).recording;
    let i = recording
        .iter()
        .position(|r| r.source == Source::Rpc)
        .unwrap();
    recording.swap(i, i + 1);
    sim::replay(recording);
}

// Pinned so any change to the core, the world, the det sources or the byte layout shows up
// here as a replay break, on purpose or not.
#[test]
fn seed_42_digest_is_pinned() {
    assert_eq!(
        sim::run(42).digest.to_hex().as_str(),
        "3550395bd07fcd9e979262298831ee5e7cf4bb9095a4a1013a772dad9361f6e2"
    );
}
