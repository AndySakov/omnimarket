//! Demo accounts (#85): shadow balances, and positions built from shadow fills (#84), marked to
//! the display price (D18). A pure model: an account's state is a function of its starting
//! balances and its fills in order, so the store keeps only those and rebuilds the rest.

use std::collections::BTreeMap;

use alloy_primitives::{U256, address};
use proto::api::v1::{
    Account, Balance, ExecutionMode, Position, TokenRef, TradeReceipt, account::Kind,
};
use proto::lineage::v1::Lineage;
use types::LineageId;
use types::chain::Address;

use crate::model::{decimal, hex, scaled_decimal};

pub const WETH: Address = address!("4200000000000000000000000000000000000006");
pub const USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
const USDT: Address = address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2");

/// A new account's shadow balances: 1 WETH and 1,000 USDC.
pub fn starting_balances() -> Vec<(Address, U256)> {
    vec![
        (WETH, U256::from(10u64).pow(U256::from(18u64))),
        (USDC, U256::from(1_000_000_000u64)),
    ]
}

/// The quote assets: cash, not positions.
pub const QUOTE_ASSETS: [Address; 3] = [WETH, USDC, USDT];

fn is_quote_asset(token: Address) -> bool {
    QUOTE_ASSETS.contains(&token)
}

/// One shadow fill: what a trade's receipt says moved, with the USD value of the quote-asset
/// side at the time.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    pub trade_id: String,
    pub token_in: TokenRef,
    pub amount_in: U256,
    pub token_out: TokenRef,
    pub amount_out: U256,
    /// What the trade was worth: the quote-asset side at its display price.
    pub value_usd: f64,
    pub at_ms: u64,
    /// The trade's receipt, for the account's history.
    pub receipt: Option<TradeReceipt>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Holding {
    token: TokenRef,
    /// Base units.
    amount: U256,
    cost_basis_usd: f64,
    realized_usd: f64,
    opened_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountState {
    pub account_id: String,
    pub kind: Kind,
    pub wallet: Option<Address>,
    pub created_at_ms: u64,
    balances: BTreeMap<Address, U256>,
    /// Token metadata for each balance, from the fills that touched it.
    tokens: BTreeMap<Address, TokenRef>,
    holdings: BTreeMap<Address, Holding>,
    pub fills: Vec<Fill>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FillError {
    /// The account holds less of the input than the trade spends.
    Insufficient {
        have: String,
        need: String,
    },
    BadToken,
}

impl std::fmt::Display for FillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Insufficient { have, need } => {
                write!(
                    f,
                    "the shadow balance holds {have}, the trade spends {need}"
                )
            }
            Self::BadToken => write!(f, "a token address doesn't parse"),
        }
    }
}

fn address_of(token: &TokenRef) -> Result<Address, FillError> {
    token
        .address
        .strip_prefix("0x")
        .and_then(|h| h.parse().ok())
        .ok_or(FillError::BadToken)
}

fn whole(amount: U256, decimals: u32) -> f64 {
    scaled_decimal(&amount.to_be_bytes::<32>(), decimals)
        .parse()
        .unwrap_or(f64::NAN)
}

fn exact(amount: U256, decimals: u32) -> String {
    scaled_decimal(&amount.to_be_bytes::<32>(), decimals)
}

impl AccountState {
    pub fn new(
        account_id: String,
        kind: Kind,
        wallet: Option<Address>,
        created_at_ms: u64,
    ) -> Self {
        let tokens = [
            (WETH, ("WETH", "Wrapped Ether", 18)),
            (USDC, ("USDC", "USD Coin", 6)),
        ]
        .into_iter()
        .map(|(address, (symbol, name, decimals))| {
            (
                address,
                TokenRef {
                    chain_id: 8453,
                    address: hex(address.as_slice()),
                    symbol: symbol.into(),
                    name: name.into(),
                    decimals,
                },
            )
        })
        .collect();
        Self {
            account_id,
            kind,
            wallet,
            created_at_ms,
            balances: starting_balances().into_iter().collect(),
            tokens,
            holdings: BTreeMap::new(),
            fills: Vec::new(),
        }
    }

    /// The shadow balance of `token`, in base units.
    pub fn balance(&self, token: Address) -> U256 {
        self.balances.get(&token).copied().unwrap_or_default()
    }

    /// Whether the account can spend `amount` of `token`.
    pub fn check(&self, token: &TokenRef, amount: U256) -> Result<(), FillError> {
        let have = self.balance(address_of(token)?);
        if have < amount {
            return Err(FillError::Insufficient {
                have: format!("{} {}", exact(have, token.decimals), token.symbol),
                need: format!("{} {}", exact(amount, token.decimals), token.symbol),
            });
        }
        Ok(())
    }

