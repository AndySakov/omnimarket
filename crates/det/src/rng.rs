use rand_chacha::ChaCha8Rng;
use rand_core::{Rng as _, SeedableRng};
use types::LineageId;

pub trait Rng {
    fn next_u64(&mut self) -> u64;
}

/// ChaCha8 from a 64-bit seed. The same type serves production and simulation; only the
/// seed's source differs. Recording the seed is enough to replay every draw, because
/// ChaCha8's output for a given seed is fixed across versions (unlike `rand::StdRng`).
pub struct SeededRng {
    seed: u64,
    inner: ChaCha8Rng,
}

impl SeededRng {
    /// For simulation and replay.
    pub fn from_seed(seed: u64) -> Self {
        Self {
            seed,
            inner: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    /// For production: one seed drawn from the OS at startup.
    pub fn from_os() -> Self {
        let seed = getrandom::u64().expect("OS random source is unavailable");
        Self::from_seed(seed)
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }
}

impl Rng for SeededRng {
    fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }
}

/// A lineage ID for a record with no natural key (D71), from two draws.
pub fn random_lineage_id(rng: &mut dyn Rng) -> LineageId {
    LineageId::from_draws(rng.next_u64(), rng.next_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_draws_the_same_numbers() {
        let mut a = SeededRng::from_seed(7);
        let mut b = SeededRng::from_seed(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    // Pinned so a dependency upgrade that changes the stream fails here, not in a replay.
    // Cross-checked against rand_chacha 0.3.1, which gives the same three numbers.
    #[test]
    fn seed_zero_stream_is_pinned() {
        let mut rng = SeededRng::from_seed(0);
        let draws = [rng.next_u64(), rng.next_u64(), rng.next_u64()];
        assert_eq!(
            draws,
            [
                13_080_132_717_333_068_652,
                8_594_738_769_458_413_623,
                12_896_916_468_484_187_878
            ]
        );
    }

    #[test]
    fn from_os_keeps_its_seed() {
        let rng = SeededRng::from_os();
        let mut replay = SeededRng::from_seed(rng.seed());
        let mut rng = rng;
        assert_eq!(rng.next_u64(), replay.next_u64());
    }
}
