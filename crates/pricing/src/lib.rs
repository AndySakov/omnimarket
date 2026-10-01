//! Pricing (M2): a pool's mid and ±2% depth, a token's display price across its pools, and
//! token metadata (D18, D19, D24). Pure functions of state the Chain Engine holds; the engine
//! reads metadata through its recorded calls and publishes price updates (D6, D77).

pub mod config;
pub mod display;
pub mod metadata;
pub mod pool;
pub mod tick_math;

pub use config::{PricingConfig, QuoteAsset, QuoteKind};
pub use display::{DisplayPrice, PoolQuote, display_price};
pub use metadata::TokenMetadata;
pub use pool::PoolPrice;
