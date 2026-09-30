use std::collections::VecDeque;
use std::time::Duration;

use futures::future::LocalBoxFuture;
use tokio::time::Instant;

/// Where a core's events come from. In production, a channel receiver fed by the I/O tasks.
pub trait EventSource {
    type Event;

    /// Waits for the next event; `None` means the source has ended.
    ///
    /// Must be cancel-safe: the core drops this future whenever another `select!` branch
    /// wins, and no event may be lost when it does (D74).
    fn next(&mut self) -> LocalBoxFuture<'_, Option<Self::Event>>;
}

/// Events delivered on a fixed schedule of simulated time. Create it inside `run_simulated`.
pub struct SimEventSource<E> {
    schedule: VecDeque<(Duration, E)>,
    origin: Instant,
}

impl<E> SimEventSource<E> {
    /// `schedule` pairs each event with its delay from now, in delivery order.
    pub fn new(schedule: Vec<(Duration, E)>) -> Self {
        Self {
            schedule: schedule.into(),
            origin: Instant::now(),
        }
    }
}

impl<E> EventSource for SimEventSource<E> {
    type Event = E;

    fn next(&mut self) -> LocalBoxFuture<'_, Option<E>> {
        Box::pin(async move {
            let (delay, _) = self.schedule.front()?;
            tokio::time::sleep_until(self.origin + *delay).await;
            // Pop only once the wait is over, so a cancelled call loses nothing.
            self.schedule.pop_front().map(|(_, event)| event)
        })
    }
}

/// Events from the I/O tasks around a core (D74): the production `EventSource`. Receiving from
/// a tokio channel is cancel-safe, so a dropped `next()` loses nothing. The source ends when
/// every sender is dropped.
pub struct ChannelEventSource<E> {
    receiver: tokio::sync::mpsc::Receiver<E>,
}

impl<E> ChannelEventSource<E> {
    pub fn new(receiver: tokio::sync::mpsc::Receiver<E>) -> Self {
        Self { receiver }
    }
}

impl<E> EventSource for ChannelEventSource<E> {
    type Event = E;

    fn next(&mut self) -> LocalBoxFuture<'_, Option<E>> {
        Box::pin(self.receiver.recv())
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;
    use crate::run_simulated;

    #[test]
    fn delivers_each_event_at_its_time() {
        run_simulated(async {
            let start = Instant::now();
            let mut source = SimEventSource::new(vec![
                (Duration::from_millis(10), 'a'),
                (Duration::from_millis(30), 'b'),
            ]);
            assert_eq!(source.next().await, Some('a'));
            assert_eq!(start.elapsed(), Duration::from_millis(10));
            assert_eq!(source.next().await, Some('b'));
            assert_eq!(start.elapsed(), Duration::from_millis(30));
            assert_eq!(source.next().await, None);
        });
    }

    #[test]
    fn a_channel_source_ends_when_the_senders_go() {
        run_simulated(async {
            let (sender, receiver) = tokio::sync::mpsc::channel(4);
            let mut source = ChannelEventSource::new(receiver);
            assert_eq!(source.next().now_or_never(), None);
            sender.send('a').await.unwrap();
            drop(sender);
            assert_eq!(source.next().await, Some('a'));
            assert_eq!(source.next().await, None);
        });
    }

    #[test]
    fn a_cancelled_wait_loses_nothing() {
        run_simulated(async {
            let mut source = SimEventSource::new(vec![(Duration::from_millis(10), 'a')]);
            assert_eq!(source.next().now_or_never(), None);
            assert_eq!(source.next().await, Some('a'));
        });
    }
}
