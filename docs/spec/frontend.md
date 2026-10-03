# Frontend

**Status:** Draft. Decision: D62. Owners: thin prototyping UI (project lead); full terminal UI (**Jutin**, including stack choices).

## Principles

1. **The trader comes first.** Every screen is judged by how fast and how safely it gets a trader from "I see a token" to "I'm in, with exits set".
2. **Mirror the leaders where users have muscle memory.** Trojan, Axiom, Photon and GMGN have trained traders on layouts and flows; matching them means nothing new to learn.
3. **Show what the backend makes possible** (see "What the backend adds").
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
| **Referrals and rewards** | Trojan Arena | Out of scope for the proof of concept | D85 |

## What the backend adds

Things this backend's design makes cheap to show:

| Idea | What makes it possible | Backend |
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

### Contract v0 (D91)

The demo's API scope is #62's: its stages and issues set what v0 serves, and D96 records the demo slice and its shortcuts (shadow execution only, demo accounts with shadow balances, intents on a placeholder EIP-712 domain).

The messages the demo (#62) needs, in `proto/omnimarket/api/v1`: `common.proto` (token references, route legs, fees), `market.proto`, `trading.proto`, `automation.proto` and `stream.proto` (the WebSocket envelope). The proto comments are the field-level reference.

**Encoding.** proto3 JSON on the wire:
- Amounts, prices, USD values and percentages are **decimal strings**, exact, with no exponent. Token amounts are in whole-token units; percentages in percent (`"-3"` is −3%); slippage and fees in basis points. The UI parses them into `decimal.js` and formats only at the edge.
- Addresses and hashes are lowercase `0x` hex strings. Lineage IDs are 16 bytes, base64 in JSON.
- Times are Unix milliseconds. 64-bit integers (times, block numbers, counts, chain IDs) travel as JSON strings and are `bigint` in TypeScript.
- Every record carries `lineage` (D53, D71). Requests carry a client-chosen `client_request_id` instead; a repeat with the same ID is the same request, so a retried trade never fills twice.
- Shadow records say so: `mode` is `EXECUTION_MODE_SHADOW` on accounts, receipts, positions and orders, and the UI labels them.

**REST (v0, served by #78 and the issues it unblocks).**

| Request | Response | Issue |
|---|---|---|
| `GET /v1/status` | `EngineStatus` | #79 |
| `GET /v1/discovery?list=&min_depth_usd=&max_age_ms=` | `DiscoveryFeed` | #81 |
| `GET /v1/tokens/{chain_id}/{address}` | `TokenSnapshot` | #76 |
| `GET /v1/trades?chain_id=&token=&limit=` | `TradeList` | #77 |
| `GET /v1/candles?chain_id=&token=&interval=&from_ms=&to_ms=` (`interval`: `1s`, `1m`, `5m`, `15m`, `1h`, `4h` or `1d`) | `CandleSeries` | #80 |
| `POST /v1/session` | `Session` (guest, or linked to a Privy login) | #85 |
| `GET /v1/account` | `Account` | #85 |
| `GET /v1/positions` | `PositionList` | #85 |
| `GET /v1/history` | `TradeHistory` | #85 |
| `POST /v1/quotes` (`QuoteRequest`) | `Quote` | #83 |
| `POST /v1/trades` (`TradeRequest`) | `TradeStatus` | #84 |
| `GET /v1/trades/{trade_id}/receipt` | `TradeReceipt` | #84 |
| `GET /v1/orders` | `OrderList` | #86 |
| `POST /v1/orders` (`PlaceOrderRequest`) | `Order` | #86 |
| `PATCH /v1/orders/{order_id}` (`UpdateOrderRequest`) | `Order` | #86 |
| `DELETE /v1/orders/{order_id}` (`CancelOrderRequest`) | `Order` | #86 |
| `GET /v1/firings/{firing_id}/explanation`, `GET /v1/trades/{trade_id}/explanation` | `FiringExplanation` | #87 |

Account requests send the session token as `Authorization: Bearer <session_token>`. A Privy user signs the quote's `intent_typed_data` and sends the signature in `TradeRequest`; a guest's intent is signed by the server, in shadow mode only.

**WebSocket.** One connection, at `/v1/stream` on the API's host (D94); the client sends `ClientMessage` (subscribe, unsubscribe) and receives `ServerMessage` (snapshot, delta, heartbeat, error). Each subscribed topic gets one snapshot, then deltas numbered by `seq`; a gap means resubscribe. Deltas replace the record with the same key; trades append. A heartbeat comes every second, and three missed mark the data stale. The topics and their messages are in [data.md](data.md#api-topics-websocket).

**The server (#78).** `crates/api`, axum (D45), run as `api --kafka BROKERS [--listen ADDR] [--cors-origin URL] [--new-pool-window-ms MS] [--trending-min-depth-usd USD]`. It consumes `pool-updates.base`, `trades.base`, `prices.base` and `status.base` from the topics' start (offsets are never committed, so a restart rebuilds its read models) and serves, so far: `GET /health`, `GET /v1/status` (`EngineStatus`, 503 until the engine's first), `GET /v1/tokens/{chain_id}/{address}` (`TokenSnapshot`, 404 until the token has a price), `GET /v1/discovery` (`DiscoveryFeed`, filtered server side) and the `status`, `token:<address>` and `discovery` topics at `/v1/stream`. The discovery read model is in [data.md](data.md#api-topics-websocket). Other topics answer `CODE_UNKNOWN_TOPIC` until their issues (#80, #85) add them.
- **`status` (#79).** The engine's latest `status.base` record in the contract's shape: the record's block as the head, its lag, pools per venue (`known` and `active` are both every pool tracked until #41 tiers them), shadow checks, and the run's mode, core instance and recording flag. A record without the chain head (a recording from before the follower recorded it) shows a lag of 0 blocks. The fields are in observability.md.
- **Read models are pure.** Each is a function of the records applied, in order, timed by the record's block time, never the wall clock (build rule 1). A recorded session gives byte-identical API output (`crates/api/tests/replay.rs`). Only the heartbeat's `server_time_ms` reads the wall clock (`det::SystemClock`), and it never reaches a read model.
- **`token:<address>`.** The snapshot is the token's latest `TokenSnapshot`; each delta is a `TokenTick` from the next price record. A subscription to a token with no price yet gets its snapshot with the token's first tick. Pool venue, fee tier and creation block, window stats and safety are empty until the discovery (#81) and safety (#89) read models fill them.
- **Throttle (D43).** At most 20 ticks a second per token, leading edge: the first tick after a quiet 50 ms goes out at once, later ones within the gap are merged (the latest wins) and sent when it's up, so the last value always arrives. The throttle's clock is block time too, so a replay throttles as the live run did; a held tick goes out with the next record.
- **Slow clients.** Each connection reads ticks from a bounded queue (256). A client that falls further behind is sent fresh snapshots of its topics, each starting again at seq 0, instead of queueing without limit.
- **Operational basics.** CORS allows the terminal's origin (`--cors-origin`, GET only). Each request's trace span carries the running request count, and each connection's span the open connection count.

**Generated types and fixtures.** `npm run api:generate` in `web/terminal/` runs `buf generate` (protobuf-es v2) into `src/api/generated`, which is committed; `npm run api:check` fails CI's `frontend` job when it's stale. `src/mocks/fixtures/api/v1/<Message>.json` holds one canonical proto3 JSON fixture per message, for MSW handlers and stories (`apiFixture(Schema)` parses one); `src/api/contract.test.ts` checks every message has one, and that records carry lineage and amounts are decimal strings. Candle series also have one fixture per interval where the chart needs more than the message fixture (`src/mocks/fixtures/api/candles/<interval>.json`: 15m, 4h and 1d, #124); the MSW handlers serve `GET /v1/candles` and each `candles:<address>:<interval>` topic from the interval's file, falling back to the `CandleSeries` fixture.

**The terminal's client (#63, D94).** `web/terminal/src/api`: `rest.ts` (typed REST client) and `queries.ts` (TanStack Query hooks) for snapshots and commands; `stream/` (the WebSocket manager, its store and hooks) for topics. `VITE_DATA_SOURCE` picks the source: `fixtures` (the default; MSW serves the fixtures above in the browser, REST and stream alike), `replay` or `live` (both with `VITE_API_URL`). The manager subscribes only to topics on screen, resubscribes a topic on a seq gap, reconnects with backoff and resubscribes after a drop, and marks data stale after three missed heartbeats. It keeps the server's clock from each heartbeat, so ages on screen count from it rather than the browser's clock, and a replay or the fixtures read as recorded. Each streamed region shows loading, live, stale, reconnecting or unavailable; the header shows the connection as Live, Replay, Fixtures, Connecting, Stale, Reconnecting or Unavailable.

**Discover (#65).** The table subscribes to `discovery`: New and Trending views, sort and filters chosen client-side over the feed's rows, and an order that holds still while the pointer or focus is in the table (new rows wait, departed rows stay dimmed until the hold ends). Opening the token page from a row is #119; the live-API criterion and the safety badge are #118.

## Open questions (Jutin's call, noted for alignment)

1. Charting library (TradingView Advanced Charts vs Lightweight Charts vs custom).
2. Framework and state management for high-frequency feeds.
3. Mobile layout scope (Trojan ships a mobile web view).
