//! Trigger orders over the API (#86): placing, editing and cancelling them, their contract
//! form, and what each firing trades. Firing itself is `triggers::Book`'s.

use std::collections::BTreeMap;

use proto::api::v1::order_level::Level;
use proto::api::v1::order_status::State as WireState;
use proto::api::v1::{
    ExecutionMode, Order, OrderKind, OrderLevel, OrderStatus, PlaceOrderRequest, QuoteRequest,
    TokenRef, UpdateOrderRequest,
};
use proto::lineage::v1::Lineage;
use triggers::{Book, Firing, Kind, State};
use types::LineageId;
use types::chain::Address;

use crate::accounts::{AccountState, QUOTE_ASSETS, USDC};
use crate::model::{decimal, hex};

/// Open orders one account may hold.
pub const MAX_OPEN_ORDERS: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderError {
    Bad(&'static str),
    /// A take-profit or stop-loss needs a position to sell from.
    NoPosition,
    UnknownOrder,
    /// Only an open order can change.
    NotOpen,
    TooMany,
}

impl std::fmt::Display for OrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bad(field) => write!(f, "{field} doesn't parse"),
            Self::NoPosition => write!(f, "there's no open position in this token to sell"),
            Self::UnknownOrder => write!(f, "no such order"),
            Self::NotOpen => write!(f, "only an open order can change"),
            Self::TooMany => write!(f, "at most {MAX_OPEN_ORDERS} open orders per account"),
        }
    }
}

/// What the book doesn't hold: the order as the user placed it.
#[derive(Clone, Debug, PartialEq)]
pub struct Terms {
    pub token: TokenRef,
    pub level: OrderLevel,
    pub spend_amount: String,
    pub spend_token: Option<TokenRef>,
    /// For a take-profit or stop-loss: the share of the position to sell, in percent.
    pub sell_pct: String,
    pub exit_guarantee: bool,
    pub slippage_bps: Option<u32>,
    pub created_at_ms: u64,
    pub last_firing_id: String,
    pub detail: String,
    pub updated_at_ms: u64,
}

#[derive(Default)]
pub struct Orders {
    book: Book,
    terms: BTreeMap<String, Terms>,
}

fn kind_of(kind: OrderKind) -> Option<Kind> {
    match kind {
        OrderKind::LimitBuy => Some(Kind::LimitBuy),
        OrderKind::TakeProfit => Some(Kind::TakeProfit),
        OrderKind::StopLoss => Some(Kind::StopLoss),
        OrderKind::Unspecified => None,
    }
}

fn wire_state(state: State) -> WireState {
    match state {
        State::Open => WireState::Open,
        State::Firing => WireState::Firing,
        State::Filled => WireState::Filled,
        State::Failed => WireState::Failed,
        State::Cancelled => WireState::Cancelled,
        State::Expired => WireState::Expired,
    }
}

pub fn parse_address(text: &str) -> Option<Address> {
    text.strip_prefix("0x")?.parse().ok()
}

/// A level as a USD price: given, or `pct_from_entry` off the position's average entry.
fn resolve(level: &OrderLevel, entry_usd: Option<f64>) -> Result<f64, OrderError> {
    let price = match &level.level {
        Some(Level::PriceUsd(p)) => p.parse::<f64>().map_err(|_| OrderError::Bad("level"))?,
        Some(Level::PctFromEntry(pct)) => {
            let pct = pct.parse::<f64>().map_err(|_| OrderError::Bad("level"))?;
            entry_usd.ok_or(OrderError::NoPosition)? * (1.0 + pct / 100.0)
        }
        None => return Err(OrderError::Bad("level")),
    };
    (price.is_finite() && price > 0.0)
        .then_some(price)
        .ok_or(OrderError::Bad("level"))
}

