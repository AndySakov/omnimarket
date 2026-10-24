//! Execution (#84): a trade's intent (D42, D59), its signature, and the trade in shadow mode
//! (D5), simulated against live state and stopped before broadcast.

pub mod intent;
pub mod safety;
pub mod shadow;

pub use intent::{Intent, LocalSigner};
pub use shadow::{Finished, Request, Signer, Simulation};
