//! What the binary's `follow`, `archive` and `replay` do, given their endpoints and sinks, so a
//! test can run them without the network. `main.rs` parses the flags and opens the real ones.

use std::fmt;
use std::thread::JoinHandle;
use std::time::Duration;

use chain_io::{CallWorker, ChainError, LogsWorker};
use det::archive::{ArchiveError, InputArchive};
use det::kafka::{InputLogError, KafkaPublisher, KafkaSink, read_input_log};
use det::{
    ChannelEventSource, ChannelRpc, InMemorySink, InputRecord, Recorder, RecordingClock,
    RecordingEventSource, RecordingRpc, Replay, SystemClock,
};
use types::chain::Block;

use crate::{
    Engine, EngineConfig, EngineError, EngineMetrics, INPUT_TOPIC, InMemoryOutbox, KafkaOutbox,
    KafkaTopics, Mode, Outbox, POOL_UPDATES_TOPIC, PRICES_TOPIC, Run, STATUS_TOPIC, Summary,
    TRADE_HISTORY_TOPIC, TRADES_TOPIC,
};

/// How long Kafka gets to deliver or return a recording.
pub const KAFKA_TIMEOUT: Duration = Duration::from_secs(30);

/// The chain I/O a `follow` runs on, each part on its own thread (rule 6).
pub struct Endpoints {
    pub blocks: tokio::sync::mpsc::Receiver<Block>,
    /// The thread sending `blocks`. It returns its endpoint's error if it gave up (D88).
    pub follower: JoinHandle<Result<(), ChainError>>,
    pub calls: CallWorker,
    /// Answers trade-history reads. Only a config with `history_blocks` reads any.
    pub logs: Option<LogsWorker>,
}

#[derive(Debug)]
pub enum FollowError {
    /// The call endpoint gave up mid-run, and the core stopped unfinished.
    Calls(ChainError),
    /// The block endpoint gave up. The core finished the blocks it had.
    Blocks(ChainError),
    Kafka(InputLogError),
    Engine(EngineError),
    Runtime(std::io::Error),
}

impl fmt::Display for FollowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Calls(e) => write!(f, "the call endpoint gave up: {e}"),
            Self::Blocks(e) => write!(f, "the block endpoint gave up: {e}"),
            Self::Kafka(e) => write!(f, "{e}"),
            Self::Engine(e) => write!(f, "{e}"),
            Self::Runtime(e) => write!(f, "the core's runtime: {e}"),
        }
    }
}

impl std::error::Error for FollowError {}

impl From<InputLogError> for FollowError {
    fn from(e: InputLogError) -> Self {
        Self::Kafka(e)
    }
}

/// A finished `follow`.
pub struct Followed {
    pub summary: Summary,
    /// The inputs, when they weren't recorded to Kafka. Empty when they were.
    pub recording: Vec<InputRecord>,
}

