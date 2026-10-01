//! The discovery read model (#81): the `discovery` topic and `GET /v1/discovery`. New lists the
//! pools created during the session, by creation block; Trending ranks tokens by 5m USD volume,
//! then 5m txns, above a minimum depth (D24). A pure function of the `pool-updates.base`,
//! `trades.base` and `prices.base` records applied, in the fixed order the feed merges them in
//! (`feed::Pending`), timed by block time (build rule 1).
//!
//! Rows are published once per block: once a block is whole, the rows as of it are compared
//! with those last published, and only the changed ones go out. That's the feed's throttle.
//! Snapshots serve the published rows, so a delta is always newer than its snapshot.

use std::collections::{BTreeMap, VecDeque};

use proto::api::v1::{DiscoveryFeed, DiscoveryList, DiscoveryRow, DiscoveryRowRemoved, TokenRef};
use proto::api::v1::{WindowStats, delta};
use proto::lineage::v1::Lineage;
use proto::pool::v1::{PoolUpdate, pool_state};
use proto::price::v1::PriceUpdate;
use proto::trade::v1::{Trade, trade};
use types::LineageId;

use crate::model::{decimal, hex};

/// Base makes a block every 2 seconds, so a block's age is its distance from the head × 2s.
pub const BASE_BLOCK_MS: u64 = 2_000;
const FIVE_MINUTES_MS: u64 = 5 * 60 * 1_000;
const ONE_HOUR_MS: u64 = 60 * 60 * 1_000;

pub struct DiscoveryConfig {
    /// How long a pool created during the session stays in New.
    pub new_pool_window_ms: u64,
    /// The least ±2% depth a token needs to trend (D24).
    pub trending_min_depth_usd: f64,
    /// The most rows each list holds.
    pub max_rows: usize,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            new_pool_window_ms: ONE_HOUR_MS,
            trending_min_depth_usd: 10_000.0,
            max_rows: 100,
        }
    }
}

/// `GET /v1/discovery`'s filters, applied server side.
#[derive(Debug, Default, PartialEq)]
pub struct Filters {
    /// Only this list's rows; both when unset.
    pub list: Option<DiscoveryList>,
    pub min_depth_usd: Option<f64>,
    /// Rows whose pool is older, or whose pool's age isn't known, are left out.
    pub max_age_ms: Option<u64>,
}

/// A pool the engine published, and what's known of it.
#[derive(Default)]
struct Pool {
    venue: String,
    /// Set when the engine discovered it empty: created during the session.
    created_block: Option<u64>,
    /// The token traded in it, once a trade or a price record names it.
    token: Option<String>,
}

#[derive(Default)]
struct Token {
    chain_id: u64,
    reference: TokenRef,
    quote_token: Option<TokenRef>,
    main_pool: String,
    price_usd: Option<f64>,
    market_cap_usd: String,
    market_cap_is_fdv: bool,
    depth_usd: f64,
    thin: bool,
    decimals: Option<u32>,
    /// The newest pool for it created during the session.
    new_pool: Option<String>,
    /// Trades within the last hour: (block time ms, USD volume, buy).
    trades: VecDeque<(u64, f64, bool)>,
    tracked: Totals,
    /// Prices within the last hour, and the newest one before it: (block time ms, USD).
    prices: VecDeque<(u64, f64)>,
    first_price_usd: Option<f64>,
    /// The latest record about it, and its block.
    cause: Vec<u8>,
    block_number: u64,
}

#[derive(Default)]
struct Totals {
    volume_usd: f64,
    buys: u64,
    sells: u64,
}

#[derive(Default)]
pub struct Discovery {
    config: DiscoveryConfig,
    pools: BTreeMap<String, Pool>,
    tokens: BTreeMap<String, Token>,
    head_block: u64,
    head_time_ms: u64,
    /// The rows last published, by token, and the block they're as of.
    published: BTreeMap<String, DiscoveryRow>,
    published_block: u64,
}

