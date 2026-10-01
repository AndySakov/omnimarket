//! Pricing in the engine (D6, M2): token metadata read through the engine's recorded calls
//! (D82), and each touched token's display price published once per canonical block (D77).
//! The math is in `crates/pricing`; this module decides what to read and when to price.

use std::collections::{BTreeMap, BTreeSet};

use pricing::metadata;
use pricing::{PoolQuote, PricingConfig, QuoteKind, TokenMetadata};
use types::LineageId;
use types::chain::{Address, Block, Bytes, CallResult, EthCall};
use venues::multicall;

use crate::outbox::{PoolUpdate, PriceUpdate, PricedPool};
use crate::state::{Effects, Pending, Stats};
use crate::v2::V2Pools;
use crate::v3::V3Pools;

/// Tokens per metadata call: four view calls each.
const METADATA_BATCH: usize = 50;
/// Tokens whose supply is read again per block, at most: one call.
const SUPPLY_BATCH: usize = 50;

/// A pricing call the engine is waiting on.
pub(crate) enum Call {
    /// name, symbol, decimals and totalSupply of each token, at `block`.
    Metadata { block: u64, tokens: Vec<Address> },
    /// totalSupply of each token.
    Supply { tokens: Vec<Address> },
}

struct Known {
    metadata: TokenMetadata,
    /// The block the total supply was last read at.
    supply_read_at: u64,
}

/// Coverage of the tokens in tracked pools (verification.md, build plan M2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Tokens in any pool that published an update, quote assets included.
    pub tokens_seen: u64,
    /// Tokens other than quote assets with at least one pool against a quote asset (D19).
    pub tokens_quotable: u64,
    /// Tokens with a published display price, the native token included.
    pub tokens_priced: u64,
    /// Of those, priced from a pool below the floor (D18).
    pub tokens_thin: u64,
    /// Tokens whose decimals couldn't be read, so they can't be priced.
    pub tokens_without_decimals: u64,
    /// For tokens without a quote pool: the tokens they trade against, by how many such
    /// tokens trade against each, most first (at most 10).
    pub unquoted_counterparts: Vec<(Address, u64)>,
}

pub(crate) struct Prices {
    chain_id: u64,
    /// The engine's quote assets (`EngineConfig::quote_assets`), shared with trade records.
    quote_assets: Vec<Address>,
    config: PricingConfig,
    native: Option<Address>,
    known: BTreeMap<Address, Known>,
    /// Metadata reads in flight.
    reading: BTreeSet<Address>,
    /// Tokens to read metadata for at the end of this block.
    wanted: BTreeSet<Address>,
    /// Each priceable token's pools against a quote asset.
    token_pools: BTreeMap<Address, BTreeSet<Address>>,
    /// Each pool's tokens, and the latest update it published (the price's lineage).
    pools: BTreeMap<Address, (Address, Address, LineageId)>,
    /// Every token seen and what it trades against, for coverage.
    counterparts: BTreeMap<Address, BTreeSet<Address>>,
    /// Tokens to price at the end of this block.
    dirty: BTreeSet<Address>,
    /// The native token's USD price and the price update that published it.
    native_usd: Option<(f64, LineageId)>,
    priced: BTreeSet<Address>,
    thin: BTreeSet<Address>,
}

impl Prices {
    pub fn new(chain_id: u64, quote_assets: Vec<Address>, config: PricingConfig) -> Self {
        // Without the native token among the quote assets nothing is quoted in it.
        let native = quote_assets
            .contains(&config.native)
            .then_some(config.native);
        Self {
            chain_id,
            quote_assets,
            config,
            native,
            known: BTreeMap::new(),
            reading: BTreeSet::new(),
            wanted: BTreeSet::new(),
            token_pools: BTreeMap::new(),
            pools: BTreeMap::new(),
            counterparts: BTreeMap::new(),
            dirty: BTreeSet::new(),
            native_usd: None,
            priced: BTreeSet::new(),
            thin: BTreeSet::new(),
        }
    }