/// Runs the core on `endpoints` until the follower stops. With `kafka` (brokers), it records the
/// inputs to `inputs.base` under `core_instance` and publishes on the engine's topics; without,
/// both stay in memory. The config is the recording's first input, so a replay can rebuild
/// the core from the recording alone.
pub fn follow(
    endpoints: Endpoints,
    kafka: Option<&str>,
    core_instance: &str,
    config: EngineConfig,
    metrics: &EngineMetrics,
) -> Result<Followed, FollowError> {
    let Endpoints {
        blocks,
        follower,
        calls,
        logs,
    } = endpoints;
    let in_memory = InMemorySink::default();
    let kafka_sink = match kafka {
        Some(brokers) => {
            for topic in [
                INPUT_TOPIC,
                POOL_UPDATES_TOPIC,
                TRADES_TOPIC,
                PRICES_TOPIC,
                STATUS_TOPIC,
                TRADE_HISTORY_TOPIC,
            ] {
                det::kafka::ensure_topic(brokers, topic)?;
            }
            Some((
                KafkaSink::new(brokers, INPUT_TOPIC, core_instance)?,
                KafkaTopics {
                    updates: KafkaPublisher::new(brokers, POOL_UPDATES_TOPIC)?,
                    trades: KafkaPublisher::new(brokers, TRADES_TOPIC)?,
                    prices: KafkaPublisher::new(brokers, PRICES_TOPIC)?,
                    statuses: KafkaPublisher::new(brokers, STATUS_TOPIC)?,
                    history: KafkaPublisher::new(brokers, TRADE_HISTORY_TOPIC)?,
                },
            ))
        }
        None => None,
    };
    let outbox: Box<dyn Outbox> = match &kafka_sink {
        Some((_, topics)) => Box::new(KafkaOutbox::new(
            topics.clone(),
            // With Kafka, the inputs always go to `inputs.base`.
            Run {
                mode: Mode::Live,
                core_instance: core_instance.to_string(),
                recording: true,
            },
        )),
        None => Box::new(InMemoryOutbox::default()),
    };
    let outbox = metrics.outbox(outbox);
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
            Box::new(ChannelEventSource::new(blocks)),
            recorder.clone(),
        )),
        Box::new(RecordingRpc::new(
            Box::new(ChannelRpc::new(calls.requests)),
            recorder.clone(),
        )),
        outbox,
    );
    let (engine, logs_thread) = match logs {
        Some(worker) => (
            engine.with_logs(Box::new(RecordingRpc::logs(
                Box::new(ChannelRpc::new(worker.requests)),
                recorder,
            ))),
            Some(worker.thread),
        ),
        None => (engine, None),
    };

    // The core is one task on a current-thread runtime (D74); the follower and the call worker
    // have their own threads. If the call worker gives up on its endpoint mid-run, the core is
    // dropped here, unfinished, and the run stops with the worker's error. If the follower gives
    // up on its endpoint, the core's blocks end and it finishes what's in flight; the run then
    // stops with the follower's error below (D88).
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(FollowError::Runtime)?;
    let calls_gave_up = calls.gave_up;
    let summary = runtime.block_on(async {
        tokio::select! {
            biased;
            Ok(error) = calls_gave_up => Err(FollowError::Calls(error)),
            summary = engine.run() => summary.map_err(FollowError::Engine),
        }
    })?;
    follower
        .join()
        .expect("the follower thread doesn't panic")
        .map_err(FollowError::Blocks)?;
    // The engine is gone, so the call and logs workers' channels are closed and they stop.
    calls
        .thread
        .join()
        .expect("the call worker thread doesn't panic");
    if let Some(thread) = logs_thread {
        thread.join().expect("the logs worker thread doesn't panic");
    }

    if let Some((sink, topics)) = kafka_sink {
        sink.flush(KAFKA_TIMEOUT)?;
        topics.updates.flush(KAFKA_TIMEOUT)?;
        topics.trades.flush(KAFKA_TIMEOUT)?;
        topics.prices.flush(KAFKA_TIMEOUT)?;
        topics.statuses.flush(KAFKA_TIMEOUT)?;
        topics.history.flush(KAFKA_TIMEOUT)?;
    }
    Ok(Followed {
        summary,
        recording: in_memory.records(),
    })
}

#[derive(Debug)]
pub enum RecordingError {
    Kafka(InputLogError),
    Archive(ArchiveError),
    Runtime(std::io::Error),
}

impl fmt::Display for RecordingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kafka(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::Runtime(e) => write!(f, "the archive's runtime: {e}"),
        }
    }
}

impl std::error::Error for RecordingError {}

/// For the archive's async client; the core never runs here.
fn io_runtime() -> Result<tokio::runtime::Runtime, RecordingError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(RecordingError::Runtime)
}

/// A core instance's recording, from `inputs.base` at `brokers`.
pub fn recording_from_kafka(
    brokers: &str,
    core_instance: &str,
) -> Result<Vec<InputRecord>, RecordingError> {
    read_input_log(brokers, INPUT_TOPIC, core_instance, KAFKA_TIMEOUT)
        .map_err(RecordingError::Kafka)
}

/// A core instance's recording, from the input-log archive.
pub fn recording_from_archive(
    archive: &InputArchive,
    core_instance: &str,
) -> Result<Vec<InputRecord>, RecordingError> {
    io_runtime()?
        .block_on(archive.read(core_instance))
        .map_err(RecordingError::Archive)
}

/// Copies a core instance's recording from Kafka to the archive (D54, D72). Returns how many
/// inputs it copied, and in how many segments.
pub fn archive(
    brokers: &str,
    core_instance: &str,
    archive: &InputArchive,
) -> Result<(usize, usize), RecordingError> {
    let recording = recording_from_kafka(brokers, core_instance)?;
    let segments = io_runtime()?
        .block_on(archive.write(core_instance, &recording))
        .map_err(RecordingError::Archive)?;
    Ok((recording.len(), segments))
}

#[derive(Debug)]
pub enum ReplayError {
    /// A replay builds the core from the config the recording starts with.
    NoConfig,
    Engine(EngineError),
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoConfig => write!(f, "the recording doesn't start with an engine config"),
            Self::Engine(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ReplayError {}

/// Runs the core again from a recording, publishing nothing.
pub fn replay(recording: Vec<InputRecord>) -> Result<Summary, ReplayError> {
    let replay = Replay::new(recording);
    let config = replay
        .config()
        .and_then(|bytes| EngineConfig::decode(&bytes))
        .ok_or(ReplayError::NoConfig)?;
    let engine = Engine::new(
        config,
        Box::new(replay.clock()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
        Box::new(InMemoryOutbox::default()),
    )
    .with_logs(Box::new(replay.logs()));
    replay.run(engine.run()).map_err(ReplayError::Engine)
}
