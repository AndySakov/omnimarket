//! Deterministic simulation (D49). For now it holds a toy core, the world that feeds it, and
//! the M0 replay demo (D72): a seeded run records every input the core receives, and
//! replaying that recording reproduces every decision.

use std::time::Duration;

use det::{
    Clock, EventSource, InMemorySink, InputRecord, Recordable, Recorder, RecordingClock,
    RecordingEventSource, RecordingRng, RecordingRpc, RecordingSink, Replay, Rng, Rpc, SeededRng,
    SimClock, SimEventSource, SimRpc,
};
use futures::future::LocalBoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use prost::Message;
use tracing::Instrument;
use types::{LineageId, Timestamp};

/// Where every simulated run starts: 2026-01-01T00:00:00Z.
pub const SIM_START: Timestamp = Timestamp::from_unix_nanos(1_767_225_600_000_000_000);

/// The simulator's input log (D72): one partition, keyed by core instance.
pub const INPUT_TOPIC: &str = "inputs.sim";

/// Events per run, one every `EVENT_EVERY`.
pub const EVENTS: u64 = 1_000;
pub const EVENT_EVERY: Duration = Duration::from_millis(10);

/// A swap on one of four pools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToyEvent {
    pub id: u64,
    pub pool: u8,
    pub price: u32,
}

impl ToyEvent {
    pub fn lineage_id(&self) -> LineageId {
        LineageId::from_natural_key("sim.toy_event", &self.id.to_le_bytes())
    }
}

