//! Tracing for OmniMarket services (D53, D70): `tracing` spans exported over OTLP to Tempo.
//!
//! Export runs on the batch processor's own thread and reads only the wall clock for span
//! timing, so it never touches a core's inputs: tracing a core can't change its decisions.

use std::fmt;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::Subscriber;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;

/// The local stack's Tempo (`scripts/stack up`).
pub const LOCAL_OTLP_ENDPOINT: &str = "http://localhost:4318/v1/traces";

#[derive(Debug)]
pub enum TelemetryError {
    Exporter(opentelemetry_otlp::ExporterBuildError),
    AlreadyInstalled,
}

impl fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exporter(e) => write!(f, "building the OTLP exporter: {e}"),
            Self::AlreadyInstalled => write!(f, "a global tracing subscriber is already set"),
        }
    }
}

impl std::error::Error for TelemetryError {}

/// Keeps the exporter alive. Dropping it flushes and shuts it down, so hold it until exit.
pub struct Telemetry {
    provider: SdkTracerProvider,
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        // Nothing useful to do with an error while exiting; the spans are lost either way.
        let _ = self.provider.shutdown();
    }
}

/// Exports spans to `otlp_endpoint` (OTLP over HTTP) as `service`, and installs the global
/// subscriber.
pub fn init(service: &str, otlp_endpoint: &str) -> Result<Telemetry, TelemetryError> {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(otlp_endpoint)
        .build()
        .map_err(TelemetryError::Exporter)?;
    let provider = SdkTracerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name(service.to_string())
                .build(),
        )
        .with_batch_exporter(exporter)
        .build();
    // Warnings and errors, including the exporter's own, also go to stderr.
    let stderr = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(LevelFilter::WARN);
    tracing::subscriber::set_global_default(subscriber(&provider).with(stderr))
        .map_err(|_| TelemetryError::AlreadyInstalled)?;
    Ok(Telemetry { provider })
}

/// A subscriber that sends every span to `provider`. `init` installs one globally; tests
/// build one over an in-memory exporter.
pub fn subscriber(
    provider: &SdkTracerProvider,
) -> impl Subscriber + Send + Sync + for<'a> LookupSpan<'a> {
    let tracer = provider.tracer("omnimarket");
    tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer))
}
