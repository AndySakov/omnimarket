//! One pool's mid price and ±2% depth (D24), from the state the engine holds.
//!
//! Exact integer state goes in; the arithmetic is `f64`, using only operations IEEE 754
//! rounds exactly (+, −, ×, ÷, `sqrt`), so the same state gives the same bits on every
//! machine and a replay reprices identically (D100). Fees are left out: depth is the amount
//! that moves the price 2%, not what a trader receives.

use std::collections::BTreeMap;

use alloy_primitives::U256;
use venues::v2::Reserves;
use venues::v3::{Price, TickLiquidity};

use crate::tick_math::sqrt_ratio_at_tick;

/// A pool's price and depth in whole tokens (already divided by each token's decimals).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoolPrice {
    /// token1 per token0.
    pub price: f64,
    /// token1 that moves the price up 2%, plus the token0 that moves it down 2% valued at
    /// `price`: the pool's ±2% depth, in token1.
    pub depth_in_token1: f64,
}

impl PoolPrice {
    /// The same pool seen from one of its tokens: that token's price in the other, and the
    /// depth in the other.
    pub fn for_token(&self, token_is_token0: bool) -> (f64, f64) {
        if token_is_token0 {
            (self.price, self.depth_in_token1)
        } else {
            (1.0 / self.price, self.depth_in_token1 / self.price)
        }
    }
}

/// `10^n` by repeated multiplication: exact to 10^22, and the same bits everywhere above.
pub fn pow10(n: u8) -> f64 {
    let mut value = 1.0;
    for _ in 0..n {
        value *= 10.0;
    }
    value
}

/// How far the square root of the price moves for ±2%.
fn sqrt_up() -> f64 {
    1.02_f64.sqrt()
}

fn sqrt_down() -> f64 {
    0.98_f64.sqrt()
}

/// v2 (x·y = k), in closed form. `None` while either reserve is empty.
pub fn v2(reserves: Reserves, decimals0: u8, decimals1: u8) -> Option<PoolPrice> {
    if reserves.reserve0 == 0 || reserves.reserve1 == 0 {
        return None;
    }
    let (x, y) = (reserves.reserve0 as f64, reserves.reserve1 as f64);
    // Up 2%: y grows by y(√1.02 − 1). Down 2%: x grows by x(1/√0.98 − 1), worth y/x each.
    let depth_raw = y * (sqrt_up() - 1.0) + y * (1.0 / sqrt_down() - 1.0);
    let (scale0, scale1) = (pow10(decimals0), pow10(decimals1));
    Some(PoolPrice {
        price: (y / scale1) / (x / scale0),
        depth_in_token1: depth_raw / scale1,
    })
}

