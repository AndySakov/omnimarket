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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TradeError {
    /// The body doesn't parse, or a field in it.
    Bad(&'static str),
    /// No unexpired quote has this ID.
    UnknownQuote,
    /// The same intent is still running.
    Running,
}

impl std::fmt::Display for TradeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bad(field) => write!(f, "{field} doesn't parse"),
            Self::UnknownQuote => write!(f, "no unexpired quote has this ID: quote again"),
            Self::Running => write!(f, "this trade is still running"),
        }
    }
}

/// A trade to run, with what it needs from the quotes.
pub struct Run {
    pub trade_id: String,
    pub client_request_id: String,
    pub account_id: String,
    pub hash: B256,
    pub quote: Quote,
    pub quote_request: QuoteRequest,
}

impl Run {
    fn key(&self) -> (String, B256) {
        (self.account_id.clone(), self.hash)
    }
}

pub enum Prepared {
    /// This intent already ran: its last status.
    Done(TradeStatus),
    Run(Box<Run>),
}

#[derive(Default)]
pub struct Trades {
    /// By account and intent hash; `None` while it runs. Guests' intents share the server's
    /// signer, so the hash alone doesn't tell two accounts' trades apart.
    finished: BTreeMap<(String, B256), Option<Finished>>,
    by_trade_id: BTreeMap<String, (String, B256)>,
    /// Finished trades, oldest first.
    order: VecDeque<((String, B256), String)>,
}

impl Trades {
    /// What `account_id`'s `request` needs to run, signed by `signer`, or its status if that
    /// account's intent already filled. Marks the intent as running.
    pub fn prepare(
        &mut self,
        quotes: &Quotes,
        request: &TradeRequest,
        account_id: &str,
        signer: Address,
    ) -> Result<Prepared, TradeError> {
        let (quote_request, quote) = quotes
            .issued(&request.quote_id)
            .ok_or(TradeError::UnknownQuote)?;
        let hash = shadow::intent_hash(quote, signer).ok_or(TradeError::Bad("quote_id"))?;
        let key = (account_id.to_string(), hash);
        // A trade that filled is final; one that failed (a bad signature, the price moved) can
        // run again.
        match self.finished.get(&key) {
            Some(Some(done)) if done.receipt.is_some() => {
                return Ok(Prepared::Done(done.status.clone()));
            }
            Some(None) => return Err(TradeError::Running),
            _ => {}
        }
        self.finished.insert(key.clone(), None);
        let mut natural = account_id.as_bytes().to_vec();
        natural.extend(hash.as_slice());
        let trade_id = hex(LineageId::from_natural_key("trade", &natural).as_bytes());
        self.by_trade_id.insert(trade_id.clone(), key);
        Ok(Prepared::Run(Box::new(Run {
            trade_id,
            client_request_id: request.client_request_id.clone(),
            account_id: account_id.to_string(),
            hash,
            quote: quote.clone(),
            quote_request: quote_request.clone(),
        })))
    }

    pub fn finish(&mut self, run: &Run, finished: Finished) {
        self.order.push_back((run.key(), run.trade_id.clone()));
        self.finished.insert(run.key(), Some(finished));
        while self.order.len() > KEPT {
            let Some((key, trade_id)) = self.order.pop_front() else {
                break;
            };
            self.finished.remove(&key);
            self.by_trade_id.remove(&trade_id);
        }
    }

    /// Forgets a trade that never finished, so the same intent can run again.
    pub fn abandon(&mut self, run: &Run) {
        if matches!(self.finished.get(&run.key()), Some(None)) {
            self.finished.remove(&run.key());
            self.by_trade_id.remove(&run.trade_id);
        }
    }

    /// A trade's receipt, for its own account only.
    pub fn receipt(&self, trade_id: &str, account_id: &str) -> Option<&TradeReceipt> {
        let key = self.by_trade_id.get(trade_id)?;
        if key.0 != account_id {
            return None;
        }
        self.finished.get(key)?.as_ref()?.receipt.as_ref()
    }
}
