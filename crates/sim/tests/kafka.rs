//! The M0 demo through the real input log: a seed recorded to Kafka replays from Kafka.
//! Needs a broker: `scripts/stack up` locally (or set KAFKA_BROKERS); CI runs it against a
//! Kafka service container.

use std::time::Duration;

use det::SeededRng;
use det::kafka::{KafkaSink, ensure_topic, read_input_log};

#[test]
#[ignore = "needs Kafka: scripts/stack up, then cargo test -p sim --test kafka -- --ignored"]
fn a_seed_recorded_to_kafka_replays_to_the_same_digest() {
    let brokers = std::env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".into());
    let timeout = Duration::from_secs(30);
    ensure_topic(&brokers, sim::INPUT_TOPIC).expect("input topic exists");

    // A fresh core instance per run, so earlier runs' records on the topic are skipped.
    let core_instance = format!("sim-test-{:016x}", SeededRng::from_os().seed());
    let sink = KafkaSink::new(&brokers, sim::INPUT_TOPIC, &core_instance).expect("producer");
    let decisions = sim::run_recorded(7, Box::new(sink.clone()));
    sink.flush(timeout).expect("every input record delivered");

    let recording =
        read_input_log(&brokers, sim::INPUT_TOPIC, &core_instance, timeout).expect("log reads");
    let in_memory = sim::run(7);
    assert_eq!(recording, in_memory.recording);
    assert_eq!(
        sim::digest(&sim::replay(recording)),
        sim::digest(&decisions)
    );
    assert_eq!(sim::digest(&decisions), in_memory.digest);
}
