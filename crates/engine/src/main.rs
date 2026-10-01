//! The Base Chain Engine binary.
//!
//!   engine follow [--rpc URL] [--call-rpc URL] [--minutes N] [--kafka BROKERS] [--otlp URL]
//!                 [--check-every N]
//!   engine replay --core-instance ID (--kafka BROKERS | --from-archive)
//!   engine archive --kafka BROKERS --core-instance ID
//!
//! `follow` runs the core on the live chain. With `--kafka` it records inputs to
//! `inputs.base` and publishes pool updates to `pool-updates.base` and trade records to
//! `trades.base`; otherwise all stay in memory. If `--rpc` or `--call-rpc` can't answer, before the run starts or during it, it
//! stops with an error naming that flag (D88). `replay` runs the core again from a recording,
//! publishing nothing. `archive` copies a recording from Kafka to the object-storage archive
//! (D54, D72).

use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use det::archive::{ArchiveError, InputArchive, S3Config};
use det::kafka::KafkaPublisher;
use det::kafka::{KafkaSink, read_input_log};
use det::{
    ChannelEventSource, ChannelRpc, InMemorySink, Recorder, RecordingClock, RecordingEventSource,
    RecordingRpc, Replay, SeededRng, SystemClock,
};
use engine::{
    Engine, EngineConfig, INPUT_TOPIC, InMemoryOutbox, KafkaOutbox, M1_TOPICS, POOL_UPDATES_TOPIC,
    Summary, TRADES_TOPIC,
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Follow {
        /// Where blocks and logs are read. Checked before the run starts (D88).
        #[arg(long, default_value = chain_io::BASE_PUBLIC_RPC)]
        rpc: String,
        /// Where `eth_call`s go (D82). Checked before the run starts (D88).
        #[arg(long, default_value = chain_io::BASE_PUBLICNODE_RPC)]
        call_rpc: String,
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
        /// Most `eth_call`s started per second.
        #[arg(long, default_value_t = 5)]
        calls_per_second: u32,
    },
    Replay {
        /// Read the recording from Kafka at these brokers.
        #[arg(long, required_unless_present = "from_archive")]
        kafka: Option<String>,
        /// Read the recording from the object-storage archive instead.
        #[arg(long)]
        from_archive: bool,
        #[arg(long)]
        core_instance: String,
        #[command(flatten)]
        s3: S3Args,
    },
    Archive {
        #[arg(long)]
        kafka: String,
        #[arg(long)]
        core_instance: String,
        #[command(flatten)]
        s3: S3Args,
    },
}

/// Where the input-log archive lives. Defaults are the local stack's RustFS (D75).
#[derive(clap::Args)]
struct S3Args {
    #[arg(long, default_value = "http://localhost:9000")]
    s3_endpoint: String,
    #[arg(long, default_value = "omnimarket-inputs")]
    s3_bucket: String,
    // hide_env_values: clap would otherwise print the keys' current values in --help.
    #[arg(
        long,
        env = "OMNIMARKET_S3_ACCESS_KEY",
        hide_env_values = true,
        default_value = "omnimarket"
    )]
    s3_access_key: String,
    #[arg(
        long,
        env = "OMNIMARKET_S3_SECRET_KEY",
        hide_env_values = true,
        default_value = "omnimarket-dev"
    )]
    s3_secret_key: String,
}

impl S3Args {
    fn archive(&self) -> Result<InputArchive, ArchiveError> {
        let config = S3Config {
            endpoint: self.s3_endpoint.clone(),
            bucket: self.s3_bucket.clone(),
            access_key: self.s3_access_key.clone(),
            secret_key: self.s3_secret_key.clone(),
        };
        InputArchive::s3(&config, ARCHIVE_PREFIX)
    }
}

/// Base's recordings in the archive: `inputs/base/<core instance>/<first seq>.pb`.
const ARCHIVE_PREFIX: &str = "inputs/base";

