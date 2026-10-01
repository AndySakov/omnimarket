//! Which tokens quote the others, and which pools price the native token in USD (D19).

use alloy_primitives::{Address, address};

/// Wrapped ether on Base.
pub const BASE_WETH: Address = address!("4200000000000000000000000000000000000006");
/// Circle's native USDC on Base.
pub const BASE_USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
/// Bridged Tether USD on Base.
pub const BASE_USDT: Address = address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2");

/// What a quote asset is worth in USD (D19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteKind {
    /// The native token: priced in USD from the reference pools.
    Native,
    /// Any other quote asset: a stablecoin, pinned at $1.
    Stable,
}

/// Pricing's part of the engine config. The quote assets themselves are the engine's one list
/// (`EngineConfig::quote_assets`, shared with trade records): `native` is priced from the
/// reference pools, and every other quote asset is a stablecoin pinned at $1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingConfig {
    /// The native token: WETH on Base.
    pub native: Address,
    /// Native/stablecoin pools whose weighted mid is the native token's USD price (D19).
    pub reference_pools: Vec<Address>,
    /// Pools below this ±2% depth don't count toward a display price (D18, D24). A tuning
    /// value (D11).
    pub liquidity_floor_usd: u64,
    /// How many blocks a total supply is trusted before it is read again.
    pub supply_refresh_blocks: u64,
}

impl PricingConfig {
    pub fn base() -> Self {
        let v3 = venues::v3::BASE;
        let (weth, usdc) = sorted(BASE_WETH, BASE_USDC);
        let (weth_, usdt) = sorted(BASE_WETH, BASE_USDT);
        Self {
            native: BASE_WETH,
            // Uniswap v3 WETH/USDC at 0.05% and 0.3%, WETH/USDT at 0.05%, and the v2 WETH/USDC
            // pair: weighted by depth, so the deepest dominates.
            reference_pools: vec![
                v3.pool_address(weth, usdc, 500),
                v3.pool_address(weth, usdc, 3_000),
                v3.pool_address(weth_, usdt, 500),
                venues::v2::BASE.pair_address(weth, usdc),
            ],
            liquidity_floor_usd: 10_000,
            supply_refresh_blocks: 1_800,
        }
    }

    /// `token`'s kind if it is one of `quote_assets`.
    pub fn quote(&self, quote_assets: &[Address], token: Address) -> Option<QuoteKind> {
        if !quote_assets.contains(&token) {
            None
        } else if token == self.native {
            Some(QuoteKind::Native)
        } else {
            Some(QuoteKind::Stable)
        }
    }
}

/// A pool's tokens in the factory's order.
fn sorted(a: Address, b: Address) -> (Address, Address) {
    (a.min(b), a.max(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Addresses the factories returned for these tokens on Base mainnet (2026-10-01:
    /// `getPool(WETH, USDC, 500)` and so on, through base-rpc.publicnode.com).
    #[test]
    fn the_native_token_is_native_and_other_quote_assets_are_stable() {
        let config = PricingConfig::base();
        let quotes = [BASE_USDC, BASE_USDT, BASE_WETH];
        assert_eq!(config.quote(&quotes, BASE_WETH), Some(QuoteKind::Native));
        assert_eq!(config.quote(&quotes, BASE_USDC), Some(QuoteKind::Stable));
        assert_eq!(config.quote(&quotes, Address::ZERO), None);
        // Not a quote asset, not priced as one, even if it's the native token.
        assert_eq!(config.quote(&[BASE_USDC], BASE_WETH), None);
    }

    #[test]
    fn reference_pools_are_the_live_pools() {
        assert_eq!(
            PricingConfig::base().reference_pools[..3],
            [
                address!("d0b53D9277642d899DF5C87A3966A349A798F224"),
                address!("6c561B446416E1A00E8E93E221854d6eA4171372"),
                address!("d92E0767473D1E3FF11Ac036f2b1DB90aD0aE55F"),
            ]
        );
    }
}
