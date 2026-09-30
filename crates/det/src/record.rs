//! Recording wrappers (rule 3): each wraps a det implementation and writes every input it
//! hands the core to a recording sink, in the order the core receives them (D72).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use futures::future::LocalBoxFuture;
use types::Timestamp;

use crate::{Clock, EventSource, Rng, Rpc};

/// Which det trait an input came through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The core's configuration, recorded before anything else (D54).
    Config,
    Clock,
    Rng,
    Event,
    Rpc,
}

/// One input a core received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputRecord {
    /// Position in the core's input order, across all its sources. Replay follows it.
    pub seq: u64,
    pub source: Source,
    /// When the input reached the core. For reading the log; replay doesn't use it.
    pub arrived: Timestamp,
    pub payload: Vec<u8>,
}

/// Why an input-log record can't be read back.
#[derive(Debug, PartialEq, Eq)]
pub enum InvalidInputRecord {
    UnknownSource(i32),
}

impl InputRecord {
    pub fn to_proto(&self) -> proto::det::v1::InputRecord {
        use proto::det::v1::InputSource;
        let source = match self.source {
            Source::Clock => InputSource::Clock,
            Source::Rng => InputSource::Rng,
            Source::Event => InputSource::Event,
            Source::Rpc => InputSource::Rpc,
            Source::Config => InputSource::Config,
        };
        proto::det::v1::InputRecord {
            seq: self.seq,
            source: source.into(),
            arrived_unix_nanos: self.arrived.unix_nanos,
            payload: self.payload.clone(),
        }
    }

    pub fn from_proto(record: proto::det::v1::InputRecord) -> Result<Self, InvalidInputRecord> {
        use proto::det::v1::InputSource;
        let source = match InputSource::try_from(record.source) {
            Ok(InputSource::Clock) => Source::Clock,
            Ok(InputSource::Rng) => Source::Rng,
            Ok(InputSource::Event) => Source::Event,
            Ok(InputSource::Rpc) => Source::Rpc,
            Ok(InputSource::Config) => Source::Config,
            Ok(InputSource::Unspecified) | Err(_) => {
                return Err(InvalidInputRecord::UnknownSource(record.source));
            }
        };
        Ok(Self {
            seq: record.seq,
            source,
            arrived: Timestamp::from_unix_nanos(record.arrived_unix_nanos),
            payload: record.payload,
        })
    }
}

/// How an event or an RPC response becomes record bytes and back.
pub trait Recordable: Sized {
    fn encode(&self) -> Vec<u8>;

    /// `None` if `bytes` isn't something `encode` produced.
    fn decode(bytes: &[u8]) -> Option<Self>;
}

pub trait RecordingSink {
    fn write(&self, record: InputRecord);
}

/// Keeps records in memory, for tests. Clones share one list: give one to the recorder and
/// keep one to read the records back.
#[derive(Clone, Default)]
pub struct InMemorySink {
    records: Rc<RefCell<Vec<InputRecord>>>,
}

impl InMemorySink {
    pub fn records(&self) -> Vec<InputRecord> {
        self.records.borrow().clone()
    }
}

impl RecordingSink for InMemorySink {
    fn write(&self, record: InputRecord) {
        self.records.borrow_mut().push(record);
    }
}

/// Numbers inputs and writes them to a sink. All of one core's recording wrappers share a
/// recorder, so `seq` orders inputs across every source the core reads.
#[derive(Clone)]
pub struct Recorder {
    state: Rc<RecorderState>,
}

struct RecorderState {
    sink: Box<dyn RecordingSink>,
    clock: Box<dyn Clock>,
    next_seq: Cell<u64>,
}

impl Recorder {
    /// `clock` stamps arrival times. Pass the unwrapped clock: stamping with a recording
    /// clock would record a clock read the core never made.
    pub fn new(sink: Box<dyn RecordingSink>, clock: Box<dyn Clock>) -> Self {
        Self {
            state: Rc::new(RecorderState {
                sink,
                clock,
                next_seq: Cell::new(0),
            }),
        }
    }

    /// Records the core's configuration. Call it once, before the core runs, so a replay can
    /// rebuild the same core from the log alone.
    pub fn record_config(&self, config: Vec<u8>) {
        self.write(Source::Config, config);
    }

