//! Which tokens quote the others, and which pools price the native token in USD (D19).

use alloy_primitives::{Address, address};

/// Wrapped ether on Base.
pub const BASE_WETH: Address = address!("4200000000000000000000000000000000000006");
/// Circle's native USDC on Base.
pub const BASE_USDC: Address = address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913");
/// Bridged Tether USD on Base.
pub const BASE_USDT: Address = address!("fde4C96c8593536E31F229EA8f37b2ADa2699bb2");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteKind {
    /// Priced in USD from the reference pools.
    Native,
    /// Pinned at $1 (D19).
    Stable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteAsset {
    pub token: Address,
    pub kind: QuoteKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PricingConfig {
    /// The tokens other tokens are priced against. A token paired only with others is
    /// unpriced (D19).
    pub quote_assets: Vec<QuoteAsset>,
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
            quote_assets: vec![
                QuoteAsset {
                    token: BASE_WETH,
                    kind: QuoteKind::Native,
                },
                QuoteAsset {
                    token: BASE_USDC,
                    kind: QuoteKind::Stable,
                },
                QuoteAsset {
                    token: BASE_USDT,
                    kind: QuoteKind::Stable,
                },
            ],
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

    pub fn quote(&self, token: Address) -> Option<QuoteKind> {
        self.quote_assets
            .iter()
            .find(|q| q.token == token)
            .map(|q| q.kind)
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
