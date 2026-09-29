use std::time::{SystemTime, UNIX_EPOCH};

use tokio::time::Instant;
use types::Timestamp;

pub trait Clock {
    fn now(&self) -> Timestamp;
}

/// The wall clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is set before 1970");
        Timestamp::from_unix_nanos(0).after(since_epoch)
    }
}

/// Simulated time, read from tokio's clock. Inside `run_simulated` that clock is paused: it
/// stands still while the core works and jumps to the next timer when the core waits, so
/// clock reads and simulated waits agree (D74). Create it inside `run_simulated`.
#[derive(Clone)]
pub struct SimClock {
    start: Timestamp,
    origin: Instant,
}

impl SimClock {
    pub fn starting_at(start: Timestamp) -> Self {
        Self {
            start,
            origin: Instant::now(),
        }
    }
}

impl Clock for SimClock {
    fn now(&self) -> Timestamp {
        self.start.after(self.origin.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::run_simulated;

    #[test]
    fn sim_clock_stands_still_while_the_core_works() {
        run_simulated(async {
            let clock = SimClock::starting_at(Timestamp::from_unix_nanos(5));
            assert_eq!(clock.now(), Timestamp::from_unix_nanos(5));
            assert_eq!(clock.now(), Timestamp::from_unix_nanos(5));
        });
    }

    #[test]
    fn sim_clock_jumps_to_the_end_of_a_wait() {
        run_simulated(async {
            let clock = SimClock::starting_at(Timestamp::from_unix_nanos(0));
            let reader = clock.clone();
            tokio::time::sleep(Duration::from_millis(200)).await;
            assert_eq!(clock.now(), Timestamp::from_unix_nanos(200_000_000));
            assert_eq!(reader.now(), Timestamp::from_unix_nanos(200_000_000));
        });
    }

    #[test]
    fn system_clock_reads_the_present() {
        let jan_2026 = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);
        assert!(SystemClock.now() > jan_2026);
    }
}
