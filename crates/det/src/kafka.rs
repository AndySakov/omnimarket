//! The input log on Kafka (D54, D72): a recording sink that writes a core's inputs to
//! `inputs.<chain>`, and a reader that loads one core instance's records back for replay.

use std::fmt;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Duration;

use prost::Message as _;
use rdkafka::ClientConfig;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::error::{KafkaError, RDKafkaErrorCode};
use rdkafka::message::Message as _;
use rdkafka::producer::{BaseProducer, BaseRecord, DeliveryResult, Producer, ProducerContext};
use rdkafka::{ClientContext, Offset, TopicPartitionList};

use crate::record::{InputRecord, InvalidInputRecord, RecordingSink};

#[derive(Debug)]
pub enum InputLogError {
    Kafka(KafkaError),
    /// A record the broker didn't accept. Every later record is suspect: replay needs them all.
    Delivery(KafkaError),
    Decode(prost::DecodeError),
    Invalid(InvalidInputRecord),
    /// Records for one core must come back in `seq` order with no gaps.
    OutOfOrder {
        expected: u64,
        found: u64,
    },
}

impl fmt::Display for InputLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kafka(e) => write!(f, "kafka: {e}"),
            Self::Delivery(e) => write!(f, "an input record wasn't delivered: {e}"),
            Self::Decode(e) => write!(f, "an input record doesn't decode: {e}"),
            Self::Invalid(e) => write!(f, "an input record is invalid: {e:?}"),
            Self::OutOfOrder { expected, found } => {
                write!(
                    f,
                    "input records out of order: expected seq {expected}, found {found}"
                )
            }
        }
    }
}

impl std::error::Error for InputLogError {}

impl From<KafkaError> for InputLogError {
    fn from(e: KafkaError) -> Self {
        Self::Kafka(e)
    }
}

/// Creates the input-log topic with one partition, so the log order is the core's input
/// order (D72). Does nothing if it already exists.
pub fn ensure_topic(brokers: &str, topic: &str) -> Result<(), InputLogError> {
    let admin: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .create()?;
    let new_topic = NewTopic::new(topic, 1, TopicReplication::Fixed(1));
    let results =
        futures::executor::block_on(admin.create_topics([&new_topic], &AdminOptions::new()))?;
    for result in results {
        match result {
            Ok(_) | Err((_, RDKafkaErrorCode::TopicAlreadyExists)) => {}
            Err((_, code)) => return Err(KafkaError::AdminOp(code).into()),
        }
    }
    Ok(())
}

/// Writes a core's input records to the input log, keyed by core instance.
///
/// `write` can't fail (the recording wrappers have nowhere to send an error), so delivery
/// failures are kept and reported by `flush`. Clones share one producer.
#[derive(Clone)]
pub struct KafkaSink {
    inner: Rc<KafkaSinkInner>,
}

struct KafkaSinkInner {
    producer: BaseProducer<DeliveryErrors>,
    topic: String,
    core_instance: String,
}

/// Keeps the first delivery error. librdkafka reports deliveries through this context from
/// `poll`, which needs it `Send + Sync`, hence the `Mutex` in a single-threaded core.
#[derive(Default)]
struct DeliveryErrors {
    first: Mutex<Option<KafkaError>>,
}

impl DeliveryErrors {
    fn keep(&self, error: KafkaError) {
        let mut first = self.first.lock().expect("delivery error lock");
        first.get_or_insert(error);
    }

    fn take(&self) -> Option<KafkaError> {
        self.first.lock().expect("delivery error lock").take()
    }
}

impl ClientContext for DeliveryErrors {}

impl ProducerContext for DeliveryErrors {
    type DeliveryOpaque = ();

    fn delivery(&self, result: &DeliveryResult<'_>, _: ()) {
        if let Err((error, _)) = result {
            self.keep(error.clone());
        }
    }
}

impl KafkaSink {
    pub fn new(brokers: &str, topic: &str, core_instance: &str) -> Result<Self, InputLogError> {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            // Retries can't reorder or duplicate records, so the log keeps the core's order.
            .set("enable.idempotence", "true")
            .create_with_context(DeliveryErrors::default())?;
        Ok(Self {
            inner: Rc::new(KafkaSinkInner {
                producer,
                topic: topic.to_string(),
                core_instance: core_instance.to_string(),
            }),
        })
    }

    /// Waits until every record written so far is acknowledged, then reports the first
    /// record that wasn't.
    pub fn flush(&self, timeout: Duration) -> Result<(), InputLogError> {
        self.inner.producer.flush(timeout)?;
        match self.inner.producer.context().take() {
            Some(error) => Err(InputLogError::Delivery(error)),
            None => Ok(()),
        }
    }
}

impl RecordingSink for KafkaSink {
    fn write(&self, record: InputRecord) {
        let inner = &self.inner;
        let payload = record.to_proto().encode_to_vec();
        let mut message = BaseRecord::to(&inner.topic)
            .key(&inner.core_instance)
            .payload(&payload);
        loop {
            match inner.producer.send(message) {
                Ok(()) => break,
                // The local queue is full: serve deliveries to make room, then retry. This
                // slows the core down rather than dropping an input.
                Err((KafkaError::MessageProduction(RDKafkaErrorCode::QueueFull), back)) => {
                    inner.producer.poll(Duration::from_millis(10));
                    message = back;
                }
                Err((error, _)) => {
                    inner.producer.context().keep(error);
                    break;
                }
            }
        }
        inner.producer.poll(Duration::ZERO);
    }
}

/// Reads every record one core instance wrote to `topic` (partition 0), in log order.
pub fn read_input_log(
    brokers: &str,
    topic: &str,
    core_instance: &str,
    timeout: Duration,
) -> Result<Vec<InputRecord>, InputLogError> {
    let consumer: BaseConsumer = ClientConfig::new()
        .set("bootstrap.servers", brokers)
        .set("group.id", "omnimarket-replay")
        .set("enable.auto.commit", "false")
        .create()?;
    let (low, high) = consumer.fetch_watermarks(topic, 0, timeout)?;
    let mut partitions = TopicPartitionList::new();
    partitions.add_partition_offset(topic, 0, Offset::Offset(low))?;
    consumer.assign(&partitions)?;

    let mut records = Vec::new();
    let mut next = low;
    while next < high {
        let Some(message) = consumer.poll(timeout) else {
            return Err(KafkaError::Global(RDKafkaErrorCode::OperationTimedOut).into());
        };
        let message = message?;
        next = message.offset() + 1;
        if message.key() != Some(core_instance.as_bytes()) {
            continue;
        }
        let bytes = message.payload().unwrap_or_default();
        let proto = proto::det::v1::InputRecord::decode(bytes).map_err(InputLogError::Decode)?;
        let record = InputRecord::from_proto(proto).map_err(InputLogError::Invalid)?;
        let expected = records.len() as u64;
        if record.seq != expected {
            return Err(InputLogError::OutOfOrder {
                expected,
                found: record.seq,
            });
        }
        records.push(record);
    }
    Ok(records)
}