/// v3, walking the initialized ticks from the current price to ±2%. `None` until the pool is
/// initialized.
pub fn v3(
    price: Option<Price>,
    liquidity: u128,
    ticks: &BTreeMap<i32, TickLiquidity>,
    decimals0: u8,
    decimals1: u8,
) -> Option<PoolPrice> {
    let price = price?;
    let q96 = f64::from(U256::from(1) << 96);
    let sqrt_at = |tick: i32| sqrt_ratio_at_tick(tick).map(|r| f64::from(r) / q96);
    let start = f64::from(price.sqrt_price_x96) / q96;
    if start <= 0.0 {
        return None;
    }

    // Up: token1 in, crossing ticks above the current one, each adding its net liquidity.
    let target = start * sqrt_up();
    let (mut active, mut at, mut token1_in) = (liquidity as f64, start, 0.0);
    for (&tick, l) in ticks.range(price.tick.saturating_add(1)..) {
        let Some(edge) = sqrt_at(tick).filter(|&edge| edge < target) else {
            break;
        };
        token1_in += active * (edge - at);
        at = edge;
        active = (active + l.net as f64).max(0.0);
    }
    token1_in += active * (target - at);

    // Down: token0 in, crossing the current tick and those below, each taking its net away.
    let target = start * sqrt_down();
    let (mut active, mut at, mut token0_in) = (liquidity as f64, start, 0.0);
    for (&tick, l) in ticks.range(..=price.tick).rev() {
        let Some(edge) = sqrt_at(tick).filter(|&edge| edge > target) else {
            break;
        };
        token0_in += active * (1.0 / edge - 1.0 / at);
        at = edge;
        active = (active - l.net as f64).max(0.0);
    }
    token0_in += active * (1.0 / target - 1.0 / at);

    let raw_price = start * start;
    let (scale0, scale1) = (pow10(decimals0), pow10(decimals1));
    Some(PoolPrice {
        price: raw_price * scale0 / scale1,
        depth_in_token1: (token1_in + token0_in * raw_price) / scale1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
    }

    #[test]
    fn pow10_is_exact_where_f64_can_be() {
        assert_eq!(pow10(0), 1.0);
        assert_eq!(pow10(6), 1e6);
        assert_eq!(pow10(18), 1e18);
    }

    #[test]
    fn v2_price_scales_by_decimals() {
        // 10 WETH (18 decimals) against 40,000 USDC (6 decimals): $4,000.
        let reserves = Reserves {
            reserve0: 10 * 10u128.pow(18),
            reserve1: 40_000 * 10u128.pow(6),
        };
        let pool = v2(reserves, 18, 6).unwrap();
        assert!(close(pool.price, 4_000.0), "{}", pool.price);
        let (usdc_in_weth, depth_in_weth) = pool.for_token(false);
        assert!(close(usdc_in_weth, 1.0 / 4_000.0));
        assert!(close(depth_in_weth, pool.depth_in_token1 / 4_000.0));
    }

    #[test]
    fn v2_depth_is_what_moves_the_price_two_percent() {
        let (x, y) = (1_000_000.0_f64, 2_000_000.0_f64);
        let pool = v2(
            Reserves {
                reserve0: x as u128,
                reserve1: y as u128,
            },
            0,
            0,
        )
        .unwrap();
        // Buy side: add dy so (y+dy)/(x') = 1.02·y/x with x'(y+dy) = xy, i.e. y+dy = y√1.02.
        let buy = y * 1.02_f64.sqrt() - y;
        // Sell side: add dx so the price is 0.98·y/x: x+dx = x/√0.98, valued at y/x.
        let sell = (x / 0.98_f64.sqrt() - x) * (y / x);
        assert!(close(pool.depth_in_token1, buy + sell));
        // About 1% of each side's value in each direction: ~2% of the pool's token1 value.
        assert!((0.019..0.021).contains(&(pool.depth_in_token1 / y)));
    }

    #[test]
    fn empty_pools_have_no_price() {
        let empty = Reserves {
            reserve0: 0,
            reserve1: 5,
        };
        assert_eq!(v2(empty, 18, 18), None);
        assert_eq!(v3(None, 1, &BTreeMap::new(), 18, 18), None);
    }

    /// A v3 pool with one position spanning every tick is a v2 pool with the same liquidity.
    #[test]
    fn a_full_range_v3_pool_matches_v2() {
        let (x, y) = (4.0e21_f64, 9.0e21_f64);
        let liquidity = (x * y).sqrt();
        let sqrt_price = (y / x).sqrt();
        let price = Price {
            sqrt_price_x96: U256::from(sqrt_price * 2f64.powi(96)),
            tick: 8_109, // log_1.0001(2.25) rounded down
        };
        let mut ticks = BTreeMap::new();
        ticks.insert(
            -887_220,
            TickLiquidity {
                gross: liquidity as u128,
                net: liquidity as i128,
            },
        );
        ticks.insert(
            887_220,
            TickLiquidity {
                gross: liquidity as u128,
                net: -(liquidity as i128),
            },
        );
        let concentrated = v3(Some(price), liquidity as u128, &ticks, 18, 18).unwrap();
        let constant_product = v2(
            Reserves {
                reserve0: x as u128,
                reserve1: y as u128,
            },
            18,
            18,
        )
        .unwrap();
        assert!(close(concentrated.price, constant_product.price));
        assert!(
            (concentrated.depth_in_token1 / constant_product.depth_in_token1 - 1.0).abs() < 1e-6
        );
    }

    /// Liquidity that ends 1% above the price counts for that 1% and no further.
    #[test]
    fn the_tick_walk_stops_counting_liquidity_where_it_ends() {
        let start_tick = 0;
        let price = Price {
            sqrt_price_x96: U256::from(1) << 96,
            tick: start_tick,
        };
        let l = 1.0e18_f64;
        // In range from tick -100 to tick 100 (±1%), nothing beyond.
        let mut ticks = BTreeMap::new();
        ticks.insert(
            -100,
            TickLiquidity {
                gross: l as u128,
                net: l as i128,
            },
        );
        ticks.insert(
            100,
            TickLiquidity {
                gross: l as u128,
                net: -(l as i128),
            },
        );
        let pool = v3(Some(price), l as u128, &ticks, 0, 0).unwrap();
        let s_up = f64::from(sqrt_ratio_at_tick(100).unwrap()) / 2f64.powi(96);
        let s_down = f64::from(sqrt_ratio_at_tick(-100).unwrap()) / 2f64.powi(96);
        let expected = l * (s_up - 1.0) + l * (1.0 / s_down - 1.0);
        assert!(
            close(pool.depth_in_token1, expected),
            "{pool:?} vs {expected}"
        );
        assert!(close(pool.price, 1.0));
    }
}
