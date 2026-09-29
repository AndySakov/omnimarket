//! The M0 demo: a simulated run replays identically from its seed.

#[test]
fn same_seed_gives_the_same_digest() {
    for seed in 0..100 {
        assert_eq!(sim::run(seed, 1_000), sim::run(seed, 1_000), "seed {seed}");
    }
}

#[test]
fn different_seeds_give_different_digests() {
    assert_ne!(sim::run(1, 1_000), sim::run(2, 1_000));
}

// Pinned so any change to the core, the clock, the Rng stream or the byte layout shows up
// here as a replay break, on purpose or not.
#[test]
fn seed_42_digest_is_pinned() {
    assert_eq!(
        sim::run(42, 1_000).to_hex().as_str(),
        "da9e44e6b67534707586075fbc529f14907a30ea021069c94a0287acb5df3f34"
    );
}
