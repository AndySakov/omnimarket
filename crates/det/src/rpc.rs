use std::cell::RefCell;
use std::time::Duration;

use futures::future::LocalBoxFuture;

/// A request/response call to something outside the core: a node, a simulator, a signer.
pub trait Rpc {
    type Request;
    type Response;

    /// The future owns what it needs (`'static`), so the core can park it with other
    /// in-flight calls and keep handling events (D74).
    fn call(&self, request: Self::Request) -> LocalBoxFuture<'static, Self::Response>;
}

/// A simulated endpoint. `answer` picks each call's latency and response at the moment the
/// call is made, so a seeded model answers in call order and replays exactly. Call it inside
/// `run_simulated`.
pub struct SimRpc<Req, Resp> {
    answer: RefCell<Answer<Req, Resp>>,
}

type Answer<Req, Resp> = Box<dyn FnMut(Req) -> (Duration, Resp)>;

impl<Req, Resp> SimRpc<Req, Resp> {
    pub fn new(answer: impl FnMut(Req) -> (Duration, Resp) + 'static) -> Self {
        Self {
            answer: RefCell::new(Box::new(answer)),
        }
    }
}

impl<Req, Resp: 'static> Rpc for SimRpc<Req, Resp> {
    type Request = Req;
    type Response = Resp;

    fn call(&self, request: Req) -> LocalBoxFuture<'static, Resp> {
        let (latency, response) = (self.answer.borrow_mut())(request);
        Box::pin(async move {
            tokio::time::sleep(latency).await;
            response
        })
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::Instant;

    use super::*;
    use crate::run_simulated;

    #[test]
    fn answers_after_its_latency() {
        run_simulated(async {
            let start = Instant::now();
            let rpc = SimRpc::new(|n: u32| (Duration::from_millis(50), n * 2));
            assert_eq!(rpc.call(21).await, 42);
            assert_eq!(start.elapsed(), Duration::from_millis(50));
        });
    }
}
