# Frontend

**Status:** Draft. Decision: D62. Owners: thin prototyping UI (project lead); full terminal UI (**Jutin**, including stack choices).

## Principles

1. **The trader comes first.** Every screen is judged by how fast and how safely it gets a trader from "I see a token" to "I'm in, with exits set".
2. **Mirror the leaders where users have muscle memory.** Trojan, Axiom, Photon and GMGN have trained traders on layouts and flows; matching them lowers switching cost.
3. **Do better where our backend lets us.** Some things they can't show us, we can (see "Where we can beat them").
4. **One API for both UIs.** Nothing the full terminal needs is hidden from the thin UI, and vice versa.

## Screens to mirror

| Screen | Industry reference | What it shows | Backend it uses |
|---|---|---|---|
| **Discovery feed** | Axiom Pulse, Trojan "Trenches" | Columns: *New* (just launched), *Final stretch* (bonding curve close to graduating, with progress), *Migrated* (graduated to a DEX pool). Filters by chain, liquidity, safety, age | D11, D36, D29 |
| **Token page** | Trojan trade page, Photon | Advanced chart (drawing tools, indicators), price and market cap, liquidity, holders, recent trades, safety panel, dev wallet activity | D18, D24, D29, D37, D41 |
| **Trade panel** | Trojan, Axiom | Instant buy/sell with **presets** (amounts, slippage, tip level), quote with route, price impact and fees, one click to trade | D25, D27, D28, D33 |
| **Orders on the chart** | Trojan visual limit orders | Drag-to-set limit, TP, SL and trailing levels on the chart; multi-level TP | D37, D39 |
| **Auto-sell** | Trojan global multi-level autosell | Global TP/SL ladder applied automatically to every buy | D37 |
| **Positions and portfolio** | All leaders | Positions across chains and wallets, realised/unrealised PnL, history; **shareable PnL cards** | D37, D41 |
| **Wallet tracker, analyzer, copy trading** | Trojan wallet tracker + analyzer | Follow wallets, see their trades and stats, copy with sizing rules and filters | D38 |
| **Wallets** | Trojan wallet management | Up to 10 wallets per user, move funds between them, deposit, withdraw (MFA), export keys | D3, D4, D57 |
| **Settings** | All leaders | Default slippage per situation, tip levels, exit guarantee default, notifications | D27, D33, D60 |
| **Referrals and rewards** | Trojan Arena (later) | Referral links and tiers; gamified rewards are a later product call | D28 |

## Where we can beat them

Things our backend makes possible that traders don't get elsewhere:

| Idea | Why only we can | Backend |
|---|---|---|
| **"Why did this fire?"** on every order: the exact swap that moved the price, the price it crossed, the route chosen and why, the fill | Full lineage on every record | D53 |
| **Verifiable trade receipts:** intent signed, route, transaction, fill vs signed minimum, fee and gas, each linkable to the chain | Signed intents + receipts | D52, D59 |
| **Live execution timing:** how long each trade took, step by step | Per-step traces | D46, D53 |
| **Safety with evidence:** not just a red badge, but the simulated sell result, measured tax, owner powers found, liquidity lock status | Four-layer safety checks | D29 |
| **Slippage and tip explained:** "new pair: 15%", "stop-loss: high tip" | Situation-based defaults | D27, D33 |
| **No gas balance needed:** trades work with zero ETH/BNB for gas | Executors pay and recover gas | D42 |
| **Exit guarantee toggle** on every stop, clearly explained | D60 | D60 |
| **Public status and brake transparency:** live system status, any active brake and when it expires | Public brake log | D52, D55, D56 |
| **Cross-chain view:** one portfolio and one fair price per asset across MegaETH, Base and BNB | Fair price | D23 |
| **Honest about thin tokens:** "thin" flag instead of a manipulable price | Thin-token handling | D18, D20 |

## API contract (backend ↔ both UIs)

- **REST** for commands and queries: quotes, placing and editing orders (returns the intent to sign), positions, history, settings, wallets.
- **WebSocket feeds:** discovery feed, token detail, price ticks (≤ 20/s per token, deltas, D43), trades, positions, order status, and a **trade status stream** with per-step timestamps.
- **Signing in the browser:** order and trade requests return the EIP-712 intent; the UI signs it in the user's Privy session and submits it (D57).
- **Types generated** from the Protobuf schemas (D41) for TypeScript, so frontend and backend can't drift.
- **Mock server:** replays recorded data (D54) through the real API shape, so the terminal can be built and demoed before the backend is live.

## Open questions (Jutin's call, noted for alignment)

1. Charting library (TradingView Advanced Charts vs Lightweight Charts vs custom).
2. Framework and state management for high-frequency feeds.
3. Mobile layout scope (Trojan ships a mobile web view).
