//! The demo's accounts (#85): sessions, guest and Privy accounts, their shadow fills, and the
//! public demo's guards. Kept in memory, and in Postgres when there's a store.

use std::collections::{BTreeMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard};

use proto::api::v1::account::Kind;
use proto::api::v1::{Account, Position, TokenRef, TradeReceipt};
use types::chain::Address;

use crate::accounts::{AccountState, Fill, FillError};
use crate::session::{Privy, PrivyUser, SESSION_MS, Sessions};
use crate::store::{Store, StoreError};

/// Sessions one IP address may open an hour.
pub const SESSIONS_PER_IP_HOUR: usize = 20;
/// Trades one account may make an hour.
pub const TRADES_PER_ACCOUNT_HOUR: usize = 120;
const HOUR_MS: u64 = 3_600_000;

#[derive(Debug)]
pub enum DemoError {
    /// A Privy token came, but no Privy app is configured.
    NoPrivy,
    /// The Privy token didn't verify.
    Privy(crate::PrivyError),
    /// Past a guard's limit.
    Limited(&'static str),
    UnknownAccount,
    Fill(FillError),
    Store(StoreError),
}

impl std::fmt::Display for DemoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPrivy => write!(f, "Privy logins aren't configured on this server"),
            Self::Privy(e) => write!(f, "the Privy token doesn't verify: {e:?}"),
            Self::Limited(what) => write!(f, "too many {what}: try again later"),
            Self::UnknownAccount => write!(f, "no such account"),
            Self::Fill(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

#[derive(Default)]
struct Registry {
    by_id: BTreeMap<String, AccountState>,
    by_privy: BTreeMap<String, String>,
    sessions_by_ip: BTreeMap<IpAddr, VecDeque<u64>>,
    trades_by_account: BTreeMap<String, VecDeque<u64>>,
}

/// Drops the times older than an hour before `now_ms`, and counts one more if under `limit`.
fn take(times: &mut VecDeque<u64>, now_ms: u64, limit: usize) -> bool {
    while times.front().is_some_and(|t| t + HOUR_MS <= now_ms) {
        times.pop_front();
    }
    if times.len() >= limit {
        return false;
    }
    times.push_back(now_ms);
    true
}

pub struct Demo {
    sessions: Sessions,
    privy: Option<Privy>,
    registry: Mutex<Registry>,
    orders: Mutex<crate::Orders>,
    store: Option<Store>,
}

/// A new session: its token, when it expires, and the account.
pub struct Opened {
    pub token: String,
    pub expires_at_ms: u64,
    pub account_id: String,
}

impl Demo {
    pub fn new(sessions: Sessions, privy: Option<Privy>) -> Self {
        Self {
            sessions,
            privy,
            registry: Mutex::default(),
            orders: Mutex::default(),
            store: None,
        }
    }

    /// With accounts kept in `store`, and those it already holds loaded.
    pub async fn with_store(mut self, store: Store) -> Result<Self, StoreError> {
        let stored = store.load().await?;
        {
            let mut registry = self.registry();
            for s in stored {
                if let Some(user) = s.privy_user {
                    registry.by_privy.insert(user, s.account.account_id.clone());
                }
                registry
                    .by_id
                    .insert(s.account.account_id.clone(), s.account);
            }
        }
        let orders = store.load_orders().await?;
        let restored = {
            let mut book = self.orders();
            for (order, terms) in orders {
                book.restore(order, terms);
            }
            book.stored()
        };
        // Orders a restart caught mid-firing were just marked failed: keep that.
        for (order, terms) in &restored {
            if order.state == triggers::State::Failed {
                store.upsert_order(order, terms).await?;
            }
        }
        self.store = Some(store);
        Ok(self)
    }

    pub fn orders(&self) -> MutexGuard<'_, crate::Orders> {
        self.orders
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Keeps an order as it is now, if there's a store.
    pub async fn keep_order(&self, order_id: &str) -> Result<(), DemoError> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let stored = self
            .orders()
            .stored()
            .into_iter()
            .find(|(o, _)| o.order_id == order_id);
        if let Some((order, terms)) = stored {
            store
                .upsert_order(&order, &terms)
                .await
                .map_err(DemoError::Store)?;
        }
        Ok(())
    }

    /// Runs `f` on the account, if it exists.
    pub fn with_account<R>(
        &self,
        account_id: &str,
        f: impl FnOnce(&AccountState) -> R,
    ) -> Option<R> {
        self.registry().by_id.get(account_id).map(f)
    }

    fn registry(&self) -> MutexGuard<'_, Registry> {
        self.registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Opens a session: a Privy user's account (created on their first login, found again on
    /// the next), or a new guest account named `guest_id`.
    pub async fn open(
        &self,
        privy_token: Option<&str>,
        guest_id: String,
        ip: Option<IpAddr>,
        now_ms: u64,
    ) -> Result<Opened, DemoError> {
        let user = match privy_token {
            Some(token) => Some(
                self.privy
                    .as_ref()
                    .ok_or(DemoError::NoPrivy)?
                    .verify(token, now_ms / 1000)
                    .map_err(DemoError::Privy)?,
            ),
            None => None,
        };
        let (account, created) = {
            let mut registry = self.registry();
            if let Some(ip) = ip
                && !take(
                    registry.sessions_by_ip.entry(ip).or_default(),
                    now_ms,
                    SESSIONS_PER_IP_HOUR,
                )
            {
                return Err(DemoError::Limited("sessions from this address"));
            }
            match &user {
                Some(PrivyUser { user_id, wallet }) => match registry.by_privy.get(user_id) {
                    Some(id) => (registry.by_id[id].clone(), false),
                    None => {
                        let id = format!("privy-{}", user_id.trim_start_matches("did:privy:"));
                        let account = AccountState::new(id.clone(), Kind::Privy, *wallet, now_ms);
                        registry.by_privy.insert(user_id.clone(), id.clone());
                        registry.by_id.insert(id, account.clone());
                        (account, true)
                    }
                },
                None => {
                    let account = AccountState::new(guest_id, Kind::Guest, None, now_ms);
                    registry
                        .by_id
                        .insert(account.account_id.clone(), account.clone());
                    (account, true)
                }
            }
        };
        if created && let Some(store) = &self.store {
            store
                .insert_account(&account, user.as_ref().map(|u| u.user_id.as_str()))
                .await
                .map_err(DemoError::Store)?;
        }
        let expires_at_ms = now_ms + SESSION_MS;
        Ok(Opened {
            token: self.sessions.issue(&account.account_id, expires_at_ms),
            expires_at_ms,
            account_id: account.account_id,
        })
    }

    /// The account a bearer token is a session of.
    pub fn account_id(&self, token: &str, now_ms: u64) -> Option<String> {
        let id = self.sessions.verify(token, now_ms)?;
        self.registry().by_id.contains_key(&id).then_some(id)
    }

    /// The account's wallet if it's a Privy account, else `None` (a guest).
    pub fn wallet(&self, account_id: &str) -> Option<Address> {
        self.registry().by_id.get(account_id)?.wallet
    }

    pub fn account(
        &self,
        account_id: &str,
        usd: impl Fn(Address) -> Option<f64>,
    ) -> Option<Account> {
        Some(self.registry().by_id.get(account_id)?.account(usd))
    }

    pub fn positions(
        &self,
        account_id: &str,
        usd: impl Fn(Address) -> Option<f64>,
    ) -> Option<Vec<Position>> {
        Some(self.registry().by_id.get(account_id)?.positions(usd))
    }

    pub fn history(&self, account_id: &str) -> Option<Vec<TradeReceipt>> {
        Some(self.registry().by_id.get(account_id)?.history())
    }

    /// Whether the account may make a trade spending `amount` of `token` now: within its
    /// shadow balance, and under the hourly trade cap, which this counts against.
    pub fn admit(
        &self,
        account_id: &str,
        token: &TokenRef,
        amount: alloy_primitives::U256,
        now_ms: u64,
    ) -> Result<(), DemoError> {
        let mut registry = self.registry();
        registry
            .by_id
            .get(account_id)
            .ok_or(DemoError::UnknownAccount)?
            .check(token, amount)
            .map_err(DemoError::Fill)?;
        if !take(
            registry
                .trades_by_account
                .entry(account_id.to_string())
                .or_default(),
            now_ms,
            TRADES_PER_ACCOUNT_HOUR,
        ) {
            return Err(DemoError::Limited("trades from this account"));
        }
        Ok(())
    }

    /// Books a shadow fill to the account, and keeps it.
    pub async fn fill(&self, account_id: &str, fill: Fill) -> Result<(), DemoError> {
        {
            let mut registry = self.registry();
            let account = registry
                .by_id
                .get_mut(account_id)
                .ok_or(DemoError::UnknownAccount)?;
            if account.fills.iter().any(|f| f.trade_id == fill.trade_id) {
                return Ok(());
            }
            account.apply(fill.clone()).map_err(DemoError::Fill)?;
        }
        if let Some(store) = &self.store {
            store
                .insert_fill(account_id, &fill)
                .await
                .map_err(DemoError::Store)?;
        }
        Ok(())
    }
}
