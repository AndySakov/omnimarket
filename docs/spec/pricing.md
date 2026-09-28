# Pricing

**Status:** Draft.

## Three prices (D18)

| Price | Used for | Definition |
|---|---|---|
| Display | Ticker, token page, PnL marks, fair price | Liquidity-weighted mid across active pools above the liquidity floor |
| Trigger | Stops, take-profits, limits | Display price + manipulation protection (open) |
| Execution | Trades | Router quote at trade size |

Candles come from swap prices. Token pages also show the deepest pool's price.

### Display price

For token T with active pools p₁…pₙ above the liquidity floor:

`price(T) = Σ (mid(pᵢ) · liquidity(pᵢ)) / Σ liquidity(pᵢ)`

- `mid(p)`: the pool's marginal price, converted to USD (D19).
- `liquidity(p)`: value on the pool's quote side in USD; for concentrated liquidity (v3/v4), liquidity near the current price, not total deposits **(define precisely)**.
- No pools above the floor → token is **unpriced (thin)**: shown with a warning, no trigger evaluation on the display price.
- Recomputed on every pool update affecting T, in the Chain Engine.

### USD conversion (D19)

- Per chain, a fixed set of **reference pools** (deepest native/stablecoin pools) prices the native token in USD, using the same liquidity-weighted mid.
- Quote assets in phase 1: native token + reference stablecoins. `usd(T) = price(T in quote) × usd(quote)`.
- Stablecoins pinned at $1 while reference stablecoins agree within ~0.5%; beyond that, priced from their pools against each other, with a depeg warning in the UI.

## Open questions

Biggest first:

1. ~~USD conversion~~ → **decided (D19).**
2. **Trigger manipulation protection.** Minimum liquidity, confirmation delay, short smoothing window, or a combination. Decides whether the ≤300ms price move → trigger target is honest.
3. **Per-DEX math.** Uniswap v2/v3/v4 (incl. hook pools, D14), Aerodrome stable curves, PancakeSwap, opaque simulated venues.
4. **Recompute cadence.** Every pool update vs coalescing per mini-block at MegaETH rates.
5. **Cross-chain fair price** (D7).
6. **Liquidity measure** for v3/v4 weighting (depth within ±x% of mid?).
