//! "Why did this fire?" (#87): the chain of records behind a firing or a shadow trade, from the
//! swap that moved the price to the receipt. Each link follows a lineage ID a record carries
//! (D53, D71): a price update is caused by its pools' latest updates, and a pool update and the
//! swap behind it are both caused by the chain event they share. Nothing is rebuilt from
//! current state: a firing keeps the records it fired on, and the rest are joined by ID.

use std::collections::{BTreeMap, VecDeque};

use execution::Finished;
use proto::api::v1::firing_explanation::{Kind, PoolChange, PriceChange, Simulation, Step};
use proto::api::v1::trade::Side;
use proto::api::v1::trade_step_timing::Outcome;
use proto::api::v1::{Firing, FiringExplanation, Trade, TradeStep};
use proto::lineage::v1::Lineage;
use proto::pool::v1::PoolUpdate;
use proto::price::v1::PriceUpdate;
use types::LineageId;
use types::chain::Address;

use crate::model::{decimal, hex, scaled_decimal};
use crate::quotes::Quotes;
use crate::trades::Decided;

type Id = [u8; 16];

/// The records kept of each kind: enough for a firing's records to still be here when its
/// trade's explanation is asked for, a few minutes on.
const KEPT_PRICES: usize = 20_000;
const KEPT_POOL_UPDATES: usize = 50_000;
const KEPT_SWAPS: usize = 50_000;
const KEPT_FIRINGS: usize = 10_000;

/// A map that keeps its newest `cap` entries.
struct Kept<K, V> {
    map: BTreeMap<K, V>,
    order: VecDeque<K>,
    cap: usize,
}

