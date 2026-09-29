//! Deterministic simulation (D49). For now it holds a toy core and a runner that drives it
//! from a seed: the M0 replay demo (D72).

use std::time::Duration;

use det::{Clock, Rng, SeededRng, SimClock};
use types::Timestamp;

/// Where every simulated run starts: 2026-01-01T00:00:00Z.
pub const SIM_START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);

/// How far simulated time moves between steps: one Base flashblock.
pub const STEP: Duration = Duration::from_millis(200);

/// What the toy core decided in one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub at: Timestamp,
    pub roll: u8,
    pub fire: bool,
}

impl Decision {
    /// A fixed byte layout, so digests don't depend on how Rust lays out the struct.
    fn to_bytes(self) -> [u8; 10] {
        let mut bytes = [0; 10];
        bytes[..8].copy_from_slice(&self.at.unix_nanos.to_le_bytes());
        bytes[8] = self.roll;
        bytes[9] = u8::from(self.fire);
        bytes
    }
}

/// A stand-in for a real core: each step reads the clock, rolls a number and fires when the
/// roll is under the threshold. It touches time and randomness only through `det`.
pub struct ToyCore {
    clock: Box<dyn Clock>,
    rng: Box<dyn Rng>,
    fire_below: u8,
}

impl ToyCore {
    pub fn new(clock: Box<dyn Clock>, rng: Box<dyn Rng>, fire_below: u8) -> Self {
        Self {
            clock,
            rng,
            fire_below,
        }
    }

    pub fn step(&mut self) -> Decision {
        let at = self.clock.now();
        let roll = (self.rng.next_u64() % 100) as u8;
        Decision {
            at,
            roll,
            fire: roll < self.fire_below,
        }
    }
}

/// Runs the toy core for `steps` steps from `seed` on simulated time and returns a digest of
/// every decision it made. Same seed, same digest.
pub fn run(seed: u64, steps: u32) -> blake3::Hash {
    let clock = SimClock::starting_at(SIM_START);
    let mut core = ToyCore::new(
        Box::new(clock.clone()),
        Box::new(SeededRng::from_seed(seed)),
        30,
    );
    let mut digest = blake3::Hasher::new();
    for _ in 0..steps {
        digest.update(&core.step().to_bytes());
        clock.advance(STEP);
    }
    digest.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toy_core_reads_simulated_time() {
        let clock = SimClock::starting_at(SIM_START);
        let mut core = ToyCore::new(
            Box::new(clock.clone()),
            Box::new(SeededRng::from_seed(1)),
            30,
        );
        assert_eq!(core.step().at, SIM_START);
        clock.advance(STEP);
        assert_eq!(core.step().at, SIM_START.after(STEP));
    }

    #[test]
    fn toy_core_fires_below_the_threshold() {
        let clock = SimClock::starting_at(SIM_START);
        let mut always = ToyCore::new(
            Box::new(clock.clone()),
            Box::new(SeededRng::from_seed(1)),
            100,
        );
        let mut never = ToyCore::new(Box::new(clock), Box::new(SeededRng::from_seed(1)), 0);
        for _ in 0..50 {
            assert!(always.step().fire);
            assert!(!never.step().fire);
        }
    }
}
