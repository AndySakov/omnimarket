//! Where demo accounts live between restarts (#85): Postgres. Only what can't be rebuilt is
//! kept, each account and its fills in order; balances and positions are replayed from them.

use proto::api::v1::TokenRef;
use proto::api::v1::account::Kind;
use tokio_postgres::{Client, NoTls};
use types::chain::Address;

use crate::accounts::{AccountState, Fill};

pub struct Store {
    client: Client,
}

#[derive(Debug)]
pub enum StoreError {
    Postgres(tokio_postgres::Error),
    /// A stored row that doesn't read back.
    Corrupt(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Postgres(e) => write!(f, "postgres: {e}"),
            Self::Corrupt(what) => write!(f, "a stored {what} doesn't read back"),
        }
    }
}

impl From<tokio_postgres::Error> for StoreError {
    fn from(e: tokio_postgres::Error) -> Self {
        Self::Postgres(e)
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS demo_accounts (
    account_id text PRIMARY KEY,
    kind text NOT NULL,
    privy_user text UNIQUE,
    wallet text,
    created_at_ms bigint NOT NULL
);
CREATE TABLE IF NOT EXISTS demo_fills (
    seq bigserial PRIMARY KEY,
    trade_id text NOT NULL UNIQUE,
    account_id text NOT NULL REFERENCES demo_accounts,
    token_in jsonb NOT NULL,
    amount_in text NOT NULL,
    token_out jsonb NOT NULL,
    amount_out text NOT NULL,
    value_usd double precision NOT NULL,
    at_ms bigint NOT NULL,
    receipt jsonb
);
CREATE TABLE IF NOT EXISTS demo_orders (
    order_id text PRIMARY KEY,
    account_id text NOT NULL REFERENCES demo_accounts,
    kind text NOT NULL,
    token_address text NOT NULL,
    level_usd double precision NOT NULL,
    expires_at_ms bigint NOT NULL,
    state text NOT NULL,
    firing_count bigint NOT NULL,
    terms jsonb NOT NULL
);
";

fn kind_name(kind: triggers::Kind) -> &'static str {
    match kind {
        triggers::Kind::LimitBuy => "limit_buy",
        triggers::Kind::TakeProfit => "take_profit",
        triggers::Kind::StopLoss => "stop_loss",
    }
}

fn state_name(state: triggers::State) -> &'static str {
    use triggers::State::*;
    match state {
        Open => "open",
        Firing => "firing",
        Filled => "filled",
        Failed => "failed",
        Cancelled => "cancelled",
        Expired => "expired",
    }
}

/// An account as stored: its state, and the Privy user it belongs to.
pub struct Stored {
    pub account: AccountState,
    pub privy_user: Option<String>,
}

