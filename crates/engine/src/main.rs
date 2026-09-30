//! The Base Chain Engine binary.
//!
//!   engine follow [--rpc URL] [--minutes N] [--kafka BROKERS] [--otlp URL] [--check-every N]
//!   engine replay --kafka BROKERS --core-instance ID
//!
//! `follow` runs the core on the live chain. With `--kafka` it records inputs to
//! `inputs.base` and publishes pool updates to `pool-updates.base`; otherwise both stay in
//! memory. `replay` runs the core again from a recording, publishing nothing.

use std::time::Duration;

use clap::{Parser, Subcommand};
use det::kafka::KafkaPublisher;
use det::kafka::{KafkaSink, read_input_log};
use det::{
    ChannelEventSource, ChannelRpc, InMemorySink, Recorder, RecordingClock, RecordingEventSource,
    RecordingRpc, Replay, SeededRng, SystemClock,
};
use engine::{
    Engine, EngineConfig, INPUT_TOPIC, InMemoryOutbox, KafkaOutbox, M1_TOPICS, POOL_UPDATES_TOPIC,
    Summary,
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Follow {
        #[arg(long, default_value = chain_io::BASE_PUBLIC_RPC)]
        rpc: String,
        #[arg(long, default_value_t = 10)]
        minutes: u64,
        /// Record inputs to Kafka's `inputs.base` at these brokers.
        #[arg(long)]
        kafka: Option<String>,
        /// Export traces here (OTLP over HTTP), e.g. the local stack's Tempo.
        #[arg(long)]
        otlp: Option<String>,
        /// Compare a sample of pools with the chain every this many blocks.
        #[arg(long)]
        check_every: Option<u64>,
    },
    Replay {
        #[arg(long)]
        kafka: String,
        #[arg(long)]
        core_instance: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Follow {
            rpc,
            minutes,
            kafka,
            otlp,
            check_every,
        } => {
            let config = EngineConfig {
                check_every,
                ..EngineConfig::base()
            };
            follow(rpc, minutes, kafka, otlp, config)
        }
        Command::Replay {
            kafka,
            core_instance,
        } => {
            let recording = read_input_log(&kafka, INPUT_TOPIC, &core_instance, TIMEOUT)?;
            println!("replaying {} inputs of {core_instance}", recording.len());
            let replay = Replay::new(recording);
            let config = replay
                .config()
                .and_then(|bytes| EngineConfig::decode(&bytes))
                .ok_or("the recording doesn't start with an engine config")?;
            let engine = Engine::new(
                config,
                Box::new(replay.clock()),
                Box::new(replay.events()),
                Box::new(replay.rpc()),
                Box::new(InMemoryOutbox::default()),
            );
            print_summary(&replay.run(engine.run())?);
            Ok(())
        }
    }
}

const TIMEOUT: Duration = Duration::from_secs(30);

fn follow(
    rpc: String,
    minutes: u64,
    kafka: Option<String>,
    otlp: Option<String>,
    config: EngineConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let _telemetry = otlp
        .map(|endpoint| telemetry::init("omnimarket-engine-base", &endpoint))
        .transpose()?;

    let (sender, receiver) = tokio::sync::mpsc::channel(1024);
    let (calls, call_worker) = chain_io::spawn_call_worker(rpc.clone());
    let follower = chain_io::spawn_head_follower(
        rpc,
        chain_io::FollowerConfig {
            poll: Duration::from_millis(500),
            topics: M1_TOPICS.to_vec(),
            start: chain_io::Start::Latest,
            run_for: Some(Duration::from_secs(minutes * 60)),
        },
        sender,
    );

    let core_instance = format!("base-{:016x}", SeededRng::from_os().seed());
    let in_memory = InMemorySink::default();
    let kafka_sink = match &kafka {
        Some(brokers) => {
            det::kafka::ensure_topic(brokers, INPUT_TOPIC)?;
            det::kafka::ensure_topic(brokers, POOL_UPDATES_TOPIC)?;
            Some((
                KafkaSink::new(brokers, INPUT_TOPIC, &core_instance)?,
                KafkaPublisher::new(brokers, POOL_UPDATES_TOPIC)?,
            ))
        }
        None => None,
    };
    let outbox: Box<dyn engine::Outbox> = match &kafka_sink {
        Some((_, publisher)) => Box::new(KafkaOutbox::new(publisher.clone())),
        None => Box::new(InMemoryOutbox::default()),
    };
    let sink: Box<dyn det::RecordingSink> = match &kafka_sink {
        Some((sink, _)) => Box::new(sink.clone()),
        None => Box::new(in_memory.clone()),
    };
    let recorder = Recorder::new(sink, Box::new(SystemClock));
    recorder.record_config(config.encode());
    let engine = Engine::new(
        config,
        Box::new(RecordingClock::new(Box::new(SystemClock), recorder.clone())),
        Box::new(RecordingEventSource::new(
            Box::new(ChannelEventSource::new(receiver)),
            recorder.clone(),
        )),
        Box::new(RecordingRpc::new(
            Box::new(ChannelRpc::new(calls)),
            recorder,
        )),
        outbox,
    );

    // The core is one task on a current-thread runtime (D74); the follower has its own thread.
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let summary = runtime.block_on(engine.run())?;
    follower
        .join()
        .expect("the follower thread doesn't panic")?;
    // The engine is gone, so the call worker's channel is closed and it stops.
    call_worker
        .join()
        .expect("the call worker thread doesn't panic")?;

    println!("core instance {core_instance}");
    match kafka_sink {
        Some((sink, publisher)) => {
            sink.flush(TIMEOUT)?;
            publisher.flush(TIMEOUT)?;
            println!("inputs recorded to {INPUT_TOPIC}, pool updates on {POOL_UPDATES_TOPIC}");
        }
        None => println!("{} inputs recorded in memory", in_memory.records().len()),
    }
    print_summary(&summary);
    Ok(())
}

fn print_summary(summary: &Summary) {
    if let Some((number, hash)) = summary.head {
        println!("head {number} {hash}");
    }
    let s = &summary.stats;
    println!(
        "{} blocks, {} logs, {} reorgs detected, digest {}",
        s.blocks, s.logs, s.reorgs_detected, summary.digest
    );
    println!(
        "v2: {} pairs tracked, {} rejected, {} verification calls ({} failed), {} updates, digest {}",
        s.pairs_tracked,
        s.pairs_rejected,
        s.verify_calls,
        s.verify_failures,
        s.updates,
        summary.updates_digest
    );
    println!(
        "shadow checks: {} passed, {} failed",
        s.checks_passed, s.checks_failed
    );
}
