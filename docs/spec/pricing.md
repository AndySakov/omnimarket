# Pricing

**Status:** Draft. Decisions D18–D24.

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

### USD conversion (D19)

- Per chain, a fixed set of **reference pools** (deepest native/stablecoin pools) prices the native token in USD, using the same liquidity-weighted mid.
- Quote assets in phase 1: native token + reference stablecoins. `usd(T) = price(T in quote) × usd(quote)`.
- Stablecoins pinned at $1 while reference stablecoins agree within ~0.5%; beyond that, priced from their pools against each other, with a depeg warning in the UI.

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

**Pool state (built, `venues`, M1):** v2 reserves from `Sync`; v3 price, tick, active liquidity and the initialized ticks' gross and net liquidity from `Initialize`, `Swap`, `Mint` and `Burn`, applied as the pool contract does (active liquidity changes only when `tickLower <= tick < tickUpper`). Quoting math on top of this state lands with routing (M4). `venues` also decodes each venue's `Swap` into the amounts the pool took in and paid out (v2 nets `amountIn − amountOut` per token), for trade records (D102).

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

Remaining: verify MegaETH venues (D21); tuning values (liquidity floor per chain, client push rate).
