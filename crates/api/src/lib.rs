//! The API gateway both UIs talk to (D62): it consumes the Base engine's output into in-memory
//! read models and serves contract v0 (D91) over REST and WebSocket. The read models, the
//! throttle and the stream protocol are pure; `server` is the I/O around them.

mod discovery;
mod feed;
mod model;
pub mod server;
mod status;
mod stream;
mod throttle;

pub use crate::discovery::{BASE_BLOCK_MS, Discovery, DiscoveryConfig, Filters};
pub use crate::feed::{DISCOVERY_TOPIC, Feed, Published, STATUS_TOPIC, token_topic};
pub use crate::model::{ModelError, ReadModel, decimal, hex, scaled_decimal};
pub use crate::status::engine_status;
pub use crate::stream::{Session, Topic, heartbeat, parse_topic, to_json};
pub use crate::throttle::{MIN_GAP_MS, Throttle};
