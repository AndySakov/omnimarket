//! Trigger orders (#86, a thin M6): limit buys, take-profits and stop-losses on a token's USD
//! display price, checked against a sorted index per token on each canonical block's price
//! (D77). A pure state machine: what it fires is a function of the orders and the prices in
//! order, so the store keeps the orders and their firing counts and rebuilds the index.

use std::collections::{BTreeMap, BTreeSet};

use alloy_primitives::{Address, keccak256};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Buys when the price is at or below the level.
    LimitBuy,
    /// Sells a share of the position when the price is at or above the level.
    TakeProfit,
    /// Sells a share of the position when the price is at or below the level.
    StopLoss,
}

impl Kind {
    /// Whether it fires on a price at or above its level (else at or below).
    fn fires_above(self) -> bool {
        matches!(self, Kind::TakeProfit)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Open,
    /// Fired; waiting on the trade. Further crossings are ignored (D35).
    Firing,
    Filled,
    Failed,
    Cancelled,
    Expired,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub order_id: String,
    pub account_id: String,
    pub token: Address,
    pub kind: Kind,
    /// The USD price it fires at.
    pub level_usd: f64,
    /// Zero: no expiry.
    pub expires_at_ms: u64,
    pub state: State,
    /// Firings so far (D35).
    pub firing_count: u64,
}

/// One activation of an order (D35).
#[derive(Clone, Debug, PartialEq)]
pub struct Firing {
    /// keccak-256 of the order ID and the per-order firing count (u64, little-endian).
    pub firing_id: [u8; 32],
    pub order_id: String,
    /// This firing's count: 1 for the first.
    pub firing_count: u64,
    pub level_usd: f64,
    pub price_usd: f64,
    pub block_number: u64,
    pub block_time_ms: u64,
}

pub fn firing_id(order_id: &str, firing_count: u64) -> [u8; 32] {
    let mut input = order_id.as_bytes().to_vec();
    input.extend(firing_count.to_le_bytes());
    keccak256(input).0
}

/// A positive price as a sortable key: positive `f64`s order like their bit patterns.
fn key(price: f64) -> u64 {
    price.max(0.0).to_bits()
}

/// Open orders by token: those firing at or above a level, and those at or below, each sorted
/// by level.
#[derive(Default)]
struct Index {
    above: BTreeSet<(u64, String)>,
    below: BTreeSet<(u64, String)>,
}

#[derive(Default)]
pub struct Book {
    orders: BTreeMap<String, Order>,
    index: BTreeMap<Address, Index>,
}

impl Book {
    /// Takes an order, open or not: an open one is indexed.
    pub fn insert(&mut self, order: Order) {
        self.unindex(&order.order_id);
        if order.state == State::Open {
            let side = self.index.entry(order.token).or_default();
            let entry = (key(order.level_usd), order.order_id.clone());
            if order.kind.fires_above() {
                side.above.insert(entry);
            } else {
                side.below.insert(entry);
            }
        }
        self.orders.insert(order.order_id.clone(), order);
    }

    fn unindex(&mut self, order_id: &str) {
        if let Some(old) = self.orders.get(order_id)
            && let Some(side) = self.index.get_mut(&old.token)
        {
            let entry = (key(old.level_usd), old.order_id.clone());
            side.above.remove(&entry);
            side.below.remove(&entry);
        }
    }

    pub fn get(&self, order_id: &str) -> Option<&Order> {
        self.orders.get(order_id)
    }

    pub fn orders(&self) -> impl Iterator<Item = &Order> {
        self.orders.values()
    }

    /// Moves an order to `state`, out of the index unless it's open again.
    pub fn set_state(&mut self, order_id: &str, state: State) -> Option<&Order> {
        let mut order = self.orders.get(order_id)?.clone();
        order.state = state;
        self.insert(order);
        self.orders.get(order_id)
    }

    /// Checks `token`'s orders against its display price at a canonical block, and fires
    /// those it crosses, in level order (the nearest first) then order ID. Each goes to
    /// `Firing` with its count raised, so the same order can't fire again until its trade is
    /// done. Orders past their expiry by `block_time_ms` expire instead.
    pub fn on_price(
        &mut self,
        token: Address,
        price_usd: f64,
        block_number: u64,
        block_time_ms: u64,
    ) -> Vec<Firing> {
        let Some(side) = self.index.get(&token) else {
            return Vec::new();
        };
        if !price_usd.is_finite() || price_usd <= 0.0 {
            return Vec::new();
        }
        let price = key(price_usd);
        // At or above the level: levels up to the price, nearest (highest) first.
        let above = side
            .above
            .range(..=(price, String::from('\u{10FFFF}')))
            .rev()
            .map(|(_, id)| id.clone());
        // At or below the level: levels from the price up, nearest (lowest) first.
        let below = side
            .below
            .range((price, String::new())..)
            .map(|(_, id)| id.clone());
        let crossed: Vec<String> = above.chain(below).collect();
        let mut firings = Vec::new();
        for order_id in crossed {
            let order = &self.orders[&order_id];
            if order.expires_at_ms != 0 && order.expires_at_ms <= block_time_ms {
                self.set_state(&order_id, State::Expired);
                continue;
            }
            let mut order = order.clone();
            order.firing_count += 1;
            order.state = State::Firing;
            firings.push(Firing {
                firing_id: firing_id(&order.order_id, order.firing_count),
                order_id: order.order_id.clone(),
                firing_count: order.firing_count,
                level_usd: order.level_usd,
                price_usd,
                block_number,
                block_time_ms,
            });
            self.insert(order);
        }
        firings
    }
}