impl Orders {
    /// Places `account`'s order at `now_ms`, named `order_id`.
    pub fn place(
        &mut self,
        account: &AccountState,
        request: &PlaceOrderRequest,
        order_id: String,
        token: TokenRef,
        spend_token: Option<TokenRef>,
        now_ms: u64,
    ) -> Result<Order, OrderError> {
        let kind = kind_of(request.kind()).ok_or(OrderError::Bad("kind"))?;
        let address = parse_address(&token.address).ok_or(OrderError::Bad("token"))?;
        let open = self
            .book
            .orders()
            .filter(|o| o.account_id == account.account_id && o.state == State::Open)
            .count();
        if open >= MAX_OPEN_ORDERS {
            return Err(OrderError::TooMany);
        }
        let entry = account.entry_usd(address);
        match kind {
            Kind::LimitBuy => {
                let spend = spend_token.as_ref().ok_or(OrderError::Bad("spend_token"))?;
                let spend_address = parse_address(&spend.address);
                if !spend_address.is_some_and(|a| QUOTE_ASSETS.contains(&a))
                    || request
                        .spend_amount
                        .parse::<f64>()
                        .map_or(true, |a| a <= 0.0)
                {
                    return Err(OrderError::Bad("spend_amount"));
                }
            }
            Kind::TakeProfit | Kind::StopLoss => {
                let pct: f64 = request
                    .sell_pct
                    .parse()
                    .map_err(|_| OrderError::Bad("sell_pct"))?;
                if !(pct > 0.0 && pct <= 100.0) {
                    return Err(OrderError::Bad("sell_pct"));
                }
                if account.balance(address).is_zero() {
                    return Err(OrderError::NoPosition);
                }
            }
        }
        let level = request.level.clone().unwrap_or_default();
        let level_usd = resolve(&level, entry)?;
        self.book.insert(triggers::Order {
            order_id: order_id.clone(),
            account_id: account.account_id.clone(),
            token: address,
            kind,
            level_usd,
            expires_at_ms: request.expires_at_ms,
            state: State::Open,
            firing_count: 0,
        });
        self.terms.insert(
            order_id.clone(),
            Terms {
                token,
                level,
                spend_amount: request.spend_amount.clone(),
                spend_token,
                sell_pct: request.sell_pct.clone(),
                // Stops default to getting out (D60); a take-profit or limit buy doesn't need to.
                exit_guarantee: request.exit_guarantee.unwrap_or(kind == Kind::StopLoss),
                slippage_bps: request.slippage_bps,
                created_at_ms: now_ms,
                last_firing_id: String::new(),
                detail: String::new(),
                updated_at_ms: now_ms,
            },
        );
        Ok(self.order(&order_id).expect("just placed"))
    }

    /// Edits an open order of `account`'s: its level, its share, its exit guarantee.
    pub fn update(
        &mut self,
        account: &AccountState,
        request: &UpdateOrderRequest,
        now_ms: u64,
    ) -> Result<Order, OrderError> {
        let mut order = self.owned(&account.account_id, &request.order_id)?.clone();
        if order.state != State::Open {
            return Err(OrderError::NotOpen);
        }
        let terms = self
            .terms
            .get_mut(&order.order_id)
            .expect("every order has terms");
        if let Some(level) = &request.level {
            order.level_usd = resolve(level, account.entry_usd(order.token))?;
            terms.level = level.clone();
        }
        if let Some(pct) = &request.sell_pct {
            pct.parse::<f64>()
                .ok()
                .filter(|p| *p > 0.0 && *p <= 100.0)
                .ok_or(OrderError::Bad("sell_pct"))?;
            terms.sell_pct = pct.clone();
        }
        if let Some(on) = request.exit_guarantee {
            terms.exit_guarantee = on;
        }
        terms.updated_at_ms = now_ms;
        let id = order.order_id.clone();
        self.book.insert(order);
        Ok(self.order(&id).expect("just updated"))
    }

    pub fn cancel(
        &mut self,
        account_id: &str,
        order_id: &str,
        now_ms: u64,
    ) -> Result<Order, OrderError> {
        if self.owned(account_id, order_id)?.state != State::Open {
            return Err(OrderError::NotOpen);
        }
        self.finish(order_id, State::Cancelled, "", now_ms);
        Ok(self.order(order_id).expect("just cancelled"))
    }

    fn owned(&self, account_id: &str, order_id: &str) -> Result<&triggers::Order, OrderError> {
        self.book
            .get(order_id)
            .filter(|o| o.account_id == account_id)
            .ok_or(OrderError::UnknownOrder)
    }

    /// Checks the orders on `token` against its canonical display price.
    pub fn on_price(
        &mut self,
        token: Address,
        price_usd: f64,
        block: u64,
        time_ms: u64,
    ) -> Vec<Firing> {
        let firings = self.book.on_price(token, price_usd, block, time_ms);
        for firing in &firings {
            if let Some(terms) = self.terms.get_mut(&firing.order_id) {
                terms.last_firing_id = hex(&firing.firing_id);
                terms.updated_at_ms = time_ms;
            }
        }
        firings
    }

    /// Settles a firing's order: filled, failed (with why) or back to another state.
    pub fn finish(&mut self, order_id: &str, state: State, detail: &str, now_ms: u64) {
        self.book.set_state(order_id, state);
        if let Some(terms) = self.terms.get_mut(order_id) {
            terms.detail = detail.to_string();
            terms.updated_at_ms = now_ms;
        }
    }

