//! The engine's one task (D74): waits on blocks and call answers, steps the state machine,
//! and carries out what each step asks for.

use det::{Clock, EventSource, Rpc};
use futures::future::LocalBoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use types::chain::{Block, CallResult, EthCall};
use types::chain::{LogsQuery, LogsResult};

use crate::outbox::Outbox;
use crate::state::{Effects, EngineConfig, EngineError, LogRead, Pending, State, Summary};

pub struct Engine {
    clock: Box<dyn Clock>,
    blocks: Box<dyn EventSource<Event = Block>>,
    rpc: Box<dyn Rpc<Request = EthCall, Response = CallResult>>,
    outbox: Box<dyn Outbox>,
    /// Where trade-history reads go; `None` reads no history.
    logs: Option<Box<dyn Rpc<Request = LogsQuery, Response = LogsResult>>>,
    state: State,
}

impl Engine {
    pub fn new(
        config: EngineConfig,
        clock: Box<dyn Clock>,
        blocks: Box<dyn EventSource<Event = Block>>,
        rpc: Box<dyn Rpc<Request = EthCall, Response = CallResult>>,
        outbox: Box<dyn Outbox>,
    ) -> Self {
        Self {
            clock,
            blocks,
            rpc,
            outbox,
            logs: None,
            state: State::new(config),
        }
    }

    /// Attaches the endpoint the config's trade history (`history_blocks`) is read from.
    pub fn with_logs(self, logs: Box<dyn Rpc<Request = LogsQuery, Response = LogsResult>>) -> Self {
        Self {
            logs: Some(logs),
            ..self
        }
    }

    /// Runs until the block source ends and every call has been answered.
    pub async fn run(self) -> Result<Summary, EngineError> {
        // Split so the pending `blocks.next()` borrows only `blocks` while a step runs.
        let Self {
            clock,
            mut blocks,
            rpc,
            outbox,
            logs,
            mut state,
        } = self;
        let mut in_flight: FuturesUnordered<LocalBoxFuture<'static, (Pending, CallResult)>> =
            FuturesUnordered::new();
        let mut reads: FuturesUnordered<LocalBoxFuture<'static, (LogRead, LogsResult)>> =
            FuturesUnordered::new();
        let mut blocks_ended = false;
        loop {
            let mut effects = Effects::default();
            tokio::select! {
                biased;
                Some((pending, result)) = in_flight.next(), if !in_flight.is_empty() => {
                    state.on_answer(pending, result, &mut effects);
                }
                Some((read, result)) = reads.next(), if !reads.is_empty() => {
                    state.on_log_read(read, result, &mut effects);
                }
                next = blocks.next(), if !blocks_ended => match next {
                    Some(block) => state.apply_block(&block, clock.now(), &mut effects)?,
                    None => blocks_ended = true,
                },
                else => break,
            }
            for update in &effects.updates {
                outbox.publish(update);
            }
            for trade in &effects.trades {
                outbox.publish_trade(trade);
            }
            for price in &effects.prices {
                outbox.publish_price(price);
            }
            if let Some(status) = &effects.status {
                outbox.publish_status(status);
            }
            for trade in &effects.history {
                outbox.publish_history(trade);
            }
            for (pending, call) in effects.calls {
                let answer = rpc.call(call);
                in_flight.push(Box::pin(async move { (pending, answer.await) }));
            }
            // Without a log endpoint the read is never answered, and no more are made. Once the
            // blocks end, no new read starts: the run finishes with what's in flight.
            if let Some(logs) = &logs
                && !blocks_ended
            {
                for (read, query) in effects.log_reads {
                    let answer = logs.call(query);
                    reads.push(Box::pin(async move { (read, answer.await) }));
                }
            }
        }
        Ok(state.summary())
    }
}
