//! The API server binary.
//!
//!   api --kafka BROKERS [--listen ADDR] [--cors-origin URL] [--group-id ID]
//!
//! Consumes `prices.base` from the start and serves contract v0 (D91): `/health`,
//! `GET /v1/tokens/{chain_id}/{address}` and the WebSocket stream at `/v1/stream`.

use std::net::SocketAddr;
use std::process::ExitCode;

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
    };
    match serve(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("api: {e:?}");
            ExitCode::FAILURE
        }
    }
}