    /// Notes the pools this step changed: which tokens to price at the block's end, and
    /// which tokens' metadata to read.
    pub fn on_updates(&mut self, updates: &[PoolUpdate], v2: &V2Pools, v3: &V3Pools) {
        for update in updates {
            let Some((token0, token1)) = pool_tokens(update.pool, v2, v3) else {
                continue;
            };
            self.pools.insert(update.pool, (token0, token1, update.id));
            self.counterparts.entry(token0).or_default().insert(token1);
            self.counterparts.entry(token1).or_default().insert(token0);
            if self.config.reference_pools.contains(&update.pool) {
                self.want_metadata(token0);
                self.want_metadata(token1);
                if let Some(native) = self.native {
                    self.dirty.insert(native);
                }
            }
            for (token, other) in [(token0, token1), (token1, token0)] {
                // The native token is priced from the reference pools only, stablecoins are
                // pinned (D19), and a token is priced only against a quote asset.
                if self.quote(token).is_some() || self.quote(other).is_none() {
                    continue;
                }
                self.token_pools
                    .entry(token)
                    .or_default()
                    .insert(update.pool);
                self.want_metadata(token);
                self.want_metadata(other);
                self.dirty.insert(token);
            }
        }
    }

    fn quote(&self, token: Address) -> Option<QuoteKind> {
        self.config.quote(&self.quote_assets, token)
    }

    fn want_metadata(&mut self, token: Address) {
        if !self.known.contains_key(&token) && !self.reading.contains(&token) {
            self.wanted.insert(token);
        }
    }

    /// The block's end: read what's wanted, then price every token touched since the last
    /// block, the native token first.
    pub fn on_block(
        &mut self,
        block: &Block,
        v2: &V2Pools,
        v3: &V3Pools,
        stats: &mut Stats,
        effects: &mut Effects,
    ) {
        self.read_metadata(block.number, stats, effects);
        self.refresh_supply(block.number, stats, effects);

        if let Some(native) = self.native
            && self.dirty.remove(&native)
        {
            let had_price = self.native_usd.is_some();
            if let Some(update) = self.price_native(native, block, v2, v3) {
                self.native_usd = Some((update.price_usd, update.id));
                if !had_price {
                    // Every token quoted in the native token can be priced now.
                    let quoted: Vec<Address> = self
                        .token_pools
                        .iter()
                        .filter(|(_, pools)| pools.iter().any(|p| self.pool_has(*p, native)))
                        .map(|(token, _)| *token)
                        .collect();
                    self.dirty.extend(quoted);
                }
                self.publish(update, effects);
            }
        }
        for token in std::mem::take(&mut self.dirty) {
            if let Some(update) = self.price_token(token, block, v2, v3) {
                self.publish(update, effects);
            }
        }
        stats.tokens_priced = self.priced.len() as u64;
    }

    fn publish(&mut self, update: PriceUpdate, effects: &mut Effects) {
        self.priced.insert(update.token);
        if update.thin {
            self.thin.insert(update.token);
        } else {
            self.thin.remove(&update.token);
        }
        effects.prices.push(update);
    }

    fn pool_has(&self, pool: Address, token: Address) -> bool {
        self.pools
            .get(&pool)
            .is_some_and(|&(t0, t1, _)| t0 == token || t1 == token)
    }

    fn read_metadata(&mut self, block: u64, stats: &mut Stats, effects: &mut Effects) {
        let wanted: Vec<Address> = std::mem::take(&mut self.wanted).into_iter().collect();
        for batch in wanted.chunks(METADATA_BATCH) {
            let calls: Vec<(Address, Vec<u8>)> =
                batch.iter().flat_map(|&t| metadata::calls(t)).collect();
            self.reading.extend(batch.iter().copied());
            stats.metadata_calls += 1;
            effects.calls.push((
                Pending::Prices(Call::Metadata {
                    block,
                    tokens: batch.to_vec(),
                }),
                multicall_at(block, &calls),
            ));
        }
    }

    /// Mints and burns change supply, and the engine doesn't follow transfers, so supply is
    /// read again once it is `supply_refresh_blocks` old: the oldest first, a batch a block.
    fn refresh_supply(&mut self, block: u64, stats: &mut Stats, effects: &mut Effects) {
        let refresh = self.config.supply_refresh_blocks;
        let mut stale: Vec<(u64, Address)> = self
            .known
            .iter()
            .filter(|(_, k)| k.supply_read_at.saturating_add(refresh) <= block)
            .filter(|(token, _)| self.priced.contains(*token))
            .map(|(token, k)| (k.supply_read_at, *token))
            .collect();
        stale.sort();
        stale.truncate(SUPPLY_BATCH);
        if stale.is_empty() {
            return;
        }
        let tokens: Vec<Address> = stale.into_iter().map(|(_, token)| token).collect();
        for token in &tokens {
            // Not picked again while this read is out; a failed read waits a full period.
            if let Some(known) = self.known.get_mut(token) {
                known.supply_read_at = block;
            }
        }
        let calls: Vec<(Address, Vec<u8>)> = tokens
            .iter()
            .map(|&t| metadata::total_supply_call(t))
            .collect();
        stats.supply_calls += 1;
        effects.calls.push((
            Pending::Prices(Call::Supply { tokens }),
            multicall_at(block, &calls),
        ));
    }