/// For the archive's async client; the core never runs here.
fn io_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Its message, not the `Debug` form that returning it from `main` would print.
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Follow {
            rpc,
            call_rpc,
            minutes,
            kafka,
            otlp,
            check_every,
            calls_per_second,
        } => {
            let config = EngineConfig {
                check_every,
                ..EngineConfig::base()
            };
            follow(
                rpc,
                call_rpc,
                minutes,
                kafka,
                otlp,
                config,
                calls_per_second,
            )
        }
        Command::Archive {
            kafka,
            core_instance,
            s3,
        } => {
            let recording = read_input_log(&kafka, INPUT_TOPIC, &core_instance, TIMEOUT)?;
            let archive = s3.archive()?;
            let segments = io_runtime()?.block_on(archive.write(&core_instance, &recording))?;
            println!(
                "archived {} inputs of {core_instance} in {segments} segments under {}/{ARCHIVE_PREFIX}/{core_instance}/",
                recording.len(),
                s3.s3_bucket
            );
            Ok(())
        }
        Command::Replay {
            kafka,
            from_archive,
            core_instance,
            s3,
        } => {
            let recording = match (from_archive, kafka) {
                (true, _) => io_runtime()?.block_on(s3.archive()?.read(&core_instance))?,
                (false, Some(kafka)) => {
                    read_input_log(&kafka, INPUT_TOPIC, &core_instance, TIMEOUT)?
                }
                (false, None) => return Err("give --kafka or --from-archive".into()),
            };
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
    call_rpc: String,
    minutes: u64,
    kafka: Option<String>,
    otlp: Option<String>,
    config: EngineConfig,
    calls_per_second: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let _telemetry = otlp
        .map(|endpoint| telemetry::init("omnimarket-engine-base", &endpoint))
        .transpose()?;

    // Both spawns check their endpoint before Kafka or the core start, so a dead one stops the
    // run in seconds instead of failing every read the core depends on (D88).
    let chain_io::CallWorker {
        requests: calls,
        gave_up: calls_gave_up,
        thread: call_worker,
    } = chain_io::spawn_call_worker(&call_rpc, calls_per_second)
        .map_err(|error| unusable_endpoint("call", "--call-rpc", &call_rpc, &error))?;
    let (sender, receiver) = tokio::sync::mpsc::channel(1024);
    let follower = chain_io::spawn_head_follower(
        &rpc,
        chain_io::FollowerConfig {
            poll: Duration::from_millis(500),
            topics: M1_TOPICS.to_vec(),
            start: chain_io::Start::Latest,
            run_for: Some(Duration::from_secs(minutes * 60)),
        },
        sender,
    )
    .map_err(|error| unusable_endpoint("block", "--rpc", &rpc, &error))?;

    let core_instance = format!("base-{:016x}", SeededRng::from_os().seed());
    let in_memory = InMemorySink::default();
    let kafka_sink = match &kafka {
        Some(brokers) => {
            det::kafka::ensure_topic(brokers, INPUT_TOPIC)?;
            det::kafka::ensure_topic(brokers, POOL_UPDATES_TOPIC)?;
            det::kafka::ensure_topic(brokers, TRADES_TOPIC)?;
            Some((
                KafkaSink::new(brokers, INPUT_TOPIC, &core_instance)?,
                KafkaPublisher::new(brokers, POOL_UPDATES_TOPIC)?,
                KafkaPublisher::new(brokers, TRADES_TOPIC)?,
            ))
        }
        None => None,
    };
    let outbox: Box<dyn engine::Outbox> = match &kafka_sink {
        Some((_, updates, trades)) => Box::new(KafkaOutbox::new(updates.clone(), trades.clone())),
        None => Box::new(InMemoryOutbox::default()),
    };
    let sink: Box<dyn det::RecordingSink> = match &kafka_sink {
        Some((sink, _, _)) => Box::new(sink.clone()),
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

    // The core is one task on a current-thread runtime (D74); the follower and the call worker
    // have their own threads. If the call worker gives up on its endpoint mid-run, the core is
    // dropped here, unfinished, and the run stops with the worker's error. If the follower gives
    // up on its endpoint, the core's blocks end and it finishes what's in flight; the run then
    // stops with the follower's error below (D88).
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let outcome: Result<Summary, Box<dyn std::error::Error>> = runtime.block_on(async {
        tokio::select! {
            biased;
            Ok(error) = calls_gave_up => {
                Err(unusable_endpoint("call", "--call-rpc", &call_rpc, &error).into())
            }
            summary = engine.run() => summary.map_err(Into::into),
        }
    });
    let summary = outcome?;
    follower
        .join()
        .expect("the follower thread doesn't panic")
        .map_err(|error| unusable_endpoint("block", "--rpc", &rpc, &error))?;
    // The engine is gone, so the call worker's channel is closed and it stops.
    call_worker
        .join()
        .expect("the call worker thread doesn't panic");

    println!("core instance {core_instance}");
    match kafka_sink {
        Some((sink, updates, trades)) => {
            sink.flush(TIMEOUT)?;
            updates.flush(TIMEOUT)?;
            trades.flush(TIMEOUT)?;
            println!(
                "inputs recorded to {INPUT_TOPIC}, pool updates on {POOL_UPDATES_TOPIC}, trades on {TRADES_TOPIC}"
            );
        }
        None => println!("{} inputs recorded in memory", in_memory.records().len()),
    }
    print_summary(&summary);
    Ok(())
}

/// What `follow` stops with when the endpoint `flag` names can't answer (D88).
fn unusable_endpoint(what: &str, flag: &str, url: &str, error: &chain_io::ChainError) -> String {
    format!(
        "the {what} endpoint {url} is unusable: {error}\nPass {flag} with another Base RPC URL."
    )
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
        "v2: {} pairs tracked, {} rejected, {} verification calls ({} failed)",
        s.pairs_tracked, s.pairs_rejected, s.verify_calls, s.verify_failures
    );
    println!(
        "v3: {} pools tracked, {} rejected, {} bootstrap calls ({} bootstraps failed)",
        s.pools_tracked, s.pools_rejected, s.bootstrap_calls, s.bootstrap_failures
    );
    println!(
        "{} pool updates, digest {}",
        s.updates, summary.updates_digest
    );
    println!(
        "{} trades ({} dropped with a failed read), digest {}",
        s.trades, s.trades_dropped, summary.trades_digest
    );
    println!(
        "shadow checks: {} passed, {} failed",
        s.checks_passed, s.checks_failed
    );
}
