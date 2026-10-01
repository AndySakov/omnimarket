//! The per-token tick throttle (D43): at most 20 ticks a second for each token, leading edge.
//! The first tick in a quiet spell goes out at once; ticks that arrive within 50 ms of the last
//! one sent are merged (a tick carries the token's whole price, so the latest replaces the
//! rest) and the merged one goes out when its 50 ms are up, so the last value always arrives.
//! Pure: the caller hands in the time, the latest applied block time in the feed.

use std::collections::BTreeMap;

use proto::api::v1::TokenTick;

/// 1000 ms / 20 ticks.
pub const MIN_GAP_MS: u64 = 50;

#[derive(Default)]
pub struct Throttle {
    /// Keyed by token address.
    slots: BTreeMap<String, Slot>,
}

#[derive(Default)]
struct Slot {
    last_sent_ms: Option<u64>,
    /// The latest tick held back, waiting for the gap to pass.
    pending: Option<TokenTick>,
}

impl Throttle {
    /// Offers a tick at `now_ms`: returns it if it may go out now, otherwise holds it,
    /// replacing any tick held for the same token.
    pub fn offer(&mut self, tick: TokenTick, now_ms: u64) -> Option<TokenTick> {
        let slot = self.slots.entry(tick.token.clone()).or_default();
        if slot.may_send(now_ms) {
            slot.pending = None;
            slot.last_sent_ms = Some(now_ms);
            Some(tick)
        } else {
            slot.pending = Some(tick);
            None
        }
    }

    /// The held ticks whose gap has passed by `now_ms`, in token order.
    pub fn due(&mut self, now_ms: u64) -> Vec<TokenTick> {
        let mut out = Vec::new();
        for slot in self.slots.values_mut() {
            if slot.pending.is_some() && slot.may_send(now_ms) {
                slot.last_sent_ms = Some(now_ms);
                out.extend(slot.pending.take());
            }
        }
        out
    }
}

impl Slot {
    fn may_send(&self, now_ms: u64) -> bool {
        match self.last_sent_ms {
            None => true,
            Some(last) => now_ms >= last.saturating_add(MIN_GAP_MS),
        }
    }
}