    fn write(&self, source: Source, payload: Vec<u8>) {
        let seq = self.state.next_seq.get();
        self.state.next_seq.set(seq + 1);
        self.state.sink.write(InputRecord {
            seq,
            source,
            arrived: self.state.clock.now(),
            payload,
        });
    }
}

pub struct RecordingClock {
    inner: Box<dyn Clock>,
    recorder: Recorder,
}

impl RecordingClock {
    pub fn new(inner: Box<dyn Clock>, recorder: Recorder) -> Self {
        Self { inner, recorder }
    }
}

impl Clock for RecordingClock {
    fn now(&self) -> Timestamp {
        let now = self.inner.now();
        self.recorder
            .write(Source::Clock, encode_u64(now.unix_nanos));
        now
    }
}

pub struct RecordingRng {
    inner: Box<dyn Rng>,
    recorder: Recorder,
}

impl RecordingRng {
    pub fn new(inner: Box<dyn Rng>, recorder: Recorder) -> Self {
        Self { inner, recorder }
    }
}

impl Rng for RecordingRng {
    fn next_u64(&mut self) -> u64 {
        let draw = self.inner.next_u64();
        self.recorder.write(Source::Rng, encode_u64(draw));
        draw
    }
}

pub struct RecordingEventSource<E> {
    inner: Box<dyn EventSource<Event = E>>,
    recorder: Recorder,
}

impl<E> RecordingEventSource<E> {
    pub fn new(inner: Box<dyn EventSource<Event = E>>, recorder: Recorder) -> Self {
        Self { inner, recorder }
    }
}

impl<E: Recordable> EventSource for RecordingEventSource<E> {
    type Event = E;

    fn next(&mut self) -> LocalBoxFuture<'_, Option<E>> {
        let recorder = self.recorder.clone();
        let next = self.inner.next();
        Box::pin(async move {
            let event = next.await;
            // Written only once the event arrives, so a cancelled wait records nothing.
            recorder.write(Source::Event, encode_event(event.as_ref()));
            event
        })
    }
}

/// Numbers calls in the order the core makes them. Responses can arrive in any order;
/// the call number ties each one back to its call on replay.
pub struct RecordingRpc<Req, Resp> {
    inner: Box<dyn Rpc<Request = Req, Response = Resp>>,
    recorder: Recorder,
    calls: Cell<u64>,
}

impl<Req, Resp> RecordingRpc<Req, Resp> {
    pub fn new(inner: Box<dyn Rpc<Request = Req, Response = Resp>>, recorder: Recorder) -> Self {
        Self {
            inner,
            recorder,
            calls: Cell::new(0),
        }
    }
}

impl<Req, Resp: Recordable + 'static> Rpc for RecordingRpc<Req, Resp> {
    type Request = Req;
    type Response = Resp;

    fn call(&self, request: Req) -> LocalBoxFuture<'static, Resp> {
        let call = self.calls.get();
        self.calls.set(call + 1);
        let recorder = self.recorder.clone();
        let response = self.inner.call(request);
        Box::pin(async move {
            let response = response.await;
            recorder.write(Source::Rpc, encode_response(call, &response));
            response
        })
    }
}

// Payload layouts, shared with replay.

