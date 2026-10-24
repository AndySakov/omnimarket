//! The API server binary.
//!
//!   api --kafka BROKERS [--listen ADDR] [--cors-origin URL] [--group-id ID]
//!       [--new-pool-window-ms MS] [--trending-min-depth-usd USD] [--call-rpc URL]
//!       [--session-secret S] [--privy-app-id ID --privy-verification-key PEM] [--postgres URL]
//!
//! Consumes `pool-updates.base`, `trades.base`, `trade-history.base`, `prices.base` and
//! `status.base` from the start and serves contract v0 (D91): `/health`, `GET /v1/status`,
//! `GET /v1/tokens/{chain_id}/{address}`, `GET /v1/discovery`, `GET /v1/candles`, `POST /v1/quotes`,
//! `POST /v1/trades`, `GET /v1/trades/{trade_id}/receipt` and the
//! WebSocket stream at `/v1/stream`, and Prometheus metrics at `/metrics`.

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
    /// Where shadow trades' simulations go (Uniswap's quoter at the quote's block). Unset, or
    /// unusable at start: they simulate on the in-memory pool state.
    #[arg(long)]
    call_rpc: Option<String>,
    /// Signs session tokens (#85). Unset: a development secret, not for a public server.
    #[arg(long, env = "OMNIMARKET_SESSION_SECRET", hide_env_values = true)]
    session_secret: Option<String>,
    /// The Privy app whose logins link demo accounts, and its PEM verification key.
    #[arg(long, requires = "privy_verification_key")]
    privy_app_id: Option<String>,
    #[arg(
        long,
        env = "OMNIMARKET_PRIVY_VERIFICATION_KEY",
        requires = "privy_app_id"
    )]
    privy_verification_key: Option<String>,
    /// Where demo accounts are kept between restarts, e.g. the local stack's
    /// postgres://omnimarket:omnimarket@localhost:5432/omnimarket. Unset: in memory only.
    #[arg(long, env = "OMNIMARKET_POSTGRES", hide_env_values = true)]
    postgres: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().init();
    let cli = Cli::parse();
    let config = Config {
        listen: cli.listen,
        call_rpc: cli.call_rpc,
        session_secret: cli.session_secret,
        privy: cli.privy_app_id.zip(cli.privy_verification_key),
        postgres: cli.postgres,
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
