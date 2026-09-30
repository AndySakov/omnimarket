//! The Base Chain Engine binary.
//!
//!   engine follow [--rpc URL] [--minutes N] [--kafka BROKERS] [--otlp URL]
//!   engine replay --kafka BROKERS --core-instance ID
//!
//! `follow` runs the core on the live chain and records its inputs: to Kafka's `inputs.base`
//! with `--kafka`, otherwise to memory. `replay` runs the core again from a recording.

use std::time::Duration;

use clap::{Parser, Subcommand};
use det::kafka::{KafkaSink, read_input_log};
use det::{
    ChannelEventSource, InMemorySink, Recorder, RecordingClock, RecordingEventSource, Replay,
    SeededRng, SystemClock,
};
use engine::{Engine, INPUT_TOPIC, M1_TOPICS, Summary};

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
        } => follow(rpc, minutes, kafka, otlp),
        Command::Replay {
            kafka,
            core_instance,
        } => {
            let recording = read_input_log(&kafka, INPUT_TOPIC, &core_instance, TIMEOUT)?;
            println!("replaying {} inputs of {core_instance}", recording.len());
            let replay = Replay::new(recording);
            let engine = Engine::new(Box::new(replay.clock()), Box::new(replay.events()));
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
) -> Result<(), Box<dyn std::error::Error>> {
    let _telemetry = otlp
        .map(|endpoint| telemetry::init("omnimarket-engine-base", &endpoint))
        .transpose()?;

    let (sender, receiver) = tokio::sync::mpsc::channel(1024);
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
            Some(KafkaSink::new(brokers, INPUT_TOPIC, &core_instance)?)
        }
        None => None,
    };
    let sink: Box<dyn det::RecordingSink> = match &kafka_sink {
        Some(sink) => Box::new(sink.clone()),
        None => Box::new(in_memory.clone()),
    };
    let recorder = Recorder::new(sink, Box::new(SystemClock));
    let engine = Engine::new(
        Box::new(RecordingClock::new(Box::new(SystemClock), recorder.clone())),
        Box::new(RecordingEventSource::new(
            Box::new(ChannelEventSource::new(receiver)),
            recorder,
        )),
    );

    // The core is one task on a current-thread runtime (D74); the follower has its own thread.
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let summary = runtime.block_on(engine.run())?;
    follower
        .join()
        .expect("the follower thread doesn't panic")?;

    println!("core instance {core_instance}");
    match kafka_sink {
        Some(sink) => {
            sink.flush(TIMEOUT)?;
            println!("inputs recorded to {INPUT_TOPIC}");
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
    println!(
        "{} blocks, {} logs, {} reorgs detected, digest {}",
        summary.blocks, summary.logs, summary.reorgs_detected, summary.digest
    );
}
