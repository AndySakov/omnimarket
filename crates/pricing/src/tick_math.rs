//! Uniswap v3's `TickMath.getSqrtRatioAtTick`, bit for bit, so tick boundaries come from
//! integers rather than a float power.

use alloy_primitives::U256;
use venues::v3::{MAX_TICK, MIN_TICK};

/// `sqrt(1.0001^tick) · 2^96`, rounded up as the contract does. `None` outside the tick range.
pub fn sqrt_ratio_at_tick(tick: i32) -> Option<U256> {
    if !(MIN_TICK..=MAX_TICK).contains(&tick) {
        return None;
    }
    let abs = tick.unsigned_abs();
    // Each constant is 2^128 / sqrt(1.0001^(2^i)), from the contract.
    const FACTORS: [u128; 19] = [
        0xfff97272373d413259a46990580e213a,
        0xfff2e50f5f656932ef12357cf3c7fdcc,
        0xffe5caca7e10e4e61c3624eaa0941cd0,
        0xffcb9843d60f6159c9db58835c926644,
        0xff973b41fa98c081472e6896dfb254c0,
        0xff2ea16466c96a3843ec78b326b52861,
        0xfe5dee046a99a2a811c461f1969c3053,
        0xfcbe86c7900a88aedcffc83b479aa3a4,
        0xf987a7253ac413176f2b074cf7815e54,
        0xf3392b0822b70005940c7a398e4b70f3,
        0xe7159475a2c29b7443b29c7fa6e889d9,
        0xd097f3bdfd2022b8845ad8f792aa5825,
        0xa9f746462d870fdf8a65dc1f90e061e5,
        0x70d869a156d2a1b890bb3df62baf32f7,
        0x31be135f97d08fd981231505542fcfa6,
        0x9aa508b5b7a84e1c677de54f3e99bc9,
        0x5d6af8dedb81196699c329225ee604,
        0x2216e584f5fa1ea926041bedfe98,
        0x48a170391f7dc42444e8fa2,
    ];
    let mut ratio = if abs & 1 != 0 {
        U256::from(0xfffcb933bd6fad37aa2d162d1a594001_u128)
    } else {
        U256::from(1_u64) << 128_usize
    };
    for (i, factor) in FACTORS.iter().enumerate() {
        if abs & (2 << i) != 0 {
            ratio = (ratio * U256::from(*factor)) >> 128;
        }
    }
    if tick > 0 {
        ratio = U256::MAX / ratio;
    }
    // From Q128.128 to Q64.96, rounding up.
    let rounded_up = !(ratio % (U256::from(1_u64) << 32_usize)).is_zero();
    Some((ratio >> 32) + U256::from(rounded_up as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_contracts_bounds_and_midpoint() {
        // TickMath.MIN_SQRT_RATIO and MAX_SQRT_RATIO.
        assert_eq!(
            sqrt_ratio_at_tick(MIN_TICK),
            Some(U256::from(4295128739_u64))
        );
        assert_eq!(
            sqrt_ratio_at_tick(MAX_TICK),
            Some(
                "1461446703485210103287273052203988822378723970342"
                    .parse()
                    .unwrap()
            )
        );
        assert_eq!(sqrt_ratio_at_tick(0), Some(U256::from(1_u64) << 96_usize));
        assert_eq!(sqrt_ratio_at_tick(MAX_TICK + 1), None);
        assert_eq!(sqrt_ratio_at_tick(MIN_TICK - 1), None);
    }

    #[test]
    fn matches_the_contract_at_sampled_ticks() {
        // Values from Uniswap v3-core's TickMath tests.
        assert_eq!(
            sqrt_ratio_at_tick(50),
            Some("79426470787362580746886972461".parse().unwrap())
        );
        assert_eq!(
            sqrt_ratio_at_tick(-50),
            Some("79030349367926598376800521322".parse().unwrap())
        );
    }
}
