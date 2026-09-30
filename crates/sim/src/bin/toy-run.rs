//! Runs the toy core for one seed with tracing to Tempo, and prints its decision digest.
//!
//!   cargo run -p sim --bin toy-run -- [seed]
//!
//! Traces go to the local stack (`scripts/stack up`), or to OTEL_EXPORTER_OTLP_TRACES_ENDPOINT.

fn main() {
    let seed = match std::env::args().nth(1) {
        Some(arg) => arg.parse().expect("the seed is a u64"),
        None => 42,
    };
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
        .unwrap_or_else(|_| telemetry::LOCAL_OTLP_ENDPOINT.to_string());
    let telemetry = telemetry::init("omnimarket-sim", &endpoint).expect("tracing starts");
    let run = sim::run(seed);
    println!(
        "seed {seed}: {} decisions, digest {}",
        run.decisions.len(),
        run.digest
    );
    drop(telemetry);
}