impl<K: Ord + Clone, V> Kept<K, V> {
    fn new(cap: usize) -> Self {
        Self {
            map: BTreeMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    fn insert(&mut self, key: K, value: V) {
        if self.map.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > self.cap {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.map.remove(&oldest);
        }
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.map.get_mut(key)
    }
}

fn id_of(bytes: &[u8]) -> Option<Id> {
    bytes.try_into().ok()
}

fn lineage_id(lineage: Option<&Lineage>) -> Option<Id> {
    id_of(&lineage?.id)
}

/// A pool update, as far as an explanation needs it: where it was and what caused it.
#[derive(Clone, Debug)]
struct PoolLink {
    pool: Vec<u8>,
    block_number: u64,
    log_index: u64,
    chain_event: Option<Id>,
}

/// A price update and the one before it for the same token.
struct Priced {
    update: PriceUpdate,
    previous: Option<Id>,
}

/// What a firing fired on, kept when it fired.
#[derive(Clone, Debug)]
pub struct Fired {
    pub firing: Firing,
    pub account_id: String,
    price: PriceUpdate,
    previous: Option<PriceUpdate>,
    /// The pool update in the firing's block that moved the price, if the price moved on one.
    moved: Option<(Id, PoolLink)>,
}

pub struct Explanations {
    prices: Kept<Id, Priced>,
    /// Each token's latest price update.
    latest: BTreeMap<Vec<u8>, Id>,
    pools: Kept<Id, PoolLink>,
    /// Swaps by the chain event that caused them.
    swaps: Kept<Id, proto::trade::v1::Trade>,
    firings: Kept<String, Fired>,
}

impl Default for Explanations {
    fn default() -> Self {
        Self {
            prices: Kept::new(KEPT_PRICES),
            latest: BTreeMap::new(),
            pools: Kept::new(KEPT_POOL_UPDATES),
            swaps: Kept::new(KEPT_SWAPS),
            firings: Kept::new(KEPT_FIRINGS),
        }
    }
}

impl Explanations {
    pub fn record_pool_update(&mut self, update: &PoolUpdate) {
        let Some(lineage) = &update.lineage else {
            return;
        };
        let Some(id) = id_of(&lineage.id) else {
            return;
        };
        self.pools.insert(
            id,
            PoolLink {
                pool: update.pool.clone(),
                block_number: update.block_number,
                log_index: update.log_index,
                chain_event: lineage.caused_by.first().and_then(|c| id_of(c)),
            },
        );
    }

    pub fn record_swap(&mut self, trade: &proto::trade::v1::Trade) {
        let event = trade.lineage.as_ref().and_then(|l| l.caused_by.first());
        if let Some(event) = event.and_then(|e| id_of(e)) {
            self.swaps.insert(event, trade.clone());
        }
    }

    pub fn record_price(&mut self, update: &PriceUpdate) {
        let Some(id) = lineage_id(update.lineage.as_ref()) else {
            return;
        };
        let previous = self.latest.insert(update.token.clone(), id);
        self.prices.insert(
            id,
            Priced {
                update: update.clone(),
                previous: previous.filter(|p| *p != id),
            },
        );
    }

    /// Keeps what `firing`, of `account_id`'s order, fired on: `price`, the price update it
    /// crossed on, and the pool update in that block that moved it.
    pub fn record_firing(
        &mut self,
        firing: &triggers::Firing,
        account_id: &str,
        price: &PriceUpdate,
    ) {
        let Some(price_id) = lineage_id(price.lineage.as_ref()) else {
            return;
        };
        let previous = self
            .prices
            .get(&price_id)
            .and_then(|p| p.previous)
            .and_then(|id| self.prices.get(&id))
            .map(|p| p.update.clone());
        let causes = price.lineage.as_ref().map_or(&[][..], |l| &l.caused_by[..]);
        let in_block: Vec<(Id, PoolLink)> = causes
            .iter()
            .filter_map(|c| id_of(c))
            .filter_map(|id| self.pools.get(&id).map(|link| (id, link.clone())))
            .filter(|(_, link)| link.block_number == price.block_number)
            .collect();
        // The main pool's update when it moved in this block, else the first that did.
        let moved = in_block
            .iter()
            .find(|(_, link)| link.pool == price.main_pool)
            .or(in_block.first())
            .cloned();
        let firing_id = hex(&firing.firing_id);
        self.firings.insert(
            firing_id.clone(),
            Fired {
                firing: Firing {
                    lineage: Some(Lineage {
                        id: LineageId::from_natural_key("firing", &firing.firing_id)
                            .as_bytes()
                            .to_vec(),
                        caused_by: vec![price_id.to_vec()],
                    }),
                    firing_id,
                    order_id: firing.order_id.clone(),
                    firing_count: firing.firing_count,
                    level_price_usd: decimal(firing.level_usd),
                    display_price_usd: decimal(firing.price_usd),
                    block_number: firing.block_number,
                    block_time_ms: firing.block_time_ms,
                    trade_id: String::new(),
                },
                account_id: account_id.to_string(),
                price: price.clone(),
                previous,
                moved,
            },
        );
    }

    /// The trade a firing made, once its trade is prepared.
    pub fn traded(&mut self, firing_id: &str, trade_id: &str) {
        if let Some(fired) = self.firings.get_mut(&firing_id.to_string()) {
            fired.firing.trade_id = trade_id.to_string();
        }
    }

    /// A firing, for its own account only.
    pub fn firing(&self, firing_id: &str, account_id: &str) -> Option<&Fired> {
        self.firings
            .get(&firing_id.to_string())
            .filter(|f| f.account_id == account_id)
    }

    fn swap(&self, fired: &Fired) -> Option<&proto::trade::v1::Trade> {
        self.swaps
            .get(fired.moved.as_ref()?.1.chain_event.as_ref()?)
    }
}

/// The explanation of `trade_id`: its firing's records when it came from one (a manual trade
/// starts at the route), then its route decision, simulation and receipt. A firing whose trade
/// hasn't run yet has an empty `trade_id` and stops at the trigger check.
pub fn explain(
    explanations: &Explanations,
    fired: Option<&Fired>,
    trade_id: &str,
    finished: Option<&Finished>,
    decided: Option<&Decided>,
    quotes: &Quotes,
) -> FiringExplanation {
    let mut out = FiringExplanation {
        firing_id: fired
            .map(|f| f.firing.firing_id.clone())
            .unwrap_or_default(),
        trade_id: trade_id.to_string(),
        ..FiringExplanation::default()
    };
    if let Some(fired) = fired {
        let at_ms = fired.price.block_timestamp.saturating_mul(1000);
        if let Some((pool_update, link)) = &fired.moved {
            let swap = explanations.swap(fired);
            let summary = match swap {
                Some(swap) => format!(
                    "Swap in {} at block {}, log {}",
                    hex(&swap.tx_hash),
                    link.block_number,
                    link.log_index
                ),
                None => format!(
                    "Swap at block {}, log {}",
                    link.block_number, link.log_index
                ),
            };
            out.steps.push(step(
                Kind::ChainEvent,
                at_ms,
                link.chain_event.map(|e| e.to_vec()),
                summary,
            ));
            out.moving_trade = swap.map(|swap| api_trade(swap, &fired.price, quotes));
            let pool_price =
                |update: Option<&PriceUpdate>, f: fn(&proto::price::v1::PoolPrice) -> f64| {
                    update
                        .and_then(|u| u.pools.iter().find(|p| p.pool == link.pool))
                        .map_or_else(String::new, |p| decimal(f(p)))
                };
            let change = PoolChange {
                pool: hex(&link.pool),
                price_before_usd: pool_price(fired.previous.as_ref(), |p| p.price_usd),
                price_after_usd: pool_price(Some(&fired.price), |p| p.price_usd),
                depth_before_usd: pool_price(fired.previous.as_ref(), |p| p.depth_usd),
                depth_after_usd: pool_price(Some(&fired.price), |p| p.depth_usd),
            };
            out.steps.push(step(
                Kind::PoolUpdate,
                at_ms,
                Some(pool_update.to_vec()),
                format!(
                    "Pool {} moved from ${} to ${}",
                    change.pool,
                    or_dash(&change.price_before_usd),
                    or_dash(&change.price_after_usd)
                ),
            ));
            out.pool_change = Some(change);
        }
        let change = PriceChange {
            display_price_before_usd: fired
                .previous
                .as_ref()
                .map_or_else(String::new, |p| decimal(p.price_usd)),
            display_price_after_usd: decimal(fired.price.price_usd),
            level_price_usd: fired.firing.level_price_usd.clone(),
        };
        out.steps.push(step(
            Kind::PriceUpdate,
            at_ms,
            fired.price.lineage.as_ref().map(|l| l.id.clone()),
            format!(
                "Display price moved from ${} to ${}, across ${}",
                or_dash(&change.display_price_before_usd),
                change.display_price_after_usd,
                change.level_price_usd
            ),
        ));
        out.price_change = Some(change);
        out.steps.push(step(
            Kind::TriggerCheck,
            fired.firing.block_time_ms,
            fired.firing.lineage.as_ref().map(|l| l.id.clone()),
            format!(
                "Order {} fired, firing {} ({})",
                fired.firing.order_id, fired.firing.firing_count, fired.firing.firing_id
            ),
        ));
        out.firing = Some(fired.firing.clone());
    }
    let steps = finished.map_or(&[][..], |f| &f.status.steps[..]);
    let timing = |kind: TradeStep| steps.iter().find(|s| s.step() == kind);
    if let Some(decided) = decided {
        let alternatives = decided.decision.alternatives.len();
        let hops = decided
            .decision
            .chosen
            .as_ref()
            .map_or(0, |c| c.route.len());
        out.steps.push(step(
            Kind::RouteDecision,
            timing(TradeStep::Quote).map_or(0, |s| s.started_at_ms),
            unhex(&decided.quote_id),
            format!(
                "Route through {hops} pool{}, over {alternatives} other route{}",
                if hops == 1 { "" } else { "s" },
                if alternatives == 1 { "" } else { "s" }
            ),
        ));
        out.route_decision = Some(decided.decision.clone());
    }
    if let Some(finished) = finished {
        let receipt = finished.receipt.as_ref();
        if let Some(simulated) = timing(TradeStep::Simulate) {
            let succeeded = simulated.outcome() == Outcome::Succeeded;
            let simulation = Simulation {
                block_number: receipt.map_or(0, |r| r.block_number),
                succeeded,
                amount_out: receipt.map_or_else(String::new, |r| r.amount_out.clone()),
                revert_reason: if succeeded {
                    String::new()
                } else {
                    simulated.detail.clone()
                },
                in_memory: receipt.is_some_and(|r| r.simulated_in_memory),
            };
            out.steps.push(step(
                Kind::Simulation,
                simulated.finished_at_ms.max(simulated.started_at_ms),
                finished.status.lineage.as_ref().map(|l| l.id.clone()),
                if succeeded {
                    format!(
                        "Simulated at block {}: {} out",
                        simulation.block_number, simulation.amount_out
                    )
                } else {
                    format!("Simulation failed: {}", simulation.revert_reason)
                },
            ));
            out.simulation = Some(simulation);
        }
        if let Some(receipt) = receipt {
            let symbol = |t: &Option<proto::api::v1::TokenRef>| {
                t.as_ref().map_or_else(String::new, |t| t.symbol.clone())
            };
            out.steps.push(step(
                Kind::Receipt,
                receipt.steps.last().map_or(0, |s| s.finished_at_ms),
                receipt.lineage.as_ref().map(|l| l.id.clone()),
                format!(
                    "Shadow fill: {} {} for {} {}",
                    receipt.amount_in,
                    symbol(&receipt.token_in),
                    receipt.amount_out,
                    symbol(&receipt.token_out)
                ),
            ));
            out.receipt = Some(receipt.clone());
        }
    }
    let key = if out.firing_id.is_empty() {
        &out.trade_id
    } else {
        &out.firing_id
    };
    let mut natural = key.as_bytes().to_vec();
    natural.extend(out.trade_id.as_bytes());
    out.lineage = Some(Lineage {
        id: LineageId::from_natural_key("firing_explanation", &natural)
            .as_bytes()
            .to_vec(),
        caused_by: out
            .steps
            .iter()
            .map(|s| s.lineage_id.clone())
            .filter(|id| !id.is_empty())
            .collect(),
    });
    out
}

fn step(kind: Kind, at_ms: u64, lineage_id: Option<Vec<u8>>, summary: String) -> Step {
    Step {
        kind: kind.into(),
        at_ms,
        lineage_id: lineage_id.unwrap_or_default(),
        summary,
    }
}

fn or_dash(value: &str) -> &str {
    if value.is_empty() { "—" } else { value }
}

fn unhex(value: &str) -> Option<Vec<u8>> {
    let digits = value.strip_prefix("0x")?;
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The engine's swap as the contract shows a trade, priced at the block's display price.
fn api_trade(swap: &proto::trade::v1::Trade, price: &PriceUpdate, quotes: &Quotes) -> Trade {
    let address = |bytes: &[u8]| Address::try_from(bytes).unwrap_or_default();
    let token = quotes.token_ref(swap.chain_id, address(&swap.token));
    let quote_token = quotes.token_ref(swap.chain_id, address(&swap.quote));
    let token_amount = scaled_decimal(&swap.token_amount, token.decimals);
    let quote_amount = scaled_decimal(&swap.quote_amount, quote_token.decimals);
    let whole = |amount: &str| amount.parse::<f64>().unwrap_or(0.0);
    let price_quote = if whole(&token_amount) > 0.0 {
        decimal(whole(&quote_amount) / whole(&token_amount))
    } else {
        String::new()
    };
    Trade {
        lineage: swap.lineage.clone(),
        chain_id: swap.chain_id,
        token: Some(token),
        quote_token: Some(quote_token),
        pool: hex(&swap.pool),
        venue: match swap.venue() {
            proto::trade::v1::trade::Venue::UniswapV2 => "uniswap-v2".into(),
            proto::trade::v1::trade::Venue::UniswapV3 => "uniswap-v3".into(),
            proto::trade::v1::trade::Venue::Unspecified => String::new(),
        },
        side: match swap.side() {
            proto::trade::v1::trade::Side::Buy => Side::Buy,
            proto::trade::v1::trade::Side::Sell => Side::Sell,
            proto::trade::v1::trade::Side::Unspecified => Side::Unspecified,
        }
        .into(),
        value_usd: decimal(whole(&token_amount) * price.price_usd),
        token_amount,
        quote_amount,
        price_quote,
        price_usd: decimal(price.price_usd),
        sender: hex(&swap.sender),
        recipient: hex(&swap.recipient),
        tx_hash: hex(&swap.tx_hash),
        block_number: swap.block_number,
        block_hash: hex(&swap.block_hash),
        log_index: swap.log_index,
        block_time_ms: swap.block_timestamp.saturating_mul(1000),
    }
}
