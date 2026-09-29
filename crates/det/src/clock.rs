use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Simulated time. It stands still until the simulator calls `advance`.
///
/// Clones share one clock: the simulator keeps a handle to move time while the core holds
/// another to read it. `Rc<Cell<_>>` rather than `Arc<Mutex<_>>` because cores are
/// single-threaded (rule 6), so the compiler stops a `SimClock` from crossing threads.
#[derive(Clone)]
pub struct SimClock {
    now: Rc<Cell<Timestamp>>,
}

impl SimClock {
    pub fn starting_at(start: Timestamp) -> Self {
        Self {
            now: Rc::new(Cell::new(start)),
        }
    }

    pub fn advance(&self, by: Duration) {
        self.now.set(self.now.get().after(by));
    }
}

impl Clock for SimClock {
    fn now(&self) -> Timestamp {
        self.now.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_clock_stands_still_until_advanced() {
        let clock = SimClock::starting_at(Timestamp::from_unix_nanos(5));
        assert_eq!(clock.now(), Timestamp::from_unix_nanos(5));
        assert_eq!(clock.now(), Timestamp::from_unix_nanos(5));
        clock.advance(Duration::from_nanos(10));
        assert_eq!(clock.now(), Timestamp::from_unix_nanos(15));
    }

    #[test]
    fn sim_clock_clones_share_time() {
        let driver = SimClock::starting_at(Timestamp::from_unix_nanos(0));
        let reader = driver.clone();
        driver.advance(Duration::from_secs(1));
        assert_eq!(reader.now(), Timestamp::from_unix_nanos(1_000_000_000));
    }

    #[test]
    fn system_clock_reads_the_present() {
        let jan_2026 = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);
        assert!(SystemClock.now() > jan_2026);
    }
}