impl Recordable for ToyEvent {
    fn encode(&self) -> Vec<u8> {
        proto::sim::v1::ToyEvent {
            id: self.id,
            pool: self.pool.into(),
            price: self.price,
        }
        .encode_to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let event = proto::sim::v1::ToyEvent::decode(bytes).ok()?;
        Some(Self {
            id: event.id,
            pool: event.pool.try_into().ok()?,
            price: event.price,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteRequest {
    pub pool: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quote {
    pub price: u32,
}

impl Recordable for Quote {
    fn encode(&self) -> Vec<u8> {
        proto::sim::v1::Quote { price: self.price }.encode_to_vec()
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let quote = proto::sim::v1::Quote::decode(bytes).ok()?;
        Some(Self { price: quote.price })
    }
}

/// What the toy core decided about one quoted swap: its decision record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// From the natural key: the swap's id.
    pub id: LineageId,
    /// The swap, then the quote.
    pub caused_by: [LineageId; 2],
    pub at: Timestamp,
    pub event: u64,
    pub quote: u32,
    pub roll: u8,
    pub fire: bool,
}

impl Decision {
    /// Emits the decision record as a wide event (D53): a span carrying its lineage and every
    /// field, under the run's span. Only reads the decision, so it can't change a replay.
    fn trace(&self) {
        let caused_by = self.caused_by.map(|id| id.to_string()).join(",");
        let _span = tracing::info_span!(
            "toy_core.decision",
            lineage.id = %self.id,
            lineage.caused_by = %caused_by,
            sim.at_unix_nanos = self.at.unix_nanos,
            event_id = self.event,
            quote = self.quote,
            roll = self.roll,
            fire = self.fire,
        )
        .entered();
    }

    pub fn to_proto(&self) -> proto::sim::v1::ToyDecision {
        proto::sim::v1::ToyDecision {
            lineage: Some(proto::lineage::v1::Lineage {
                id: self.id.as_bytes().to_vec(),
                caused_by: self
                    .caused_by
                    .iter()
                    .map(|id| id.as_bytes().to_vec())
                    .collect(),
            }),
            at_unix_nanos: self.at.unix_nanos,
            event_id: self.event,
            quote: self.quote,
            roll: self.roll.into(),
            fire: self.fire,
        }
    }
}

/// A stand-in for a real core. Swaps priced 900 and up (about one in ten) need a quote; the
/// core keeps handling swaps while quotes are in flight, and fires when a quote comes back
/// below the swap's price and a roll passes. It reaches the outside only through `det`.
pub struct ToyCore {
    clock: Box<dyn Clock>,
    rng: Box<dyn Rng>,
    events: Box<dyn EventSource<Event = ToyEvent>>,
    rpc: Box<dyn Rpc<Request = QuoteRequest, Response = Quote>>,
}

impl ToyCore {
    pub fn new(
        clock: Box<dyn Clock>,
        rng: Box<dyn Rng>,
        events: Box<dyn EventSource<Event = ToyEvent>>,
        rpc: Box<dyn Rpc<Request = QuoteRequest, Response = Quote>>,
    ) -> Self {
        Self {
            clock,
            rng,
            events,
            rpc,
        }
    }

    /// Runs until the event source ends and every quote is back.
    pub async fn run(self) -> Vec<Decision> {
        let Self {
            clock,
            mut rng,
            mut events,
            rpc,
        } = self;
        let mut decisions = Vec::new();
        let mut in_flight: FuturesUnordered<LocalBoxFuture<'static, (ToyEvent, Quote)>> =
            FuturesUnordered::new();
        let mut events_ended = false;
        loop {
            tokio::select! {
                biased;
                Some((event, quote)) = in_flight.next(), if !in_flight.is_empty() => {
                    // The quote has no natural key, so its ID comes from the Rng (D71).
                    let quote_id = det::random_lineage_id(rng.as_mut());
                    let roll = (rng.next_u64() % 100) as u8;
                    let decision = Decision {
                        id: LineageId::from_natural_key("sim.toy_decision", &event.id.to_le_bytes()),
                        caused_by: [event.lineage_id(), quote_id],
                        at: clock.now(),
                        event: event.id,
                        quote: quote.price,
                        roll,
                        fire: quote.price < event.price && roll < 90,
                    };
                    decision.trace();
                    decisions.push(decision);
                }
                next = events.next(), if !events_ended => match next {
                    Some(event) if event.price >= 900 => {
                        let quote = rpc.call(QuoteRequest { pool: event.pool });
                        in_flight.push(Box::pin(async move { (event, quote.await) }));
                    }
                    Some(_) => {}
                    None => events_ended = true,
                },
                else => break,
            }
        }
        decisions
    }
}

// The world draws from its own seeded streams, apart from the core's Rng, so a change to
// the core doesn't change what the world does.
const EVENT_STREAM: u64 = 0x9e37_79b9_7f4a_7c15;
const RPC_STREAM: u64 = 0xbf58_476d_1ce4_e5b9;

fn world_events(seed: u64) -> Vec<(Duration, ToyEvent)> {
    let mut rng = SeededRng::from_seed(seed ^ EVENT_STREAM);
    (0..EVENTS)
        .map(|id| {
            let event = ToyEvent {
                id,
                pool: (rng.next_u64() % 4) as u8,
                price: (rng.next_u64() % 1000) as u32,
            };
            (EVENT_EVERY * (id as u32 + 1), event)
        })
        .collect()
}

/// Quotes take 20 to 200ms, like a simulation or a signing call.
fn world_rpc(seed: u64) -> SimRpc<QuoteRequest, Quote> {
    let mut rng = SeededRng::from_seed(seed ^ RPC_STREAM);
    SimRpc::new(move |_: QuoteRequest| {
        let latency = Duration::from_millis(20 + rng.next_u64() % 180);
        let quote = Quote {
            price: (rng.next_u64() % 1000) as u32,
        };
        (latency, quote)
    })
}

/// A digest of the decision records' proto bytes, length-delimited so records can't run
/// together. prost encodes a given message to the same bytes every time.
pub fn digest(decisions: &[Decision]) -> blake3::Hash {
    let mut digest = blake3::Hasher::new();
    for decision in decisions {
        digest.update(&decision.to_proto().encode_length_delimited_to_vec());
    }
    digest.finalize()
}

pub struct Run {
    pub decisions: Vec<Decision>,
    pub digest: blake3::Hash,
    pub recording: Vec<InputRecord>,
}

/// Runs the toy core for one seed on simulated time, recording every input it receives.
pub fn run(seed: u64) -> Run {
    let sink = InMemorySink::default();
    let decisions = run_recorded(seed, Box::new(sink.clone()));
    Run {
        digest: digest(&decisions),
        decisions,
        recording: sink.records(),
    }
}

/// Runs the toy core for one seed, writing every input it receives to `sink`.
pub fn run_recorded(seed: u64, sink: Box<dyn RecordingSink>) -> Vec<Decision> {
    det::run_simulated(async {
        let clock = SimClock::starting_at(SIM_START);
        let recorder = Recorder::new(sink, Box::new(clock.clone()));
        let core = ToyCore::new(
            Box::new(RecordingClock::new(Box::new(clock), recorder.clone())),
            Box::new(RecordingRng::new(
                Box::new(SeededRng::from_seed(seed)),
                recorder.clone(),
            )),
            Box::new(RecordingEventSource::new(
                Box::new(SimEventSource::new(world_events(seed))),
                recorder.clone(),
            )),
            Box::new(RecordingRpc::new(Box::new(world_rpc(seed)), recorder)),
        );
        core.run()
            .instrument(tracing::info_span!("toy_core.run", seed))
            .await
    })
}

/// Replays a recording through the toy core and returns its decisions. Panics with "replay
/// diverged" if the core asks for inputs out of recorded order.
pub fn replay(recording: Vec<InputRecord>) -> Vec<Decision> {
    let replay = Replay::new(recording);
    let core = ToyCore::new(
        Box::new(replay.clock()),
        Box::new(replay.rng()),
        Box::new(replay.events()),
        Box::new(replay.rpc()),
    );
    replay.run(core.run())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_expensive_swap_gets_one_decision() {
        let expensive: Vec<u64> = world_events(3)
            .iter()
            .filter(|(_, e)| e.price >= 900)
            .map(|(_, e)| e.id)
            .collect();
        let mut decided: Vec<u64> = run(3).decisions.iter().map(|d| d.event).collect();
        decided.sort();
        assert!(!expensive.is_empty());
        assert_eq!(decided, expensive);
    }

    // With quotes up to 200ms and swaps every 10ms, a core that waited on each quote would
    // fall behind; this one decides each swap within a quote's latency of its arrival.
    #[test]
    fn swaps_keep_flowing_while_quotes_are_in_flight() {
        let arrived: Vec<Timestamp> = world_events(3)
            .iter()
            .map(|(delay, _)| SIM_START.after(*delay))
            .collect();
        for d in run(3).decisions {
            let waited = d.at.unix_nanos - arrived[d.event as usize].unix_nanos;
            assert!((20_000_000..200_000_000).contains(&waited), "{d:?}");
        }
    }

    #[test]
    fn fires_only_below_the_swap_price() {
        let prices: Vec<u32> = world_events(3).iter().map(|(_, e)| e.price).collect();
        let decisions = run(3).decisions;
        assert!(decisions.iter().any(|d| d.fire));
        for d in decisions {
            assert_eq!(
                d.fire,
                d.quote < prices[d.event as usize] && d.roll < 90,
                "{d:?}"
            );
        }
    }

    #[test]
    fn every_decision_carries_its_lineage() {
        let events: Vec<ToyEvent> = world_events(3).into_iter().map(|(_, e)| e).collect();
        for d in run(3).decisions {
            let event = &events[d.event as usize];
            assert_eq!(
                d.id,
                LineageId::from_natural_key("sim.toy_decision", &d.event.to_le_bytes())
            );
            assert_eq!(d.caused_by[0], event.lineage_id());
            let lineage = d.to_proto().lineage.unwrap();
            assert_eq!(lineage.id, d.id.as_bytes());
            assert_eq!(lineage.caused_by.len(), 2);
        }
    }

    #[test]
    fn toy_payloads_round_trip() {
        let event = ToyEvent {
            id: 7,
            pool: 2,
            price: 950,
        };
        assert_eq!(ToyEvent::decode(&event.encode()), Some(event));
        let too_big_pool = proto::sim::v1::ToyEvent {
            id: 7,
            pool: 256,
            price: 950,
        };
        assert_eq!(ToyEvent::decode(&too_big_pool.encode_to_vec()), None);
        let quote = Quote { price: 12 };
        assert_eq!(Quote::decode(&quote.encode()), Some(quote));
    }
}