    pub fn on_answer(&mut self, call: Call, result: CallResult, stats: &mut Stats) {
        match call {
            Call::Metadata { block, tokens } => {
                for token in &tokens {
                    self.reading.remove(token);
                }
                let Some(answers) = answers(&result, tokens.len() * 4) else {
                    // Forget them; each is wanted again the next time its pool updates.
                    stats.metadata_failures += 1;
                    return;
                };
                for (i, token) in tokens.into_iter().enumerate() {
                    let a = &answers[4 * i..4 * i + 4];
                    let metadata = metadata::decode(&[
                        a[0].as_ref().map(|b| b.as_ref()),
                        a[1].as_ref().map(|b| b.as_ref()),
                        a[2].as_ref().map(|b| b.as_ref()),
                        a[3].as_ref().map(|b| b.as_ref()),
                    ]);
                    self.known.insert(
                        token,
                        Known {
                            metadata,
                            supply_read_at: block,
                        },
                    );
                    self.mark_dirty(token);
                }
            }
            Call::Supply { tokens, .. } => {
                let Some(answers) = answers(&result, tokens.len()) else {
                    stats.metadata_failures += 1;
                    return;
                };
                for (token, answer) in tokens.into_iter().zip(answers) {
                    let supply = answer
                        .as_ref()
                        .and_then(|b| metadata::decode_total_supply(b));
                    if let Some(known) = self.known.get_mut(&token)
                        && supply.is_some()
                        && known.metadata.total_supply != supply
                    {
                        known.metadata.total_supply = supply;
                        self.mark_dirty(token);
                    }
                }
            }
        }
    }

    /// Something a token's price depends on changed: price it, or whatever it quotes.
    fn mark_dirty(&mut self, token: Address) {
        match self.quote(token) {
            // The native token's decimals price it, and every token quoted in it.
            Some(QuoteKind::Native) => {
                self.dirty.insert(token);
            }
            Some(QuoteKind::Stable) => {}
            None => {
                if self.token_pools.contains_key(&token) {
                    self.dirty.insert(token);
                }
            }
        }
        // A quote asset's decimals arriving prices the tokens quoted in it.
        if self.quote(token).is_some() {
            let quoted: Vec<Address> = self
                .token_pools
                .iter()
                .filter(|(_, pools)| pools.iter().any(|p| self.pool_has(*p, token)))
                .map(|(t, _)| *t)
                .collect();
            self.dirty.extend(quoted);
            if let Some(native) = self.native
                && self
                    .config
                    .reference_pools
                    .iter()
                    .any(|p| self.pool_has(*p, token))
            {
                self.dirty.insert(native);
            }
        }
    }

    fn decimals(&self, token: Address) -> Option<u8> {
        self.known.get(&token)?.metadata.decimals
    }

    /// The USD price of a quote asset: the native token's latest, or $1 for a stablecoin.
    fn quote_usd(&self, quote: Address) -> Option<(f64, Option<LineageId>)> {
        match self.quote(quote)? {
            QuoteKind::Native => self.native_usd.map(|(usd, id)| (usd, Some(id))),
            QuoteKind::Stable => Some((1.0, None)),
        }
    }

    /// `token` in `pool`, priced through the pool's other token, which must be a quote asset.
    fn quote_in(
        &self,
        token: Address,
        pool: Address,
        v2: &V2Pools,
        v3: &V3Pools,
    ) -> Option<(PoolQuote, LineageId, Option<LineageId>)> {
        let &(token0, token1, update) = self.pools.get(&pool)?;
        let token_is_token0 = token == token0;
        let quote = if token_is_token0 { token1 } else { token0 };
        let (quote_usd, quote_lineage) = self.quote_usd(quote)?;
        let (decimals0, decimals1) = (self.decimals(token0)?, self.decimals(token1)?);
        let priced = match (v2.pair(&pool), v3.pool(&pool)) {
            (Some(pair), _) => pricing::pool::v2(pair.reserves, decimals0, decimals1),
            (None, Some(v3)) => {
                pricing::pool::v3(v3.price, v3.liquidity, &v3.ticks, decimals0, decimals1)
            }
            (None, None) => None,
        }?;
        let (price_in_quote, depth_in_quote) = priced.for_token(token_is_token0);
        Some((
            PoolQuote {
                pool,
                quote,
                price_in_quote,
                price_usd: price_in_quote * quote_usd,
                depth_usd: depth_in_quote * quote_usd,
            },
            update,
            quote_lineage,
        ))
    }

