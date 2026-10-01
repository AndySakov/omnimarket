//! A token's display price: the liquidity-weighted mid across its pools (D18), in USD.

use alloy_primitives::Address;

/// One of a token's pools, priced in USD through the pool's quote asset (D19).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoolQuote {
    pub pool: Address,
    pub quote: Address,
    /// The token's price in the quote asset.
    pub price_in_quote: f64,
    pub price_usd: f64,
    /// ±2% depth in USD (D24).
    pub depth_usd: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayPrice {
    pub price_usd: f64,
    /// Summed over every pool, counted or not.
    pub depth_usd: f64,
    /// No pool reached the liquidity floor: the price is the deepest pool's (D18, D20).
    pub thin: bool,
    /// The deepest pool: the token page shows its own price beside the display price (D18).
    pub main: PoolQuote,
}

/// Whether a pool counts toward a display price: at or above the liquidity floor (D18).
pub fn counts(depth_usd: f64, floor_usd: f64) -> bool {
    depth_usd >= floor_usd
}

/// `Σ price·depth / Σ depth` over the pools at or above `floor_usd`; with none, the deepest
/// pool's price, flagged thin. Pools whose price isn't a positive finite number are skipped.
/// Ties for deepest go to the first pool in `pools`.
pub fn display_price(pools: &[PoolQuote], floor_usd: f64) -> Option<DisplayPrice> {
    let usable: Vec<&PoolQuote> = pools
        .iter()
        .filter(|p| p.price_usd.is_finite() && p.price_usd > 0.0)
        .filter(|p| p.depth_usd.is_finite() && p.depth_usd >= 0.0)
        .collect();
    let mut main = *usable.first()?;
    for pool in &usable {
        if pool.depth_usd > main.depth_usd {
            main = pool;
        }
    }
    let depth_usd = usable.iter().map(|p| p.depth_usd).sum();
    let (weighted, counted) = usable
        .iter()
        .filter(|p| counts(p.depth_usd, floor_usd))
        .fold((0.0, 0.0), |(w, d), p| {
            (w + p.price_usd * p.depth_usd, d + p.depth_usd)
        });
    let (price_usd, thin) = if counted > 0.0 {
        (weighted / counted, false)
    } else {
        (main.price_usd, true)
    };
    Some(DisplayPrice {
        price_usd,
        depth_usd,
        thin,
        main: *main,
    })
}
