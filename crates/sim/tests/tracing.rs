//! The observability skeleton (D70): decisions are traced with their lineage, and tracing
//! can't change a run.

use opentelemetry::Value;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

fn traced_run(seed: u64) -> (sim::Run, Vec<opentelemetry_sdk::trace::SpanData>) {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let run =
        tracing::subscriber::with_default(telemetry::subscriber(&provider), || sim::run(seed));
    provider.force_flush().expect("spans flush");
    (run, exporter.get_finished_spans().expect("spans read"))
}

fn attribute(span: &opentelemetry_sdk::trace::SpanData, key: &str) -> Option<Value> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.clone())
}

#[test]
fn tracing_does_not_change_the_digest() {
    let (traced, spans) = traced_run(42);
    assert!(!spans.is_empty());
    assert_eq!(traced.digest, sim::run(42).digest);
    assert_eq!(traced.recording, sim::run(42).recording);
}

#[test]
fn every_decision_span_carries_its_lineage() {
    let (run, spans) = traced_run(3);
    let decisions: Vec<_> = spans
        .iter()
        .filter(|s| s.name == "toy_core.decision")
        .collect();
    assert_eq!(decisions.len(), run.decisions.len());
    let parent = spans.iter().find(|s| s.name == "toy_core.run").unwrap();
    for (span, decision) in decisions.iter().zip(&run.decisions) {
        assert_eq!(
            attribute(span, "lineage.id"),
            Some(Value::from(decision.id.to_string()))
        );
        let caused_by = format!("{},{}", decision.caused_by[0], decision.caused_by[1]);
        assert_eq!(
            attribute(span, "lineage.caused_by"),
            Some(Value::from(caused_by))
        );
        assert_eq!(span.parent_span_id, parent.span_context.span_id());
    }
}