impl Store {
    /// Connects and creates the tables if they're missing.
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
        // The connection does the socket I/O; it ends when the client is dropped.
        #[allow(clippy::disallowed_methods)]
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::warn!(%error, "the postgres connection ended");
            }
        });
        client.batch_execute(SCHEMA).await?;
        Ok(Self { client })
    }

    pub async fn insert_account(
        &self,
        account: &AccountState,
        privy_user: Option<&str>,
    ) -> Result<(), StoreError> {
        let kind = match account.kind {
            Kind::Privy => "privy",
            _ => "guest",
        };
        let wallet = account.wallet.map(|w| crate::model::hex(w.as_slice()));
        self.client
            .execute(
                "INSERT INTO demo_accounts (account_id, kind, privy_user, wallet, created_at_ms)
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT (account_id) DO NOTHING",
                &[
                    &account.account_id,
                    &kind,
                    &privy_user,
                    &wallet,
                    &(account.created_at_ms as i64),
                ],
            )
            .await?;
        Ok(())
    }

    /// Keeps a fill. The same trade twice is kept once.
    pub async fn insert_fill(&self, account_id: &str, fill: &Fill) -> Result<(), StoreError> {
        let token = |t: &TokenRef| serde_json::to_value(t).expect("a TokenRef serializes");
        self.client
            .execute(
                "INSERT INTO demo_fills
                   (trade_id, account_id, token_in, amount_in, token_out, amount_out, value_usd, at_ms, receipt)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (trade_id) DO NOTHING",
                &[
                    &fill.trade_id,
                    &account_id,
                    &token(&fill.token_in),
                    &fill.amount_in.to_string(),
                    &token(&fill.token_out),
                    &fill.amount_out.to_string(),
                    &fill.value_usd,
                    &(fill.at_ms as i64),
                    &fill
                        .receipt
                        .as_ref()
                        .map(|r| serde_json::to_value(r).expect("a receipt serializes")),
                ],
            )
            .await?;
        Ok(())
    }

    /// Keeps an order as it is now: its state and firing count included, so a restart never
    /// fires it twice (D35).
    pub async fn upsert_order(
        &self,
        order: &triggers::Order,
        terms: &crate::Terms,
    ) -> Result<(), StoreError> {
        let terms = serde_json::json!({
            "token": terms.token,
            "level": terms.level,
            "spend_amount": terms.spend_amount,
            "spend_token": terms.spend_token,
            "sell_pct": terms.sell_pct,
            "exit_guarantee": terms.exit_guarantee,
            "slippage_bps": terms.slippage_bps,
            "created_at_ms": terms.created_at_ms,
            "last_firing_id": terms.last_firing_id,
            "detail": terms.detail,
            "updated_at_ms": terms.updated_at_ms,
        });
        self.client
            .execute(
                "INSERT INTO demo_orders
                   (order_id, account_id, kind, token_address, level_usd, expires_at_ms, state, firing_count, terms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (order_id) DO UPDATE SET
                   level_usd = $5, state = $7, firing_count = $8, terms = $9",
                &[
                    &order.order_id,
                    &order.account_id,
                    &kind_name(order.kind),
                    &crate::model::hex(order.token.as_slice()),
                    &order.level_usd,
                    &(order.expires_at_ms as i64),
                    &state_name(order.state),
                    &(order.firing_count as i64),
                    &terms,
                ],
            )
            .await?;
        Ok(())
    }

    /// Every order, with its terms.
    pub async fn load_orders(&self) -> Result<Vec<(triggers::Order, crate::Terms)>, StoreError> {
        let corrupt = |what: &str| StoreError::Corrupt(format!("order's {what}"));
        let mut orders = Vec::new();
        for row in self
            .client
            .query(
                "SELECT order_id, account_id, kind, token_address, level_usd, expires_at_ms, state, firing_count, terms
                 FROM demo_orders",
                &[],
            )
            .await?
        {
            let kind = match row.get::<_, &str>(2) {
                "limit_buy" => triggers::Kind::LimitBuy,
                "take_profit" => triggers::Kind::TakeProfit,
                "stop_loss" => triggers::Kind::StopLoss,
                _ => return Err(corrupt("kind")),
            };
            use triggers::State::*;
            let state = match row.get::<_, &str>(6) {
                "open" => Open,
                "firing" => Firing,
                "filled" => Filled,
                "failed" => Failed,
                "cancelled" => Cancelled,
                "expired" => Expired,
                _ => return Err(corrupt("state")),
            };
            let token = row
                .get::<_, &str>(3)
                .strip_prefix("0x")
                .and_then(|h| h.parse().ok())
                .ok_or_else(|| corrupt("token"))?;
            let t: serde_json::Value = row.get(8);
            let field = |name: &str| t[name].clone();
            let terms = crate::Terms {
                token: serde_json::from_value(field("token")).map_err(|_| corrupt("token"))?,
                level: serde_json::from_value(field("level")).map_err(|_| corrupt("level"))?,
                spend_amount: field("spend_amount").as_str().unwrap_or_default().to_string(),
                spend_token: serde_json::from_value(field("spend_token")).map_err(|_| corrupt("spend token"))?,
                sell_pct: field("sell_pct").as_str().unwrap_or_default().to_string(),
                exit_guarantee: field("exit_guarantee").as_bool().unwrap_or_default(),
                slippage_bps: field("slippage_bps").as_u64().map(|b| b as u32),
                created_at_ms: field("created_at_ms").as_u64().unwrap_or_default(),
                last_firing_id: field("last_firing_id").as_str().unwrap_or_default().to_string(),
                detail: field("detail").as_str().unwrap_or_default().to_string(),
                updated_at_ms: field("updated_at_ms").as_u64().unwrap_or_default(),
            };
            let expires: i64 = row.get(5);
            let count: i64 = row.get(7);
            orders.push((
                triggers::Order {
                    order_id: row.get(0),
                    account_id: row.get(1),
                    token,
                    kind,
                    level_usd: row.get(4),
                    expires_at_ms: expires as u64,
                    state,
                    firing_count: count as u64,
                },
                terms,
            ));
        }
        Ok(orders)
    }

    /// Every account, its fills replayed in the order they were kept.
    pub async fn load(&self) -> Result<Vec<Stored>, StoreError> {
        let mut accounts = Vec::new();
        for row in self
            .client
            .query(
                "SELECT account_id, kind, privy_user, wallet, created_at_ms FROM demo_accounts",
                &[],
            )
            .await?
        {
            let kind: &str = row.get(1);
            let wallet: Option<&str> = row.get(3);
            let wallet = match wallet {
                Some(w) => Some(
                    w.strip_prefix("0x")
                        .and_then(|h| h.parse::<Address>().ok())
                        .ok_or_else(|| StoreError::Corrupt("wallet".into()))?,
                ),
                None => None,
            };
            let created: i64 = row.get(4);
            accounts.push(Stored {
                account: AccountState::new(
                    row.get(0),
                    if kind == "privy" {
                        Kind::Privy
                    } else {
                        Kind::Guest
                    },
                    wallet,
                    created as u64,
                ),
                privy_user: row.get(2),
            });
        }
        let fills = self
            .client
            .query(
                "SELECT account_id, trade_id, token_in, amount_in, token_out, amount_out, value_usd, at_ms, receipt
                 FROM demo_fills ORDER BY seq",
                &[],
            )
            .await?;
        for row in fills {
            let account_id: &str = row.get(0);
            let corrupt = |what: &str| StoreError::Corrupt(format!("fill's {what}"));
            let token = |i: usize| {
                serde_json::from_value::<TokenRef>(row.get(i)).map_err(|_| corrupt("token"))
            };
            let amount = |i: usize| {
                row.get::<_, &str>(i)
                    .parse::<alloy_primitives::U256>()
                    .map_err(|_| corrupt("amount"))
            };
            let at: i64 = row.get(7);
            let fill = Fill {
                trade_id: row.get(1),
                token_in: token(2)?,
                amount_in: amount(3)?,
                token_out: token(4)?,
                amount_out: amount(5)?,
                value_usd: row.get(6),
                at_ms: at as u64,
                receipt: row
                    .get::<_, Option<serde_json::Value>>(8)
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|_| corrupt("receipt"))?,
            };
            let stored = accounts
                .iter_mut()
                .find(|s| s.account.account_id == account_id)
                .ok_or_else(|| corrupt("account"))?;
            stored
                .account
                .apply(fill)
                .map_err(|e| StoreError::Corrupt(format!("fill that no longer applies: {e}")))?;
        }
        Ok(accounts)
    }
}
