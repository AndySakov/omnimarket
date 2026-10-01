# Product

## Vision

A multi-chain EVM trading terminal for fast-moving tokens. A trader sees a token the moment its pool is created, understands whether it's safe and liquid, buys in one click at the best available route, and leaves automated exits running, across MegaETH, Base, and BNB Chain from one account.

It's a proof of concept (D85), not a commercial product. It exists to exercise the hard parts of a real terminal: ingesting chain data at head, keeping live pool state in memory, routing across fragmented liquidity, landing transactions fast and safely, and reacting to price moves automatically.

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
- Quoting and routing across pools: single, multi-hop and split, chosen per order (D25)
- Bonding-curve venues, four.meme first (D36)
- Intent-based execution: route → simulate → executor submits → track (D42)
- Private / MEV-protected submission on BNB (D30)
- Token safety checks: simulation, contract inspection, liquidity, behaviour (D29)
- Full order catalogue incl. trailing stops, multi-level TP, dev-sell, migration, scheduled (D37)
- Copy trading (D38)
- Positions and PnL
- Embedded wallets, several per user (see [wallets.md](wallets.md))
- Observability, replay and brakes (D52–D56)

*(Scope updated by the final verification pass: items once listed as stretch were promoted by later decisions.)*

### Stretch
- Own BNB and MegaETH nodes (D44 evaluation)
- EIP-7702 delegate as intent carrier (D47, phase 2 candidate)

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
| Base | Uniswap v2/v3/v4 (incl. launchpad hook pools: Clanker, Zora, Flaunch), Aerodrome | Aerodrome uses Solidly-style pools, so it needs its own pricing math |
| BNB Chain | PancakeSwap v2/v3 + Infinity (CL and bin pools), four.meme bonding curves | Public mempool, so sandwich risk is real |
| MegaETH | Kumbaya (dominant, ~80% of chain TVL early 2026) + Algebra-based pools | Newest chain with the thinnest tooling; highest ingest rate |

## Draft performance targets

Superseded by [slas.md](slas.md) (D46): trigger ≤ 50ms, click ≤ 100ms, quote ≤ 10ms, tick ≤ 100ms (p99, internal). Original drafts kept below for reference.

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
2. ~~Router contract~~ → our own immutable router executing signed intents (D26, D42).
3. ~~Frontend depth~~ → both: a thin prototyping UI and a full terminal UI owned by Jutin (D62, [frontend.md](frontend.md)).
4. **Prop AMMs as a venue class.** Proprietary AMMs (Tessera, ElfomoFi on Base; HumidiFi-style on Solana) quote from market-maker pricing that updates several times per block, so their output can't be computed from indexed state. Supporting them means a simulation-based quote adapter with very short quote lifetimes. Relevant mainly for major-pair legs (e.g. USDC → ETH), not memecoin pools. Which chains have them: Base and BNB confirmed (BNB: e.g. LunarBase on BNB/USDT, BTCB/USDT); none found on MegaETH yet. → `routing.md`
5. ~~Historical backfill~~ → 30 days per chain at launch (D15).
