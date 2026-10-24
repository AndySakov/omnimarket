//! The API gateway both UIs talk to (D62): it consumes the Base engine's output into in-memory
//! read models and serves contract v0 (D91) over REST and WebSocket. The read models, the
//! throttle and the stream protocol are pure; `server` is the I/O around them.

mod accounts;
mod candles;
mod demo;
mod discovery;
mod feed;
mod model;
mod quotes;
mod safety;
pub mod server;
mod session;
mod status;
mod store;
mod stream;
mod throttle;
mod trades;

pub use crate::accounts::{AccountState, Fill, FillError, starting_balances};
pub use crate::candles::{Candles, INTERVALS, MAX_SERIES, candles_topic, interval_named};
pub use crate::demo::{Demo, DemoError, Opened, SESSIONS_PER_IP_HOUR, TRADES_PER_ACCOUNT_HOUR};
pub use crate::discovery::{BASE_BLOCK_MS, Discovery, DiscoveryConfig, Filters};
pub use crate::feed::{DISCOVERY_TOPIC, Feed, Published, STATUS_TOPIC, token_topic};
pub use crate::model::{ModelError, ReadModel, decimal, hex, scaled_decimal};
pub use crate::quotes::{QuoteError, Quotes};
pub use crate::safety::{
    RECHECK_MS, Safety, WATCH_MS, failed as safety_check_failed, report as safety_report,
};
pub use crate::session::{Privy, PrivyError, PrivyUser, SESSION_MS, Sessions};
pub use crate::status::engine_status;
pub use crate::store::{Store, StoreError, Stored};
pub use crate::stream::{Session, Topic, heartbeat, parse_topic, to_json};
pub use crate::throttle::{MIN_GAP_MS, Throttle};
pub use crate::trades::{Prepared, Run, TradeError, Trades};
