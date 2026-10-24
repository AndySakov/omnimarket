//! Shadow trades (#84): `POST /v1/trades` runs a quote's trade in shadow mode, once per intent
//! (build rule 7), and keeps its status and receipt.

use std::collections::{BTreeMap, VecDeque};

use alloy_primitives::B256;
use execution::shadow::{self, Finished};
use proto::api::v1::{Quote, QuoteRequest, TradeReceipt, TradeRequest, TradeStatus};
use types::LineageId;
use types::chain::Address;

use crate::model::hex;
use crate::quotes::Quotes;

/// Finished trades kept for repeats and receipts; the oldest go first.
const KEPT: usize = 10_000;

/// Guest accounts until #85: every trade is a guest's.
pub const GUEST_ACCOUNT: &str = "guest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeError {
    /// The body doesn't parse, or a field in it.
    Bad(&'static str),
    /// No unexpired quote has this ID.
    UnknownQuote,
    /// A wallet's signature: Privy accounts come with #85.
    NoWallets,
    /// The same intent is still running.
    Running,
}

impl std::fmt::Display for TradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bad(field) => write!(f, "{field} doesn't parse"),
            Self::UnknownQuote => write!(f, "no unexpired quote has this ID: quote again"),
            Self::NoWallets => write!(f, "only guest accounts trade until Privy accounts land"),
            Self::Running => write!(f, "this trade is still running"),
        }
    }
}

/// A trade to run, with what it needs from the quotes.
pub struct Run {
    pub trade_id: String,
    pub client_request_id: String,
    pub hash: B256,
    pub quote: Quote,
    pub quote_request: QuoteRequest,
}

pub enum Prepared {
    /// This intent already ran: its last status.
    Done(TradeStatus),
    Run(Box<Run>),
}

#[derive(Default)]
pub struct Trades {
    /// By intent hash; `None` while it runs.
    finished: BTreeMap<B256, Option<Finished>>,
    by_trade_id: BTreeMap<String, B256>,
    /// Finished trades, oldest first.
    order: VecDeque<(B256, String)>,
}

impl Trades {
    /// What `request` needs to run, signed by `signer`, or its status if the intent already
    /// ran. Marks the intent as running.
    pub fn prepare(
        &mut self,
        quotes: &Quotes,
        request: &TradeRequest,
        signer: Address,
    ) -> Result<Prepared, TradeError> {
        if !request.intent_signature.is_empty() {
            return Err(TradeError::NoWallets);
        }
        let (quote_request, quote) = quotes
            .issued(&request.quote_id)
            .ok_or(TradeError::UnknownQuote)?;
        let hash = shadow::intent_hash(quote, signer).ok_or(TradeError::Bad("quote_id"))?;
        match self.finished.get(&hash) {
            Some(Some(done)) => return Ok(Prepared::Done(done.status.clone())),
            Some(None) => return Err(TradeError::Running),
            None => {}
        }
        self.finished.insert(hash, None);
        let trade_id = hex(LineageId::from_natural_key("trade", hash.as_slice()).as_bytes());
        self.by_trade_id.insert(trade_id.clone(), hash);
        Ok(Prepared::Run(Box::new(Run {
            trade_id,
            client_request_id: request.client_request_id.clone(),
            hash,
            quote: quote.clone(),
            quote_request: quote_request.clone(),
        })))
    }

    pub fn finish(&mut self, hash: B256, finished: Finished) {
        self.order
            .push_back((hash, finished.status.trade_id.clone()));
        self.finished.insert(hash, Some(finished));
        while self.order.len() > KEPT {
            let Some((hash, trade_id)) = self.order.pop_front() else {
                break;
            };
            self.finished.remove(&hash);
            self.by_trade_id.remove(&trade_id);
        }
    }

    /// Forgets a trade that never finished, so the same intent can run again.
    pub fn abandon(&mut self, hash: B256, trade_id: &str) {
        if matches!(self.finished.get(&hash), Some(None)) {
            self.finished.remove(&hash);
            self.by_trade_id.remove(trade_id);
        }
    }

    pub fn receipt(&self, trade_id: &str) -> Option<&TradeReceipt> {
        let hash = self.by_trade_id.get(trade_id)?;
        self.finished.get(hash)?.as_ref()?.receipt.as_ref()
    }
}