    /// The trade a firing makes for `account`: sell `sell_pct` of the position into USDC, or
    /// spend the limit buy's amount. `None` once there's nothing left to sell.
    pub fn trade_for(&self, firing: &Firing, account: &AccountState) -> Option<QuoteRequest> {
        let order = self.book.get(&firing.order_id)?;
        let terms = self.terms.get(&firing.order_id)?;
        let (token_in, token_out, amount_in) = match order.kind {
            Kind::LimitBuy => {
                let spend = terms.spend_token.as_ref()?;
                (
                    spend.address.clone(),
                    terms.token.address.clone(),
                    terms.spend_amount.clone(),
                )
            }
            Kind::TakeProfit | Kind::StopLoss => {
                let held = account.balance(order.token);
                let pct: f64 = terms.sell_pct.parse().ok()?;
                // Basis points of the position, so the amount stays exact.
                let bps = (pct * 100.0).round() as u64;
                let selling = held * alloy_primitives::U256::from(bps)
                    / alloy_primitives::U256::from(10_000u64);
                if selling.is_zero() {
                    return None;
                }
                (
                    terms.token.address.clone(),
                    hex(USDC.as_slice()),
                    crate::model::scaled_decimal(
                        &selling.to_be_bytes::<32>(),
                        terms.token.decimals,
                    ),
                )
            }
        };
        Some(QuoteRequest {
            // Its first 16 bytes: unique per firing, and within a request ID's 64 characters.
            client_request_id: format!("firing-{}", hex(&firing.firing_id[..16])),
            chain_id: terms.token.chain_id,
            token_in,
            token_out,
            amount_in,
            slippage_bps: terms.slippage_bps,
        })
    }

    pub fn exit_guarantee(&self, order_id: &str) -> bool {
        self.terms.get(order_id).is_some_and(|t| t.exit_guarantee)
    }

    pub fn account_of(&self, order_id: &str) -> Option<String> {
        Some(self.book.get(order_id)?.account_id.clone())
    }

    /// The account's orders, newest first.
    pub fn list(&self, account_id: &str) -> Vec<Order> {
        let mut orders: Vec<Order> = self
            .book
            .orders()
            .filter(|o| o.account_id == account_id)
            .filter_map(|o| self.order(&o.order_id))
            .collect();
        orders.sort_by_key(|o| std::cmp::Reverse(o.created_at_ms));
        orders
    }

    /// Every order with its terms: what the store keeps.
    pub fn stored(&self) -> Vec<(triggers::Order, Terms)> {
        self.book
            .orders()
            .filter_map(|o| Some((o.clone(), self.terms.get(&o.order_id)?.clone())))
            .collect()
    }

    /// An order back from the store. One a restart caught mid-firing fails rather than
    /// guess whether its trade ran.
    pub fn restore(&mut self, mut order: triggers::Order, mut terms: Terms) {
        if order.state == State::Firing {
            order.state = State::Failed;
            terms.detail = "the server restarted while it fired".into();
        }
        self.terms.insert(order.order_id.clone(), terms);
        self.book.insert(order);
    }

    /// One order in the contract's form.
    pub fn order(&self, order_id: &str) -> Option<Order> {
        let o = self.book.get(order_id)?;
        let t = self.terms.get(order_id)?;
        let id = LineageId::from_natural_key("order", o.order_id.as_bytes());
        Some(Order {
            lineage: Some(Lineage {
                id: id.as_bytes().to_vec(),
                caused_by: vec![],
            }),
            order_id: o.order_id.clone(),
            account_id: o.account_id.clone(),
            token: Some(t.token.clone()),
            kind: match o.kind {
                Kind::LimitBuy => OrderKind::LimitBuy,
                Kind::TakeProfit => OrderKind::TakeProfit,
                Kind::StopLoss => OrderKind::StopLoss,
            }
            .into(),
            mode: ExecutionMode::Shadow.into(),
            level: Some(t.level.clone()),
            level_price_usd: decimal(o.level_usd),
            spend_amount: t.spend_amount.clone(),
            spend_token: t.spend_token.clone(),
            sell_pct: t.sell_pct.clone(),
            exit_guarantee: t.exit_guarantee,
            slippage_bps: t.slippage_bps,
            expires_at_ms: o.expires_at_ms,
            created_at_ms: t.created_at_ms,
            status: Some(OrderStatus {
                lineage: None,
                order_id: o.order_id.clone(),
                state: wire_state(o.state).into(),
                firing_count: o.firing_count,
                last_firing_id: t.last_firing_id.clone(),
                detail: t.detail.clone(),
                updated_at_ms: t.updated_at_ms,
            }),
        })
    }
}
