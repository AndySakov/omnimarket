# Pricing

**Status:** Draft. Decisions D18–D24, D100. Display price, USD conversion, depth and token metadata built for Base (`crates/pricing`, in the engine, M2).

## Three prices (D18)

| Price | Used for | Definition |
|---|---|---|
| Display | Ticker, token page, PnL marks, fair price | Liquidity-weighted mid across active pools above the liquidity floor |
| Trigger | Stops, take-profits, limits | Display price, fired instantly (D20) |
| Execution | Trades | Router quote at trade size |

Candles come from swap prices. Token pages also show the deepest pool's price.

### Display price

For token T with active pools p₁…pₙ above the liquidity floor:

`price(T) = Σ (mid(pᵢ) · liquidity(pᵢ)) / Σ liquidity(pᵢ)`

- `mid(p)`: the pool's marginal price, converted to USD (D19).
- `liquidity(p)`: the pool's **±2% depth** in USD, buy + sell side (D24). Same measure sets the D11 liquidity floor.
- No pools above the floor → priced from its deepest pool and flagged **thin** in the UI; triggers still evaluate (D20).
- Recomputed on every pool update affecting T, in the Chain Engine.

### Built (M2, D100)

`crates/pricing` is pure functions of the state the engine holds; the engine runs them and publishes.

- **Which pools.** A token's pools against a quote asset (Base: WETH, USDC, USDT). A pool between two other tokens prices neither. Until D11's tiers land (#41), every pool the engine tracks counts; the floor applies already.
- **Pool mid.** v2: `reserve1 / reserve0`. v3: `(sqrtPriceX96 / 2⁹⁶)²`. Both scaled by `10^(decimals0 − decimals1)` to whole tokens, then inverted if the token is token1.
- **±2% depth (D24).** Fee-free. Buy side: the quote that moves the price up 2%. Sell side: the token that moves it down 2%, valued at the mid. v2 in closed form, `y(√1.02 − 1) + y(1/√0.98 − 1)` in token1; v3 by walking initialized ticks from the current price to `√P·√1.02` (adding each crossed tick's net liquidity) and to `√P·√0.98` (subtracting it), with `Δtoken1 = L·Δ√P` and `Δtoken0 = L·Δ(1/√P)` per stretch.
- **USD.** `mid × usd(quote)`, `depth × usd(quote)`: WETH's latest display price, or $1 for a stablecoin.
- **Display price.** `Σ price·depth / Σ depth` over the pools at or above the floor ($10,000, a D11 tuning value). None at or above it: the deepest pool's price, **thin**. The update also carries the deepest (main) pool, the display price in its quote asset, and every pool's own price and depth.
- **Native token.** WETH is priced from the reference pools only: Uniswap v3 WETH/USDC 0.05% and 0.3%, WETH/USDT 0.05%, v2 WETH/USDC. Stablecoins are pinned and publish no update.
- **Arithmetic.** `f64` from exact integers, with only +, −, ×, ÷ and `sqrt` (correctly rounded everywhere); tick boundaries from Uniswap's integer `getSqrtRatioAtTick`; 10ⁿ by repeated multiplication. A replay reprices bit for bit (D100).
- **Cadence.** After each canonical block (D77): every token whose pools changed since the last block, WETH first. A WETH move doesn't republish every WETH-quoted token; each update names the WETH price it used (D22's lazy path).

### Token metadata (D100)

- `name`, `symbol`, `decimals`, `totalSupply`, read in one Multicall3 call per 50 tokens (four calls each) through the engine's recorded calls and the call worker's rate limits (D82), at the end of the block the token's first quote-asset pool updated. Read once; a call that fails as a whole is forgotten and read again on the token's next update.
- **Fallbacks.** A `bytes32` name or symbol (early tokens such as MKR) is read up to its first trailing zero; control characters are dropped; a revert, an empty string or an unreadable answer leaves the field unset. Decimals above 77 or unreadable: unset, and the token isn't priced.
- **Supply** is read again once it is 1,800 blocks old (about an hour), for priced tokens, oldest first, 50 a block. The engine doesn't follow `Transfer` logs, so a mint or burn shows within the hour.
- **Market cap** = total supply × display price, published as `fdv_usd`: total supply counts locked and unvested tokens, so it's a fully diluted value. Unset without a supply.

### USD conversion (D19)

- Per chain, a fixed set of **reference pools** (deepest native/stablecoin pools) prices the native token in USD, using the same liquidity-weighted mid.
- Quote assets in phase 1: native token + reference stablecoins. `usd(T) = price(T in quote) × usd(quote)`.
- Stablecoins pinned at $1 while reference stablecoins agree within ~0.5%; beyond that, priced from their pools against each other, with a depeg warning in the UI. **Not built yet:** stablecoins are always pinned (D100).
- Quote-asset coverage on Base: 53% of tokens in updated pools have a pool against WETH, USDC or USDT; most of the rest trade against one token, ADS ([verification.md](verification.md#measured-base-pricing-m2)).

## Quoting (D21)

In-memory math per pool type, exact to the contract's rounding; simulation only for opaque venues. A background shadow check compares sampled quotes with on-chain simulation.

| Pool type | Chains | Math |
|---|---|---|
| Uniswap v2 + forks | All | Constant product |
| Uniswap v3/v4 + forks | All | Concentrated liquidity, tick walk |
| v4 hooks | All | Modelled or simulated (D14) |
| Aerodrome volatile / stable / Slipstream | Base | Constant product / stable curve / concentrated |
| Kumbaya | MegaETH | v3-like, non-standard bytecode: simulated until shadow check passes |
| Algebra-based | MegaETH | Algebra concentrated liquidity, dynamic fees |
| PancakeSwap Infinity (CL + bin pools, hooks) | BNB | CL: v4-like · bin pools: own math **(to spec)** |

**Pool state (built, `venues`, M1):** v2 reserves from `Sync`; v3 price, tick, active liquidity and the initialized ticks' gross and net liquidity from `Initialize`, `Swap`, `Mint` and `Burn`, applied as the pool contract does (active liquidity changes only when `tickLower <= tick < tickUpper`). Quoting math on top of this state lands with routing (M4).

## Recompute cadence (D22)

| Work | When |
|---|---|
| Token price, trigger check | Every pool update |
| Client push | Throttled, ≤10/s per token, latest wins |
| Quote-asset (ETH/BNB/stable) move | Convert levels, check only orders crossed; USD prices derived lazily |

## Fair price (D23)

- Single-chain tokens: fair price = display price.
- Curated cross-chain assets (ETH, BNB, USDC, USDT, wrapped BTC): ±2%-depth-weighted average of per-chain display prices, via a hand-maintained address map.
- Published by a small aggregator reading per-chain price updates from Kafka. Phase 2 needs a faster feed.

## Open questions

Biggest first:

1. ~~USD conversion~~ → **decided (D19).**
2. ~~Trigger manipulation protection~~ → **decided (D20): none beyond a slippage limit**, Trojan-style instant triggers.
3. ~~Per-DEX math~~ → **decided (D21): in memory**, simulation for opaque venues. MegaETH venues checked; PancakeSwap Infinity bin pools added to scope.
4. ~~Recompute cadence~~ → **decided (D22).**
5. ~~Cross-chain fair price~~ → **decided (D23).**
6. ~~Liquidity measure~~ → **decided (D24): ±2% depth.**

Remaining: verify MegaETH venues (D21); tuning values (liquidity floor per chain, client push rate); D19's depeg check.
