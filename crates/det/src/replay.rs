//! Replay: plays a recording back through the det traits, in recorded order (D74).
//!
//! An input is available only when its record is next in the log, so the core has to ask
//! for its inputs in exactly the order it did when recorded. When it doesn't, the replay has
//! diverged: that is a bug in the core or a changed core, and replay panics with
//! "replay diverged" rather than feeding the core inputs it never saw.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::marker::PhantomData;
use std::rc::Rc;
use std::task::{Poll, Waker};
use std::time::Duration;

use futures::future::LocalBoxFuture;
use types::Timestamp;

use crate::record::{Recordable, decode_event, decode_response, decode_u64, response_call};
use crate::{Clock, EventSource, InputRecord, Rng, Rpc, Source, run_simulated};

/// Replay sources never wait on timers, so on the paused clock this one fires only when
/// the core is stuck waiting for an input that isn't next in the log.
const STALLED_AFTER: Duration = Duration::from_secs(24 * 3600);

/// A recording, handed out as det sources. The sources share one log.
#[derive(Clone)]
pub struct Replay {
    log: Rc<RefCell<ReplayLog>>,
}

struct ReplayLog {
    records: VecDeque<InputRecord>,
    waiting: Vec<Waker>,
}

impl Replay {
    /// `records` in log order.
    pub fn new(records: Vec<InputRecord>) -> Self {
        Self {
            log: Rc::new(RefCell::new(ReplayLog {
                records: records.into(),
                waiting: Vec::new(),
            })),
        }
    }

    pub fn clock(&self) -> ReplayClock {
        ReplayClock {
            replay: self.clone(),
        }
    }

    pub fn rng(&self) -> ReplayRng {
        ReplayRng {
            replay: self.clone(),
        }
    }

    pub fn events<E>(&self) -> ReplayEvents<E> {
        ReplayEvents {
            replay: self.clone(),
            event: PhantomData,
        }
    }

    pub fn rpc<Req, Resp>(&self) -> ReplayRpc<Req, Resp> {
        ReplayRpc {
            replay: self.clone(),
            calls: Cell::new(0),
            types: PhantomData,
        }
    }

    /// Runs `core`, built on this replay's sources, to completion on simulated time.
    /// Panics if the core stalls on an input the log doesn't have next, or finishes with
    /// records unread.
    pub fn run<F: Future>(self, core: F) -> F::Output {
        run_simulated(async {
            let output = tokio::time::timeout(STALLED_AFTER, core)
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "replay diverged: the core is waiting for an input, and the next record is {}",
                        self.describe_next()
                    )
                });
            let unread = self.log.borrow().records.len();
            assert!(
                unread == 0,
                "replay diverged: the core finished with {unread} records unread, starting with {}",
                self.describe_next()
            );
            output
        })
    }

    /// Takes the next record if `pick` accepts it, and wakes every source waiting on the log.
    fn take<T>(&self, pick: impl FnOnce(&InputRecord) -> Option<T>) -> Option<T> {
        let mut log = self.log.borrow_mut();
        let value = pick(log.records.front()?)?;
        log.records.pop_front();
        for waker in log.waiting.drain(..) {
            waker.wake();
        }
        Some(value)
    }

    /// Ready once `pick` accepts the next record.
    fn wait<T: 'static>(
        &self,
        pick: impl Fn(&InputRecord) -> Option<T> + 'static,
    ) -> LocalBoxFuture<'static, T> {
        let replay = self.clone();
        Box::pin(poll_fn(move |cx| match replay.take(&pick) {
            Some(value) => Poll::Ready(value),
            None => {
                replay.log.borrow_mut().waiting.push(cx.waker().clone());
                Poll::Pending
            }
        }))
    }

    /// Takes a record the core reads without waiting (clock, Rng): it must be next.
    fn take_now(&self, source: Source) -> u64 {
        self.take(|r| (r.source == source).then(|| payload_u64(r)))
            .unwrap_or_else(|| {
                panic!(
                    "replay diverged: the core read {source:?}, and the next record is {}",
                    self.describe_next()
                )
            })
    }

    fn describe_next(&self) -> String {
        match self.log.borrow().records.front() {
            Some(r) => format!("seq {} ({:?})", r.seq, r.source),
            None => "none: the log is empty".to_string(),
        }
    }
}

fn payload_u64(record: &InputRecord) -> u64 {
    decode_u64(&record.payload).unwrap_or_else(|| corrupt(record))
}

fn corrupt(record: &InputRecord) -> ! {
    panic!(
        "replay diverged: record seq {} ({:?}) has a payload that doesn't decode",
        record.seq, record.source
    )
}

