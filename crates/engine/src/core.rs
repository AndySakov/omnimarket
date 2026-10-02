//! The engine's one task (D74): waits on blocks and call answers, steps the state machine,
//! and carries out what each step asks for.

use det::{Clock, EventSource, Rpc};
use futures::future::LocalBoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use types::chain::{Block, CallResult, EthCall};

use crate::outbox::Outbox;
use crate::state::{Effects, EngineConfig, EngineError, Pending, State, Summary};

pub struct Engine {
    clock: Box<dyn Clock>,
    blocks: Box<dyn EventSource<Event = Block>>,
    rpc: Box<dyn Rpc<Request = EthCall, Response = CallResult>>,
    outbox: Box<dyn Outbox>,
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
            state: State::new(config),
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
            mut state,
        } = self;
        let mut in_flight: FuturesUnordered<LocalBoxFuture<'static, (Pending, CallResult)>> =
            FuturesUnordered::new();
        let mut blocks_ended = false;
        loop {
            let mut effects = Effects::default();
            tokio::select! {
                biased;
                Some((pending, result)) = in_flight.next(), if !in_flight.is_empty() => {
                    state.on_answer(pending, result, &mut effects);
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
            for (pending, call) in effects.calls {
                let answer = rpc.call(call);
                in_flight.push(Box::pin(async move { (pending, answer.await) }));
            }
        }
        Ok(state.summary())
    }
}
