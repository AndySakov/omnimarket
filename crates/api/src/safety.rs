//! Token safety (#89, D29's first layer): which tokens to check, and what their last round trip
//! showed. The worker in `server.rs` runs the checks; this keeps the queue and the reports.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use alloy_primitives::U256;
use execution::safety::{RoundTrip, Verdict};
use proto::api::v1::safety_report::RoundTrip as WireRoundTrip;
use proto::api::v1::{SafetyReport, SafetySummary, SafetyVerdict, TokenRef};
use proto::lineage::v1::Lineage;
use types::LineageId;
use types::chain::Address;

use crate::model::{decimal, hex, scaled_decimal};

/// The most of a revert reason a report carries.
pub const MAX_REASON: usize = 200;

/// A report is good for this long; a watched token is checked again after it.
pub const RECHECK_MS: u64 = 5 * 60_000;
/// A token is watched for this long after its page was last read.
pub const WATCH_MS: u64 = 30 * 60_000;

#[derive(Default)]
pub struct Safety {
    reports: BTreeMap<Address, SafetyReport>,
    /// When each watched token's page was last read.
    watched: BTreeMap<Address, u64>,
    queue: VecDeque<Address>,
    queued: BTreeSet<Address>,
}

impl Safety {
    pub fn report(&self, token: Address) -> Option<&SafetyReport> {
        self.reports.get(&token)
    }

    /// The badge's summary of the token's report.
    pub fn summary(&self, token: Address) -> Option<SafetySummary> {
        let report = self.reports.get(&token)?;
        let trip = report.round_trip.as_ref();
        Some(SafetySummary {
            verdict: report.verdict,
            buy_tax_pct: trip.map(|t| t.buy_tax_pct.clone()).unwrap_or_default(),
            sell_tax_pct: trip.map(|t| t.sell_tax_pct.clone()).unwrap_or_default(),
            checked_at_ms: report.checked_at_ms,
        })
    }

    fn stale(&self, token: Address, now_ms: u64) -> bool {
        self.reports
            .get(&token)
            .is_none_or(|r| r.checked_at_ms + RECHECK_MS <= now_ms)
    }

    /// Queues `token` for a check unless its report is fresh or it's queued already.
    pub fn enqueue(&mut self, token: Address, now_ms: u64) {
        if self.stale(token, now_ms) && self.queued.insert(token) {
            self.queue.push_back(token);
        }
    }

    /// A pool trading it against WETH was just discovered: check it once, if it has no report.
    pub fn discovered(&mut self, token: Address) {
        if !self.reports.contains_key(&token) && self.queued.insert(token) {
            self.queue.push_back(token);
        }
    }

    /// Its page was read: watch it, and check it if its report is stale.
    pub fn watch(&mut self, token: Address, now_ms: u64) {
        self.watched.insert(token, now_ms);
        self.enqueue(token, now_ms);
    }

    /// Queues every watched token whose report has gone stale, and stops watching those whose
    /// page hasn't been read for `WATCH_MS`.
    pub fn sweep(&mut self, now_ms: u64) {
        self.watched.retain(|_, read| *read + WATCH_MS > now_ms);
        let due: Vec<Address> = self.watched.keys().copied().collect();
        for token in due {
            self.enqueue(token, now_ms);
        }
    }

    pub fn next_due(&mut self) -> Option<Address> {
        let token = self.queue.pop_front()?;
        self.queued.remove(&token);
        Some(token)
    }

    pub fn record(&mut self, token: Address, report: SafetyReport) {
        self.reports.insert(token, report);
    }

    /// A check that couldn't finish. The report becomes a warning saying why, so a pass never
    /// stands after a check that couldn't confirm it. A honeypot verdict stays, with the flag
    /// added: a failed check is no evidence it got safer.
    pub fn check_failed(&mut self, token: Address, reason: &'static str, now_ms: u64) {
        let failed = failed(token, reason, now_ms);
        match self.reports.get_mut(&token) {
            Some(report) if report.verdict() == SafetyVerdict::Honeypot => {
                report.flags.retain(|f| f.code != "check_failed");
                report.flags.extend(failed.flags);
            }
            _ => {
                self.reports.insert(token, failed);
            }
        }
    }
}

/// A check that couldn't finish (the probe reverted, its answer didn't decode, the endpoint
/// failed): a warning that says so, replacing an earlier report, so a token never keeps
/// showing a pass its latest check couldn't confirm.
pub fn failed(token: Address, reason: &'static str, now_ms: u64) -> SafetyReport {
    let mut key = token.to_vec();
    key.extend(now_ms.to_le_bytes());
    SafetyReport {
        lineage: Some(Lineage {
            id: LineageId::from_natural_key("safety_report", &key)
                .as_bytes()
                .to_vec(),
            caused_by: vec![],
        }),
        verdict: SafetyVerdict::Warning.into(),
        round_trip: None,
        flags: vec![proto::api::v1::safety_report::Flag {
            code: "check_failed".into(),
            detail: reason.to_string(),
        }],
        checked_block: 0,
        checked_at_ms: now_ms,
    }
}

fn pct(ppm: u64) -> String {
    decimal(ppm as f64 / 10_000.0)
}

/// The contract's report of a round trip through `pool`, spending `spent` (WETH) for a token of
/// `decimals`, checked at `block` and `now_ms`.
pub fn report(
    token: Address,
    trip: &RoundTrip,
    pool: Address,
    spent: TokenRef,
    decimals: u32,
    block: u64,
    now_ms: u64,
) -> SafetyReport {
    let verdict = match trip.verdict() {
        Verdict::Passed => SafetyVerdict::Passed,
        Verdict::Warning => SafetyVerdict::Warning,
        Verdict::Honeypot => SafetyVerdict::Honeypot,
    };
    let exact = |amount: U256, decimals: u32| scaled_decimal(&amount.to_be_bytes::<32>(), decimals);
    let mut key = token.to_vec();
    key.extend(block.to_le_bytes());
    SafetyReport {
        lineage: Some(Lineage {
            id: LineageId::from_natural_key("safety_report", &key)
                .as_bytes()
                .to_vec(),
            caused_by: vec![],
        }),
        verdict: verdict.into(),
        round_trip: Some(WireRoundTrip {
            amount_spent: exact(trip.spent, spent.decimals),
            amount_returned: exact(trip.returned, spent.decimals),
            spent_token: Some(spent),
            token_bought: exact(trip.received_tokens, decimals),
            buy_tax_pct: pct(trip.buy_tax_ppm()),
            sell_tax_pct: if trip.sold {
                pct(trip.sell_tax_ppm())
            } else {
                String::new()
            },
            sell_succeeded: trip.sold,
            // The token's own revert message: evidence to show, cut to a size a panel can.
            revert_reason: trip.reason.chars().take(MAX_REASON).collect(),
            pool: hex(pool.as_slice()),
        }),
        flags: vec![],
        checked_block: block,
        checked_at_ms: now_ms,
    }
}