pub(crate) fn encode_u64(value: u64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

pub(crate) fn decode_u64(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

/// `[0]` when the source has ended, `[1, event…]` otherwise.
fn encode_event<E: Recordable>(event: Option<&E>) -> Vec<u8> {
    match event {
        None => vec![0],
        Some(event) => [&[1][..], &event.encode()].concat(),
    }
}

pub(crate) fn decode_event<E: Recordable>(bytes: &[u8]) -> Option<Option<E>> {
    match bytes.split_first()? {
        (0, []) => Some(None),
        (1, event) => E::decode(event).map(Some),
        _ => None,
    }
}

/// The call number (8 bytes, little-endian), then the response.
fn encode_response<R: Recordable>(call: u64, response: &R) -> Vec<u8> {
    [&call.to_le_bytes()[..], &response.encode()].concat()
}

pub(crate) fn response_call(bytes: &[u8]) -> Option<u64> {
    decode_u64(bytes.get(..8)?)
}

pub(crate) fn decode_response<R: Recordable>(bytes: &[u8]) -> Option<R> {
    R::decode(bytes.get(8..)?)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::FutureExt;

    use super::*;
    use crate::{SeededRng, SimClock, SimEventSource, SimRpc, run_simulated};

    impl Recordable for char {
        fn encode(&self) -> Vec<u8> {
            self.to_string().into_bytes()
        }

        fn decode(bytes: &[u8]) -> Option<Self> {
            let mut chars = std::str::from_utf8(bytes).ok()?.chars();
            let c = chars.next()?;
            chars.next().is_none().then_some(c)
        }
    }

    fn at(millis: u64) -> Timestamp {
        Timestamp::from_unix_nanos(millis * 1_000_000)
    }

    #[test]
    fn records_every_source_in_the_order_the_core_saw_it() {
        let sink = InMemorySink::default();
        run_simulated(async {
            let clock = SimClock::starting_at(at(0));
            let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock.clone()));
            let recorded_clock = RecordingClock::new(Box::new(clock), recorder.clone());
            let mut rng = RecordingRng::new(Box::new(SeededRng::from_seed(0)), recorder.clone());
            let mut events = RecordingEventSource::new(
                Box::new(SimEventSource::new(vec![(Duration::from_millis(10), 'a')])),
                recorder.clone(),
            );
            let rpc = RecordingRpc::new(
                Box::new(SimRpc::new(|c: char| (Duration::from_millis(5), c))),
                recorder,
            );

            assert_eq!(events.next().await, Some('a'));
            recorded_clock.now();
            let draw = rng.next_u64();
            assert_eq!(rpc.call('z').await, 'z');
            assert_eq!(events.next().await, None);

            let records = sink.records();
            let summary: Vec<_> = records
                .iter()
                .map(|r| (r.seq, r.source, r.arrived, r.payload.clone()))
                .collect();
            assert_eq!(
                summary,
                vec![
                    (0, Source::Event, at(10), vec![1, b'a']),
                    (1, Source::Clock, at(10), encode_u64(10_000_000)),
                    (2, Source::Rng, at(10), encode_u64(draw)),
                    (3, Source::Rpc, at(15), vec![0, 0, 0, 0, 0, 0, 0, 0, b'z']),
                    (4, Source::Event, at(15), vec![0]),
                ]
            );
        });
    }

    #[test]
    fn a_cancelled_event_wait_records_nothing() {
        let sink = InMemorySink::default();
        run_simulated(async {
            let clock = SimClock::starting_at(at(0));
            let recorder = Recorder::new(Box::new(sink.clone()), Box::new(clock));
            let mut events = RecordingEventSource::new(
                Box::new(SimEventSource::new(vec![(Duration::from_millis(10), 'a')])),
                recorder,
            );
            assert_eq!(events.next().now_or_never(), None);
            assert!(sink.records().is_empty());
            assert_eq!(events.next().await, Some('a'));
            assert_eq!(sink.records().len(), 1);
        });
    }

    #[test]
    fn input_records_round_trip_through_proto() {
        for source in [
            Source::Config,
            Source::Clock,
            Source::Rng,
            Source::Event,
            Source::Rpc,
        ] {
            let record = InputRecord {
                seq: 3,
                source,
                arrived: at(5),
                payload: vec![1, 2],
            };
            assert_eq!(InputRecord::from_proto(record.to_proto()), Ok(record));
        }
    }

    #[test]
    fn an_unspecified_source_is_rejected() {
        let record = proto::det::v1::InputRecord {
            source: 0,
            ..Default::default()
        };
        assert_eq!(
            InputRecord::from_proto(record),
            Err(InvalidInputRecord::UnknownSource(0))
        );
    }

    #[test]
    fn event_payloads_round_trip() {
        assert_eq!(
            decode_event::<char>(&encode_event(Some(&'q'))),
            Some(Some('q'))
        );
        assert_eq!(
            decode_event::<char>(&encode_event::<char>(None)),
            Some(None)
        );
        assert_eq!(decode_event::<char>(&[2, b'q']), None);
        assert_eq!(decode_event::<char>(&[0, b'q']), None);
    }
}