impl Discovery {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Applies a `pool-updates.base` record. A pool discovered empty (v2 reserves of zero, or an
    /// uninitialized v3 pool) was created at that block; one first seen with state was created
    /// before the engine started, so it's never new.
    pub fn apply_pool_update(&mut self, update: &PoolUpdate) -> Vec<delta::Payload> {
        let out = self.advance(update.block_number);
        let after = update.after.as_ref().and_then(|s| s.state.as_ref());
        let (venue, empty) = match after {
            Some(pool_state::State::V2(v2)) => {
                ("uniswap-v2", is_zero(&v2.reserve0) && is_zero(&v2.reserve1))
            }
            Some(pool_state::State::V3(v3)) => ("uniswap-v3", !v3.initialized),
            None => ("", false),
        };
        let pool = self.pools.entry(hex(&update.pool)).or_default();
        if pool.venue.is_empty() {
            pool.venue = venue.to_string();
        }
        if update.before.is_none() && empty {
            pool.created_block = Some(update.block_number);
            if let Some(token) = pool.token.clone() {
                let id = lineage_id(&update.lineage);
                self.note_new_pool(&token, &hex(&update.pool), id, update.block_number);
            }
        }
        out
    }

    /// Applies a `trades.base` record: the token's volume and txns, valued at its latest display
    /// price (a trade before the token's first price counts as a txn with no volume).
    pub fn apply_trade(&mut self, trade: &Trade) -> Vec<delta::Payload> {
        let out = self.advance(trade.block_number);
        let time_ms = trade.block_timestamp.saturating_mul(1000);
        self.head_time_ms = self.head_time_ms.max(time_ms);
        let address = hex(&trade.token);
        let pool_address = hex(&trade.pool);
        let pool = self.pools.entry(pool_address.clone()).or_default();
        if pool.venue.is_empty() {
            pool.venue = match trade.venue() {
                trade::Venue::UniswapV2 => "uniswap-v2",
                trade::Venue::UniswapV3 => "uniswap-v3",
                trade::Venue::Unspecified => "",
            }
            .to_string();
        }
        let created = pool.created_block;
        pool.token.get_or_insert_with(|| address.clone());
        let id = lineage_id(&trade.lineage);
        let token = self.token_mut(trade.chain_id, &address);
        let volume_usd = match (token.price_usd, token.decimals) {
            (Some(price), Some(decimals)) => to_f64(&trade.token_amount) / pow10(decimals) * price,
            _ => 0.0,
        };
        let buy = trade.side() == trade::Side::Buy;
        token.trades.push_back((time_ms, volume_usd, buy));
        token.tracked.volume_usd += volume_usd;
        if buy {
            token.tracked.buys += 1;
        } else {
            token.tracked.sells += 1;
        }
        token.cause = id.clone();
        token.block_number = trade.block_number;
        if let Some(block) = created
            && self.newer_than_noted(&address, block)
        {
            self.note_new_pool(&address, &pool_address, id, trade.block_number);
        }
        out
    }

    /// Whether a pool created at `block` is newer than the token's noted new pool, if any. A
    /// trade and a price record then credit the same pool, whichever names it first.
    fn newer_than_noted(&self, token: &str, block: u64) -> bool {
        match self.tokens.get(token).map(|t| self.created_block(t)) {
            Some(Some(noted)) => block > noted,
            _ => true,
        }
    }

    /// Applies a `prices.base` record: the token's price, depth and market cap, and which token
    /// each of its pools trades.
    pub fn apply_price(&mut self, update: &PriceUpdate) -> Vec<delta::Payload> {
        let out = self.advance(update.block_number);
        let time_ms = update.block_timestamp.saturating_mul(1000);
        self.head_time_ms = self.head_time_ms.max(time_ms);
        let address = hex(&update.token);
        let id = lineage_id(&update.lineage);
        let mut newest_created: Option<(u64, String)> = None;
        for p in &update.pools {
            let pool_address = hex(&p.pool);
            let pool = self.pools.entry(pool_address.clone()).or_default();
            pool.token.get_or_insert_with(|| address.clone());
            if let Some(block) = pool.created_block
                && pool.token.as_deref() == Some(address.as_str())
                && newest_created.as_ref().is_none_or(|(b, _)| block > *b)
            {
                newest_created = Some((block, pool_address));
            }
        }
        let quote = TokenRef {
            chain_id: update.chain_id,
            address: hex(&update.quote_token),
            ..self
                .tokens
                .get(&hex(&update.quote_token))
                .map(|t| t.reference.clone())
                .unwrap_or_default()
        };
        let metadata = update.metadata.clone().unwrap_or_default();
        let token = self.token_mut(update.chain_id, &address);
        token.reference.symbol = metadata.symbol.unwrap_or_default();
        token.reference.name = metadata.name.unwrap_or_default();
        token.reference.decimals = metadata.decimals.unwrap_or_default();
        token.decimals = metadata.decimals;
        token.quote_token = Some(quote);
        token.main_pool = hex(&update.main_pool);
        token.price_usd = Some(update.price_usd);
        token.first_price_usd.get_or_insert(update.price_usd);
        token.prices.push_back((time_ms, update.price_usd));
        token.market_cap_usd = update.fdv_usd.map(decimal).unwrap_or_default();
        token.market_cap_is_fdv = update.fdv_usd.is_some();
        token.depth_usd = update.depth_usd;
        token.thin = update.thin;
        token.cause = id.clone();
        token.block_number = update.block_number;
        if let Some((block, pool)) = newest_created {
            self.note_new_pool(&address, &pool, id, block.max(update.block_number));
        }
        out
    }

    /// The published feed, filtered.
    pub fn feed(&self, filters: &Filters) -> DiscoveryFeed {
        let rows: Vec<DiscoveryRow> = self
            .published
            .values()
            .filter(|row| filters.passes(row, self.published_block))
            .cloned()
            .collect();
        let mut key = self.published_block.to_le_bytes().to_vec();
        for row in &rows {
            key.extend(lineage_id(&row.lineage));
        }
        DiscoveryFeed {
            lineage: Some(Lineage {
                id: LineageId::from_natural_key("discovery_feed", &key)
                    .as_bytes()
                    .to_vec(),
                caused_by: rows.iter().map(|r| lineage_id(&r.lineage)).collect(),
            }),
            rows,
            block_number: self.published_block,
        }
    }

    /// The block the published rows are as of.
    pub fn published_block(&self) -> u64 {
        self.published_block
    }

    fn token_mut(&mut self, chain_id: u64, address: &str) -> &mut Token {
        let token = self.tokens.entry(address.to_string()).or_default();
        token.chain_id = chain_id;
        token.reference.chain_id = chain_id;
        token.reference.address = address.to_string();
        token
    }

    /// Notes a pool created during the session for a token already known (a pool's token is
    /// only learnt from a trade or price record, which adds the token first).
    fn note_new_pool(&mut self, token: &str, pool: &str, cause: Vec<u8>, block: u64) {
        if let Some(t) = self.tokens.get_mut(token) {
            t.new_pool = Some(pool.to_string());
            t.cause = cause;
            t.block_number = t.block_number.max(block);
        }
    }

    /// Publishes the rows as of the head when `block` is the first record from a later block.
    fn advance(&mut self, block: u64) -> Vec<delta::Payload> {
        if block <= self.head_block {
            return Vec::new();
        }
        let out = self.complete(self.head_block);
        self.head_block = block;
        out
    }

    /// Publishes the rows as of the head once every block up to `block` is whole, if the head's
    /// rows haven't gone out yet.
    pub fn complete(&mut self, block: u64) -> Vec<delta::Payload> {
        let head = self.head_block;
        if head == 0 || head > block || head == self.published_block {
            return Vec::new();
        }
        self.publish()
    }

    /// Ranks the rows as of the head, and returns the ones that changed since the last publish.
    fn publish(&mut self) -> Vec<delta::Payload> {
        self.prune();
        let rows = self.rows();
        let mut out = Vec::new();
        for (token, row) in &rows {
            if self.published.get(token) != Some(row) {
                out.push(delta::Payload::DiscoveryRow(row.clone()));
            }
        }
        for (token, row) in &self.published {
            if !rows.contains_key(token) {
                let mut key = token.as_bytes().to_vec();
                key.extend(self.head_block.to_le_bytes());
                out.push(delta::Payload::DiscoveryRowRemoved(DiscoveryRowRemoved {
                    lineage: Some(Lineage {
                        id: LineageId::from_natural_key("discovery_row_removed", &key)
                            .as_bytes()
                            .to_vec(),
                        caused_by: vec![lineage_id(&row.lineage)],
                    }),
                    chain_id: row.token.as_ref().map(|t| t.chain_id).unwrap_or_default(),
                    token: token.clone(),
                }));
            }
        }
        self.published = rows;
        self.published_block = self.head_block;
        out
    }

    /// Drops trades and prices older than the longest window.
    fn prune(&mut self) {
        let horizon = self.head_time_ms.saturating_sub(ONE_HOUR_MS);
        for token in self.tokens.values_mut() {
            while token.trades.front().is_some_and(|t| t.0 < horizon) {
                token.trades.pop_front();
            }
            // Keep the newest price before the horizon: the hour's opening price.
            while token.prices.len() > 1 && token.prices[1].0 <= horizon {
                token.prices.pop_front();
            }
        }
    }

    /// Every row as of the head, with its lists and rank: its place in Trending if it trends,
    /// otherwise in New. Ties break by token address, so the order holds while ranks don't move.
    fn rows(&self) -> BTreeMap<String, DiscoveryRow> {
        let mut new: Vec<(u64, &String)> = Vec::new();
        let mut trending: Vec<(f64, u64, &String)> = Vec::new();
        for (address, token) in &self.tokens {
            if let Some(created) = self.created_block(token)
                && self.age_ms(created) <= self.config.new_pool_window_ms
            {
                new.push((created, address));
            }
            let (volume, buys, sells) = token.window(self.head_time_ms, FIVE_MINUTES_MS);
            if buys + sells > 0
                && token.price_usd.is_some()
                && token.depth_usd >= self.config.trending_min_depth_usd
            {
                trending.push((volume, buys + sells, address));
            }
        }
        new.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        trending.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(b.2)));
        let mut rows = BTreeMap::new();
        for (rank, (_, address)) in new.iter().take(self.config.max_rows).enumerate() {
            let row = self.row(address, &self.tokens[*address]);
            rows.entry((*address).clone()).or_insert(row).rank = rank as u32 + 1;
        }
        for (rank, (_, _, address)) in trending.iter().take(self.config.max_rows).enumerate() {
            let row = rows
                .entry((*address).clone())
                .or_insert_with(|| self.row(address, &self.tokens[*address]));
            row.rank = rank as u32 + 1;
        }
        for (address, row) in rows.iter_mut() {
            if new
                .iter()
                .take(self.config.max_rows)
                .any(|(_, a)| *a == address)
            {
                row.lists.push(DiscoveryList::New.into());
            }
            if trending
                .iter()
                .take(self.config.max_rows)
                .any(|(_, _, a)| *a == address)
            {
                row.lists.push(DiscoveryList::Trending.into());
            }
        }
        rows
    }

    fn created_block(&self, token: &Token) -> Option<u64> {
        token
            .new_pool
            .as_ref()
            .and_then(|pool| self.pools.get(pool))
            .and_then(|pool| pool.created_block)
    }

    fn age_ms(&self, created_block: u64) -> u64 {
        self.head_block
            .saturating_sub(created_block)
            .saturating_mul(BASE_BLOCK_MS)
    }

    fn row(&self, address: &str, token: &Token) -> DiscoveryRow {
        let created = self.created_block(token);
        let pool = match (&token.new_pool, created) {
            (Some(pool), Some(_)) => pool.clone(),
            _ => token.main_pool.clone(),
        };
        let mut key = address.as_bytes().to_vec();
        key.extend(&token.cause);
        DiscoveryRow {
            lineage: Some(Lineage {
                id: LineageId::from_natural_key("discovery_row", &key)
                    .as_bytes()
                    .to_vec(),
                caused_by: vec![token.cause.clone()],
            }),
            token: Some(token.reference.clone()),
            venue: self
                .pools
                .get(&pool)
                .map(|p| p.venue.clone())
                .unwrap_or_default(),
            pool,
            quote_token: token.quote_token.clone(),
            pool_created_block: created.unwrap_or_default(),
            pool_created_at_ms: created
                .map(|block| self.head_time_ms.saturating_sub(self.age_ms(block)))
                .unwrap_or_default(),
            display_price_usd: token.price_usd.map(decimal).unwrap_or_default(),
            market_cap_usd: token.market_cap_usd.clone(),
            market_cap_is_fdv: token.market_cap_is_fdv,
            depth_usd: token
                .price_usd
                .map(|_| decimal(token.depth_usd))
                .unwrap_or_default(),
            stats_5m: Some(token.stats(self.head_time_ms, FIVE_MINUTES_MS)),
            stats_1h: Some(token.stats(self.head_time_ms, ONE_HOUR_MS)),
            stats_tracked: Some(WindowStats {
                volume_usd: decimal(token.tracked.volume_usd),
                buys: token.tracked.buys,
                sells: token.tracked.sells,
                price_change_pct: change_pct(token.first_price_usd, token.price_usd),
            }),
            thin: token.thin,
            // Filled once safety checks run (#89).
            safety: None,
            rank: 0,
            lists: Vec::new(),
            block_number: token.block_number,
        }
    }
}

