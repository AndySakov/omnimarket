# Product

## Vision

A multi-chain EVM trading terminal for fast-moving tokens. A trader sees a token the moment its pool is created, understands whether it's safe and liquid, buys in one click at the best available route, and leaves automated exits running, across MegaETH, Base, and BNB Chain from one account.

The product exists to exercise the hard parts of a real terminal: ingesting chain data at head, keeping live pool state in memory, routing across fragmented liquidity, landing transactions fast and safely, and reacting to price moves automatically.

## Target user

An active on-chain trader who today switches between a chart site, a block explorer, and a trading bot. They care about, in order: **speed**, **not getting rugged or sandwiched**, and **automation**.

## Core journeys

1. **Discover.** Open the terminal, watch a live feed of new pools and trending tokens per chain. Click a token and see price, liquidity, holders, volume, a live candle chart, and a safety check (can it be sold, buy/sell tax, owner privileges).
2. **Instant buy.** Enter an amount in the native token or a stablecoin. See a quote with route, price impact, and fees. One click → signed, submitted, confirmed. Position appears with live PnL.
3. **Automated exit.** On a position, set take-profit (e.g. sell 50% at 2×), stop-loss, or a trailing stop. It fires server-side while the user is offline.
4. **Limit entry.** "Buy X if price drops to Y." Same engine as exits.
5. **Copy trade.** Follow a wallet address. When it buys or sells on a supported DEX, mirror it with the user's sizing rules and limits.
6. **Portfolio.** All positions across chains and wallets, realised and unrealised PnL, trade history.
7. **Funds.** Deposit, withdraw (with MFA), move funds between the user's wallets, export keys.

## Feature scope

### MVP (must ship)
- Indexer for pool creation and swap events on each chain's main DEXes, safe against chain reorganisations
- Live token pricing, liquidity, and candles (1s / 1m / 5m / 1h)
- New-pairs and token-detail feeds over WebSocket
- Quoting and routing across pools (multi-hop; split routes stretch)
- Execution pipeline: build → simulate → sign → submit → track, with nonce and gas management
- Token safety check via simulated buy + sell
- Take-profit / stop-loss / limit orders
- Positions and PnL
- Embedded wallets, several per user (see [wallets.md](wallets.md))

### Stretch
- Copy trading
- Trailing stops
- Split routing across pools
- Private / MEV-protected submission on BNB
- Smart-account session keys as an alternative wallet mode

### Phase 2 (D7): prop AMM on MegaETH
- On-chain pool whose price is pushed by our quoting service several times per block
- Pricing from the Chain Engine's cross-chain fair price, plus spread and inventory skew
- Gas-cheap, top-of-block price updates; inventory and risk limits
- Our router trades against it as an opaque venue
- Specced separately (`prop-amm.md`) after phase 1 is locked

### Non-goals
- Our own order book or matching engine
- Solana
- Mobile app, Telegram bot, browser extension (the API should make these possible, but we don't build them)
- Fiat on-ramp, perps, real users, token launchpad

## DEX coverage (to confirm per chain)

| Chain | Likely venues | Notes |
|---|---|---|
| Base | Uniswap v2/v3/v4, Aerodrome | Aerodrome uses Solidly-style pools, so it needs its own pricing math |
| BNB Chain | PancakeSwap v2/v3 (+ Infinity) | Public mempool, so sandwich risk is real |
| MegaETH | **(verify)** — pick the dominant 1–2 venues | Newest chain with the thinnest tooling; highest ingest rate |

## Draft performance targets

Placeholders to be hardened in `slas.md`. Listed now so they shape design discussions.

| Metric | Draft target |
|---|---|
| Indexer lag behind chain head (p99) | ≤ 1 block (Base/BNB); ≤ 250ms (MegaETH) |
| Quote latency (p99) | ≤ 25ms |
| Click → transaction broadcast (p99) | ≤ 150ms |
| Price move → trigger order broadcast (p99) | ≤ 300ms |
| Price tick → client WebSocket (p99) | ≤ 100ms |
| Concurrent active trigger orders | 100k |

## Open questions

1. ~~Demo environment~~ → decided in D5 (live reads, shadow execution, real-funds proof).
2. **Router contract:** our own on-chain router (fees, bundling approve + swap, safety checks on-chain) vs calling DEX routers directly.
3. **Frontend depth:** full terminal UI vs a thin UI that exists to demo the backend.
4. **Prop AMMs as a venue class.** Proprietary AMMs (Tessera, ElfomoFi on Base; HumidiFi-style on Solana) quote from market-maker pricing that updates several times per block, so their output can't be computed from indexed state. Supporting them means a simulation-based quote adapter with very short quote lifetimes. Relevant mainly for major-pair legs (e.g. USDC → ETH), not memecoin pools. Which chains have them: Base confirmed, BNB and MegaETH **(verify)**. → `routing.md`
5. **Historical backfill:** how much history to index per chain (days vs from genesis of each DEX).