    fn price_native(
        &self,
        native: Address,
        block: &Block,
        v2: &V2Pools,
        v3: &V3Pools,
    ) -> Option<PriceUpdate> {
        let pools: Vec<Address> = self
            .config
            .reference_pools
            .iter()
            .copied()
            .filter(|p| self.pool_has(*p, native))
            .collect();
        self.price_from(native, &pools, block, v2, v3)
    }

    fn price_token(
        &self,
        token: Address,
        block: &Block,
        v2: &V2Pools,
        v3: &V3Pools,
    ) -> Option<PriceUpdate> {
        let pools: Vec<Address> = self.token_pools.get(&token)?.iter().copied().collect();
        self.price_from(token, &pools, block, v2, v3)
    }

    fn price_from(
        &self,
        token: Address,
        pools: &[Address],
        block: &Block,
        v2: &V2Pools,
        v3: &V3Pools,
    ) -> Option<PriceUpdate> {
        let metadata = self.known.get(&token)?.metadata.clone();
        let mut quotes = Vec::new();
        let mut caused_by = BTreeSet::new();
        for &pool in pools {
            if let Some((quote, update, quote_lineage)) = self.quote_in(token, pool, v2, v3) {
                quotes.push(quote);
                caused_by.insert(update);
                caused_by.extend(quote_lineage);
            }
        }
        let floor = self.config.liquidity_floor_usd as f64;
        let display = pricing::display_price(&quotes, floor)?;
        let (main_quote_usd, _) = self.quote_usd(display.main.quote)?;
        let decimals = metadata.decimals?;
        let fdv_usd = metadata
            .total_supply
            .map(|supply| f64::from(supply) / pricing::pool::pow10(decimals) * display.price_usd);
        Some(PriceUpdate::new(
            self.chain_id,
            block,
            token,
            caused_by.into_iter().collect(),
            metadata,
            display,
            display.price_usd / main_quote_usd,
            fdv_usd,
            quotes
                .iter()
                .map(|q| PricedPool {
                    quote: *q,
                    counted: pricing::display::counts(q.depth_usd, floor),
                })
                .collect(),
        ))
    }

    pub fn coverage(&self) -> Coverage {
        let is_quote = |t: &Address| self.quote(*t).is_some();
        let unquoted: Vec<&Address> = self
            .counterparts
            .keys()
            .filter(|t| !is_quote(t) && !self.token_pools.contains_key(*t))
            .collect();
        let mut counts: BTreeMap<Address, u64> = BTreeMap::new();
        for token in &unquoted {
            for other in &self.counterparts[*token] {
                *counts.entry(*other).or_default() += 1;
            }
        }
        let mut unquoted_counterparts: Vec<(Address, u64)> = counts.into_iter().collect();
        // Most first; ties by address, so the order is deterministic.
        unquoted_counterparts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        unquoted_counterparts.truncate(10);
        Coverage {
            tokens_seen: self.counterparts.len() as u64,
            tokens_quotable: self.token_pools.len() as u64,
            tokens_priced: self.priced.len() as u64,
            tokens_thin: self.thin.len() as u64,
            tokens_without_decimals: self
                .known
                .values()
                .filter(|k| k.metadata.decimals.is_none())
                .count() as u64,
            unquoted_counterparts,
        }
    }
}

fn pool_tokens(pool: Address, v2: &V2Pools, v3: &V3Pools) -> Option<(Address, Address)> {
    if let Some(pair) = v2.pair(&pool) {
        return Some((pair.token0, pair.token1));
    }
    v3.pool(&pool).map(|p| (p.token0, p.token1))
}

fn multicall_at(block: u64, calls: &[(Address, Vec<u8>)]) -> EthCall {
    EthCall {
        to: multicall::ADDRESS,
        data: Bytes::from(multicall::encode(calls)),
        block,
    }
}

/// A multicall's answers, if the call returned and has `expected` of them.
fn answers(result: &CallResult, expected: usize) -> Option<Vec<Option<Bytes>>> {
    let CallResult::Returned(data) = result else {
        return None;
    };
    multicall::decode(data).filter(|a| a.len() == expected)
}
