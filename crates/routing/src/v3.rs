//! Uniswap v3's exact-input swap, as `UniswapV3Pool.swap` computes it: `SwapMath`,
//! `SqrtPriceMath` and the tick walk, in the contract's integer arithmetic and rounding.
//!
//! The pool steps at most one bitmap word (256 tick spacings) at a time, even with no tick to
//! cross, and each step rounds on its own, so the walk keeps the contract's word boundaries.

use alloy_primitives::{U256, U512};
use pricing::tick_math::sqrt_ratio_at_tick;
use venues::v3::{MAX_TICK, MIN_TICK, Pool};

const Q96: U256 = U256::from_limbs([0, 1 << 32, 0, 0]);
/// `TickMath.MIN_SQRT_RATIO + 1` and `MAX_SQRT_RATIO - 1`: the price limits a quote swaps to.
fn min_sqrt_limit() -> U256 {
    U256::from(4_295_128_740u64)
}
fn max_sqrt_limit() -> U256 {
    "1461446703485210103287273052203988822378723970341"
        .parse()
        .expect("a literal")
}

/// The output of swapping `amount_in` of token0 (`zero_for_one`) or token1 through `pool`, and
/// whether all of it was used. `None` for an uninitialized pool or a result past `uint256`.
pub fn amount_out(pool: &Pool, zero_for_one: bool, amount_in: U256) -> Option<Swapped> {
    let price = pool.price?;
    let limit = if zero_for_one {
        min_sqrt_limit()
    } else {
        max_sqrt_limit()
    };
    let mut remaining = amount_in;
    let mut out = U256::ZERO;
    let mut sqrt_price = price.sqrt_price_x96;
    let mut tick = price.tick;
    let mut liquidity = pool.liquidity;
    while !remaining.is_zero() && sqrt_price != limit {
        let (next, initialized) = next_tick_within_word(pool, tick, zero_for_one);
        let next = next.clamp(MIN_TICK, MAX_TICK);
        let sqrt_next = sqrt_ratio_at_tick(next)?;
        let target = if zero_for_one {
            sqrt_next.max(limit)
        } else {
            sqrt_next.min(limit)
        };
        let step = swap_step(sqrt_price, target, liquidity, remaining, pool.fee)?;
        sqrt_price = step.sqrt_price;
        remaining = remaining.checked_sub(step.amount_in.checked_add(step.fee)?)?;
        out = out.checked_add(step.amount_out)?;
        if sqrt_price == sqrt_next {
            if initialized {
                let net = pool.ticks.get(&next).map_or(0, |t| t.net);
                let net = if zero_for_one {
                    net.checked_neg()?
                } else {
                    net
                };
                liquidity = liquidity.checked_add_signed(net)?;
            }
            tick = if zero_for_one { next - 1 } else { next };
        } else {
            // A partial step spends what's left: the swap is done.
            break;
        }
    }
    Some(Swapped {
        amount_out: out,
        unspent: remaining,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Swapped {
    pub amount_out: U256,
    /// Input left when the price reached its limit: the pool can't take it all.
    pub unspent: U256,
}

/// `TickBitmap.nextInitializedTickWithinOneWord` over the pool's initialized ticks.
fn next_tick_within_word(pool: &Pool, tick: i32, lte: bool) -> (i32, bool) {
    let spacing = pool.tick_spacing;
    let compressed = tick.div_euclid(spacing);
    if lte {
        let word_start = (compressed >> 8) << 8;
        match pool
            .ticks
            .range(word_start * spacing..=compressed * spacing)
            .next_back()
        {
            Some((t, _)) => (*t, true),
            None => (word_start * spacing, false),
        }
    } else {
        let first = compressed + 1;
        let word_end = ((first >> 8) << 8) + 255;
        match pool
            .ticks
            .range(first * spacing..=word_end * spacing)
            .next()
        {
            Some((t, _)) => (*t, true),
            None => (word_end * spacing, false),
        }
    }
}

struct Step {
    sqrt_price: U256,
    amount_in: U256,
    amount_out: U256,
    fee: U256,
}

/// `SwapMath.computeSwapStep` for an exact input.
fn swap_step(
    current: U256,
    target: U256,
    liquidity: u128,
    remaining: U256,
    fee_pips: u32,
) -> Option<Step> {
    let zero_for_one = current >= target;
    let fee = U256::from(fee_pips);
    let million = U256::from(1_000_000u64);
    let less_fee = mul_div(remaining, million - fee, million)?;
    let to_target = if zero_for_one {
        amount0_delta(target, current, liquidity, true)?
    } else {
        amount1_delta(current, target, liquidity, true)?
    };
    let next = if less_fee >= to_target {
        target
    } else {
        next_sqrt_price_from_input(current, liquidity, less_fee, zero_for_one)?
    };
    let reached = next == target;
    let (amount_in, amount_out) = if zero_for_one {
        (
            if reached {
                to_target
            } else {
                amount0_delta(next, current, liquidity, true)?
            },
            amount1_delta(next, current, liquidity, false)?,
        )
    } else {
        (
            if reached {
                to_target
            } else {
                amount1_delta(current, next, liquidity, true)?
            },
            amount0_delta(current, next, liquidity, false)?,
        )
    };
    let fee = if !reached {
        remaining.checked_sub(amount_in)?
    } else {
        mul_div_up(amount_in, fee, million - fee)?
    };
    Some(Step {
        sqrt_price: next,
        amount_in,
        amount_out,
        fee,
    })
}

/// `SqrtPriceMath.getAmount0Delta`.
fn amount0_delta(a: U256, b: U256, liquidity: u128, round_up: bool) -> Option<U256> {
    let (low, high) = if a > b { (b, a) } else { (a, b) };
    if low.is_zero() {
        return None;
    }
    let numerator1 = U256::from(liquidity) << 96;
    let numerator2 = high - low;
    if round_up {
        let x = mul_div_up(numerator1, numerator2, high)?;
        Some(x.div_ceil(low))
    } else {
        Some(mul_div(numerator1, numerator2, high)? / low)
    }
}

/// `SqrtPriceMath.getAmount1Delta`.
fn amount1_delta(a: U256, b: U256, liquidity: u128, round_up: bool) -> Option<U256> {
    let (low, high) = if a > b { (b, a) } else { (a, b) };
    let liquidity = U256::from(liquidity);
    if round_up {
        mul_div_up(liquidity, high - low, Q96)
    } else {
        mul_div(liquidity, high - low, Q96)
    }
}

/// `SqrtPriceMath.getNextSqrtPriceFromInput`.
fn next_sqrt_price_from_input(
    sqrt_price: U256,
    liquidity: u128,
    amount_in: U256,
    zero_for_one: bool,
) -> Option<U256> {
    if sqrt_price.is_zero() || liquidity == 0 {
        return None;
    }
    if zero_for_one {
        // getNextSqrtPriceFromAmount0RoundingUp, adding.
        if amount_in.is_zero() {
            return Some(sqrt_price);
        }
        let numerator1: U256 = U256::from(liquidity) << 96;
        let (product, overflowed) = amount_in.overflowing_mul(sqrt_price);
        if !overflowed {
            let (denominator, overflowed) = numerator1.overflowing_add(product);
            if !overflowed {
                return mul_div_up(numerator1, sqrt_price, denominator);
            }
        }
        Some(numerator1.div_ceil((numerator1 / sqrt_price).checked_add(amount_in)?))
    } else {
        // getNextSqrtPriceFromAmount1RoundingDown, adding.
        let liquidity = U256::from(liquidity);
        let quotient = if amount_in <= (U256::from(1u64) << 160) - U256::from(1u64) {
            (amount_in << 96) / liquidity
        } else {
            mul_div(amount_in, Q96, liquidity)?
        };
        sqrt_price.checked_add(quotient)
    }
}

/// `FullMath.mulDiv`: a·b/d rounded down, `None` past `uint256` or for d = 0.
fn mul_div(a: U256, b: U256, d: U256) -> Option<U256> {
    if d.is_zero() {
        return None;
    }
    let q = U512::from(a) * U512::from(b) / U512::from(d);
    U256::checked_from_limbs_slice(q.as_limbs())
}

/// `FullMath.mulDivRoundingUp`.
fn mul_div_up(a: U256, b: U256, d: U256) -> Option<U256> {
    if d.is_zero() {
        return None;
    }
    let product = U512::from(a) * U512::from(b);
    let d = U512::from(d);
    let q = product / d + U512::from(!(product % d).is_zero() as u8);
    U256::checked_from_limbs_slice(q.as_limbs())
}
