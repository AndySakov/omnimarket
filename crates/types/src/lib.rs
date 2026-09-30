//! Plain value types shared by every crate.

use std::time::Duration;

/// A point in time, in nanoseconds since the Unix epoch. Every clock read is one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp {
    pub unix_nanos: u64,
}

impl Timestamp {
    pub const fn from_unix_nanos(unix_nanos: u64) -> Self {
        Self { unix_nanos }
    }

    /// Panics past the year 2554, where nanoseconds since 1970 overflow a u64.
    pub fn after(self, elapsed: Duration) -> Self {
        let elapsed =
            u64::try_from(elapsed.as_nanos()).expect("duration overflows u64 nanoseconds");
        let unix_nanos = self
            .unix_nanos
            .checked_add(elapsed)
            .expect("timestamp overflows u64 nanoseconds");
        Self { unix_nanos }
    }
}

/// Identifies a record in the lineage graph (D53, D71): 16 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineageId([u8; 16]);

impl LineageId {
    /// For a record with a natural key: BLAKE3 over the record kind and the key, cut to 128
    /// bits. The kind is length-prefixed so no two (kind, key) pairs encode the same bytes.
    /// Callers encode key fields fixed-width, little-endian, in the order `data.md` lists them.
    pub fn from_natural_key(kind: &str, key: &[u8]) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&(kind.len() as u64).to_le_bytes());
        hasher.update(kind.as_bytes());
        hasher.update(key);
        let mut id = [0; 16];
        id.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        Self(id)
    }

    /// For a record with no natural key: two draws from the core's seeded Rng.
    pub fn from_draws(high: u64, low: u64) -> Self {
        let mut id = [0; 16];
        id[..8].copy_from_slice(&high.to_be_bytes());
        id[8..].copy_from_slice(&low.to_be_bytes());
        Self(id)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn after_adds_the_duration() {
        let t = Timestamp::from_unix_nanos(1_000);
        assert_eq!(
            t.after(Duration::from_millis(200)),
            Timestamp::from_unix_nanos(200_001_000)
        );
    }

    // Pinned: a change here would give every stored record a new ID.
    #[test]
    fn natural_key_ids_are_pinned() {
        let id = LineageId::from_natural_key("sim.toy_event", &7u64.to_le_bytes());
        assert_eq!(
            id.as_bytes(),
            &[
                238, 51, 62, 132, 230, 172, 94, 85, 137, 47, 164, 19, 160, 204, 62, 57
            ]
        );
    }

    #[test]
    fn kind_and_key_do_not_run_together() {
        assert_ne!(
            LineageId::from_natural_key("ab", b"c"),
            LineageId::from_natural_key("a", b"bc")
        );
    }

    #[test]
    fn draws_fill_the_id_in_order() {
        let id = LineageId::from_draws(1, 2);
        assert_eq!(
            id.as_bytes(),
            &[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2]
        );
    }
}
