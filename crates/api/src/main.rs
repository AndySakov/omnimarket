//! The API server binary.
//!
//!   api --kafka BROKERS [--listen ADDR] [--cors-origin URL] [--group-id ID]
//!       [--new-pool-window-ms MS] [--trending-min-depth-usd USD]
//!
//! Consumes `pool-updates.base`, `trades.base`, `prices.base` and `status.base` from the start
//! and serves contract v0 (D91): `/health`, `GET /v1/status`,
//! `GET /v1/tokens/{chain_id}/{address}`, `GET /v1/discovery` and the WebSocket stream at
//! `/v1/stream`, and Prometheus metrics at `/metrics`.

use std::net::SocketAddr;
use std::process::ExitCode;

use api::DiscoveryConfig;
use api::server::{Config, serve};
use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// Kafka brokers carrying the engine's output.
    #[arg(long)]
    kafka: String,
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    /// The terminal's origin, allowed by CORS.
    #[arg(long, default_value = "http://localhost:5173")]
    cors_origin: String,
    #[arg(long, default_value = "omnimarket-api")]
    group_id: String,
    /// How long a pool created during the session stays in the discovery feed's New list.
    #[arg(long, default_value_t = 3_600_000)]
    new_pool_window_ms: u64,
    /// The least ±2% depth, in USD, a token needs to trend (D24).
    #[arg(long, default_value_t = 10_000.0)]
    trending_min_depth_usd: f64,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().init();
    let cli = Cli::parse();
    let config = Config {
        listen: cli.listen,
        kafka: cli.kafka,
        group_id: cli.group_id,
        cors_origin: cli.cors_origin,
        discovery: DiscoveryConfig {
            new_pool_window_ms: cli.new_pool_window_ms,
            trending_min_depth_usd: cli.trending_min_depth_usd,
            ..DiscoveryConfig::default()
        },
    };
    match serve(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("api: {e:?}");
            ExitCode::FAILURE
        }
    }
}
