//! `UniswapV2Library.getAmountOut`: the pair's 0.3% fee, rounded down.

use alloy_primitives::U256;

/// What a pair with `reserve_in` and `reserve_out` pays for `amount_in`. `None` for an empty
/// side or an output past `uint256`.
pub fn amount_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> Option<U256> {
    if amount_in.is_zero() || reserve_in.is_zero() || reserve_out.is_zero() {
        return None;
    }
    let with_fee = amount_in.checked_mul(U256::from(997u64))?;
    let numerator = with_fee.checked_mul(reserve_out)?;
    let denominator = reserve_in
        .checked_mul(U256::from(1000u64))?
        .checked_add(with_fee)?;
    Some(numerator / denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pays_the_constant_product_less_the_fee_rounded_down() {
        // 1 in against 1,000/1,000: 997·1000 / (1000·1000 + 997) = 0.996...: nothing.
        let r = U256::from(1_000u64);
        assert_eq!(amount_out(U256::from(1u64), r, r), Some(U256::ZERO));
        // 100 in: 99,700,000 / 1,099,700 = 90.66: 90.
        assert_eq!(
            amount_out(U256::from(100u64), r, r),
            Some(U256::from(90u64))
        );
        assert_eq!(amount_out(U256::ZERO, r, r), None);
        assert_eq!(amount_out(U256::from(1u64), U256::ZERO, r), None);
    }
}