pub struct ReplayClock {
    replay: Replay,
}

impl Clock for ReplayClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_unix_nanos(self.replay.take_now(Source::Clock))
    }
}

pub struct ReplayRng {
    replay: Replay,
}

impl Rng for ReplayRng {
    fn next_u64(&mut self) -> u64 {
        self.replay.take_now(Source::Rng)
    }
}

pub struct ReplayEvents<E> {
    replay: Replay,
    event: PhantomData<fn() -> E>,
}

// Cancel-safe: a record is taken only in the poll that returns it.
impl<E: Recordable + 'static> EventSource for ReplayEvents<E> {
    type Event = E;

    fn next(&mut self) -> LocalBoxFuture<'_, Option<E>> {
        self.replay.wait(|r| {
            (r.source == Source::Event)
                .then(|| decode_event(&r.payload).unwrap_or_else(|| corrupt(r)))
        })
    }
}

pub struct ReplayRpc<Req, Resp> {
    replay: Replay,
    calls: Cell<u64>,
    types: PhantomData<fn(Req) -> Resp>,
}

impl<Req, Resp: Recordable + 'static> Rpc for ReplayRpc<Req, Resp> {
    type Request = Req;
    type Response = Resp;

    fn call(&self, _request: Req) -> LocalBoxFuture<'static, Resp> {
        let call = self.calls.get();
        self.calls.set(call + 1);
        self.replay.wait(move |r| {
            let ours = r.source == Source::Rpc && response_call(&r.payload) == Some(call);
            ours.then(|| decode_response(&r.payload).unwrap_or_else(|| corrupt(r)))
        })
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;
    use crate::record::encode_u64;

    fn record(seq: u64, source: Source, payload: Vec<u8>) -> InputRecord {
        InputRecord {
            seq,
            source,
            arrived: Timestamp::from_unix_nanos(0),
            payload,
        }
    }

    #[test]
    fn hands_back_recorded_reads() {
        let replay = Replay::new(vec![
            record(0, Source::Clock, encode_u64(7)),
            record(1, Source::Rng, encode_u64(99)),
        ]);
        let clock = replay.clock();
        let mut rng = replay.rng();
        let (now, draw) = replay.run(async move { (clock.now(), rng.next_u64()) });
        assert_eq!((now, draw), (Timestamp::from_unix_nanos(7), 99));
    }

    #[test]
    #[should_panic(
        expected = "replay diverged: the core read Rng, and the next record is seq 0 (Clock)"
    )]
    fn a_read_out_of_order_diverges() {
        let replay = Replay::new(vec![record(0, Source::Clock, encode_u64(7))]);
        let mut rng = replay.rng();
        replay.run(async move { rng.next_u64() });
    }

    #[test]
    #[should_panic(expected = "replay diverged: the core finished with 1 records unread")]
    fn unread_records_diverge() {
        let replay = Replay::new(vec![record(0, Source::Clock, encode_u64(7))]);
        replay.run(async {});
    }

    #[test]
    #[should_panic(expected = "replay diverged: the core is waiting for an input")]
    fn waiting_on_an_input_that_is_not_next_diverges() {
        let replay = Replay::new(vec![record(0, Source::Clock, encode_u64(7))]);
        let mut events = replay.events::<u64>();
        replay.run(async move { events.next().await });
    }

    #[test]
    fn a_cancelled_event_wait_loses_nothing() {
        let replay = Replay::new(vec![
            record(0, Source::Clock, encode_u64(7)),
            record(1, Source::Event, [&[1][..], &encode_u64(42)].concat()),
        ]);
        let clock = replay.clock();
        let mut events = replay.events::<u64>();
        let event = replay.run(async move {
            assert_eq!(events.next().now_or_never(), None);
            clock.now();
            events.next().await
        });
        assert_eq!(event, Some(42));
    }

    #[test]
    fn rpc_responses_match_their_call_whatever_order_they_arrive_in() {
        let response = |seq, call: u64, answer: u64| {
            record(
                seq,
                Source::Rpc,
                [call.to_le_bytes(), answer.to_le_bytes()].concat(),
            )
        };
        let replay = Replay::new(vec![response(0, 1, 11), response(1, 0, 10)]);
        let rpc = replay.rpc::<(), u64>();
        let answers = replay.run(async move {
            let first = rpc.call(());
            let second = rpc.call(());
            futures::join!(first, second)
        });
        assert_eq!(answers, (10, 11));
    }

    impl Recordable for u64 {
        fn encode(&self) -> Vec<u8> {
            encode_u64(*self)
        }

        fn decode(bytes: &[u8]) -> Option<Self> {
            decode_u64(bytes)
        }
    }
}