impl Token {
    /// USD volume, buys and sells in the window ending at `now_ms`.
    fn window(&self, now_ms: u64, window_ms: u64) -> (f64, u64, u64) {
        let start = now_ms.saturating_sub(window_ms);
        let mut out = (0.0, 0, 0);
        for &(_, volume, buy) in self.trades.iter().filter(|t| t.0 >= start) {
            out.0 += volume;
            if buy {
                out.1 += 1;
            } else {
                out.2 += 1;
            }
        }
        out
    }

    fn stats(&self, now_ms: u64, window_ms: u64) -> WindowStats {
        let (volume, buys, sells) = self.window(now_ms, window_ms);
        let start = now_ms.saturating_sub(window_ms);
        // The price when the window opened: the newest at or before its start, else the oldest.
        let opening = self
            .prices
            .iter()
            .rev()
            .find(|p| p.0 <= start)
            .or(self.prices.front())
            .map(|p| p.1);
        WindowStats {
            volume_usd: decimal(volume),
            buys,
            sells,
            price_change_pct: change_pct(opening, self.price_usd),
        }
    }
}

impl Filters {
    fn passes(&self, row: &DiscoveryRow, head_block: u64) -> bool {
        if let Some(list) = self.list
            && !row.lists.contains(&list.into())
        {
            return false;
        }
        if let Some(min) = self.min_depth_usd
            && row.depth_usd.parse::<f64>().unwrap_or(0.0) < min
        {
            return false;
        }
        if let Some(max) = self.max_age_ms {
            if row.pool_created_block == 0 {
                return false;
            }
            let age = head_block
                .saturating_sub(row.pool_created_block)
                .saturating_mul(BASE_BLOCK_MS);
            if age > max {
                return false;
            }
        }
        true
    }
}

fn change_pct(from: Option<f64>, to: Option<f64>) -> String {
    match (from, to) {
        (Some(from), Some(to)) if from > 0.0 => decimal((to / from - 1.0) * 100.0),
        _ => String::new(),
    }
}

fn lineage_id(lineage: &Option<Lineage>) -> Vec<u8> {
    lineage.as_ref().map(|l| l.id.clone()).unwrap_or_default()
}

fn is_zero(big_endian: &[u8]) -> bool {
    big_endian.iter().all(|&b| b == 0)
}

/// A big-endian integer as the nearest f64, by correctly rounded steps only (D100).
fn to_f64(big_endian: &[u8]) -> f64 {
    big_endian
        .iter()
        .fold(0.0, |acc, &b| acc * 256.0 + f64::from(b))
}

/// 10^n by repeated multiplication, which gives the same bits on every platform.
fn pow10(n: u32) -> f64 {
    (0..n).fold(1.0, |acc, _| acc * 10.0)
}