    /// Applies one fill: the input leaves the balance, the output joins it, and the
    /// position in the non-quote side moves. A buy adds its value to the cost basis; a sell
    /// takes the sold share of the cost basis off and realises the difference.
    pub fn apply(&mut self, fill: Fill) -> Result<(), FillError> {
        let token_in = address_of(&fill.token_in)?;
        let token_out = address_of(&fill.token_out)?;
        self.check(&fill.token_in, fill.amount_in)?;
        *self.balances.entry(token_in).or_default() -= fill.amount_in;
        *self.balances.entry(token_out).or_default() += fill.amount_out;
        self.tokens.insert(token_in, fill.token_in.clone());
        self.tokens.insert(token_out, fill.token_out.clone());
        if !is_quote_asset(token_out) {
            let holding = self.holdings.entry(token_out).or_insert_with(|| Holding {
                token: fill.token_out.clone(),
                opened_at_ms: fill.at_ms,
                ..Holding::default()
            });
            if holding.amount.is_zero() {
                holding.opened_at_ms = fill.at_ms;
            }
            holding.amount += fill.amount_out;
            holding.cost_basis_usd += fill.value_usd;
            holding.updated_at_ms = fill.at_ms;
        }
        if !is_quote_asset(token_in)
            && let Some(holding) = self.holdings.get_mut(&token_in)
        {
            let decimals = holding.token.decimals;
            let share = if holding.amount.is_zero() {
                0.0
            } else {
                (whole(fill.amount_in, decimals) / whole(holding.amount, decimals)).min(1.0)
            };
            let cost = holding.cost_basis_usd * share;
            holding.realized_usd += fill.value_usd - cost;
            holding.cost_basis_usd -= cost;
            holding.amount -= fill.amount_in.min(holding.amount);
            if holding.amount.is_zero() {
                holding.cost_basis_usd = 0.0;
            }
            holding.updated_at_ms = fill.at_ms;
        }
        self.fills.push(fill);
        Ok(())
    }

    /// The account as the contract has it, its balances valued at `usd`.
    pub fn account(&self, usd: impl Fn(Address) -> Option<f64>) -> Account {
        Account {
            lineage: Some(self.lineage("account")),
            account_id: self.account_id.clone(),
            kind: self.kind.into(),
            wallet_address: self.wallet.map(|w| hex(w.as_slice())).unwrap_or_default(),
            mode: ExecutionMode::Shadow.into(),
            balances: self
                .balances
                .iter()
                .filter(|(_, amount)| !amount.is_zero())
                .map(|(token, amount)| {
                    let token_ref = self.tokens.get(token).cloned().unwrap_or_default();
                    Balance {
                        value_usd: usd(*token).map_or_else(String::new, |p| {
                            decimal(whole(*amount, token_ref.decimals) * p)
                        }),
                        amount: exact(*amount, token_ref.decimals),
                        token: Some(token_ref),
                    }
                })
                .collect(),
            created_at_ms: self.created_at_ms,
        }
    }

    /// Every position, open ones first by token, then closed ones, marked at `usd`.
    pub fn positions(&self, usd: impl Fn(Address) -> Option<f64>) -> Vec<Position> {
        let mut positions: Vec<Position> = self
            .holdings
            .iter()
            .map(|(token, h)| {
                let amount = whole(h.amount, h.token.decimals);
                let mark = usd(*token);
                let value = mark.map(|p| amount * p);
                let unrealized = value.map(|v| v - h.cost_basis_usd);
                let mut key = self.account_id.as_bytes().to_vec();
                key.extend(token.as_slice());
                Position {
                    lineage: Some(Lineage {
                        id: LineageId::from_natural_key("position", &key)
                            .as_bytes()
                            .to_vec(),
                        caused_by: vec![],
                    }),
                    account_id: self.account_id.clone(),
                    token: Some(h.token.clone()),
                    mode: ExecutionMode::Shadow.into(),
                    amount: exact(h.amount, h.token.decimals),
                    average_entry_price_usd: if amount > 0.0 {
                        decimal(h.cost_basis_usd / amount)
                    } else {
                        String::new()
                    },
                    cost_basis_usd: decimal(h.cost_basis_usd),
                    mark_price_usd: mark.map_or_else(String::new, decimal),
                    value_usd: value.map_or_else(String::new, decimal),
                    unrealized_pnl_usd: unrealized.map_or_else(String::new, decimal),
                    unrealized_pnl_pct: match unrealized {
                        Some(u) if h.cost_basis_usd > 0.0 => decimal(u / h.cost_basis_usd * 100.0),
                        _ => String::new(),
                    },
                    realized_pnl_usd: decimal(h.realized_usd),
                    opened_at_ms: h.opened_at_ms,
                    updated_at_ms: h.updated_at_ms,
                    closed: h.amount.is_zero(),
                }
            })
            .collect();
        positions.sort_by_key(|p| p.closed);
        positions
    }

    /// The account's finished trades, newest first.
    pub fn history(&self) -> Vec<TradeReceipt> {
        self.fills
            .iter()
            .rev()
            .filter_map(|f| f.receipt.clone())
            .collect()
    }

    fn lineage(&self, kind: &str) -> Lineage {
        Lineage {
            id: LineageId::from_natural_key(kind, self.account_id.as_bytes())
                .as_bytes()
                .to_vec(),
            caused_by: vec![],
        }
    }
}
