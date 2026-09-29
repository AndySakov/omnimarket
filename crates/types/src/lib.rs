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
}
