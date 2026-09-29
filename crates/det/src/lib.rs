//! The det runtime (D49, D54, D72): the only way core code reads time or draws randomness.
//!
//! Each trait has a real implementation for production and a simulated one for tests and
//! the simulator. Cores hold a `Box<dyn Clock>` and a `Box<dyn Rng>` and never know which.

mod clock;
mod rng;

pub use clock::{Clock, SimClock, SystemClock};
pub use rng::{Rng, SeededRng};
