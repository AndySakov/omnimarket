//! The det runtime (D49, D54, D72, D74): the only way core code reads time, draws randomness,
//! receives events or calls out.
//!
//! Each trait has a real implementation for production, a simulated one for tests and the
//! simulator, a recording wrapper and a replay source. Cores hold `Box<dyn …>` of each and
//! never know which. `Clock` and `Rng` never wait, so they are plain methods; `EventSource`
//! and `Rpc` wait, so they return futures and the core runs as one task (D74).

mod clock;
mod events;
pub mod kafka;
mod record;
mod replay;
mod rng;
mod rpc;

use std::future::Future;

pub use clock::{Clock, SimClock, SystemClock};
pub use events::{EventSource, SimEventSource};
pub use record::{
    InMemorySink, InputRecord, InvalidInputRecord, Recordable, Recorder, RecordingClock,
    RecordingEventSource, RecordingRng, RecordingRpc, RecordingSink, Source,
};
pub use replay::{Replay, ReplayClock, ReplayEvents, ReplayRng, ReplayRpc};
pub use rng::{Rng, SeededRng, random_lineage_id};
pub use rpc::{Rpc, SimRpc};

/// Runs `future` to completion as one task on a current-thread runtime with tokio's clock
/// paused: time moves only when every task is waiting, straight to the next timer (D74).
pub fn run_simulated<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("a current-thread runtime builds")
        .block_on(future)
}
