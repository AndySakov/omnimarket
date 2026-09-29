//! The M0 demo: a simulated run is reproducible from its seed, and replays exactly from its
//! recording.

use det::Source;

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
        assert_eq!(sim::replay(run.recording), run.digest, "seed {seed}");
    }
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
    // An RPC payload is the call number (8 bytes), then the quote.
    quote.payload[8] ^= 1;
    assert_ne!(sim::replay(recording), run.digest);
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
        "8a85778f3f7b7a89811c296bec38722f0dead54c63978d1f4b68fc558f85d5d4"
    );
}
