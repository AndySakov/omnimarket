# Issue 64 remediation plan

Issue 64 asks for the first token workspace slice: a routable token page with a dense market header, live-shaped chart, trades tape, pool provenance, and safety evidence. The current frontend remains fixture-only until the shared API/WebSocket work is available.

## Delivered in this frontend slice

- Added the public `/base/token/:address` route and browser back/forward handling.
- Expanded the header model with typed USD/quote price, market cap, liquidity, ±2% depth, 24-hour volume, transaction count, and total supply. Safety tax evidence remains in the safety panel only.
- Added pool venue/fee/address links with Basescan destinations.
- Added the contract-supported 1-second, 1-minute, 5-minute, and 1-hour chart intervals through the shared candles topic.
- Added price and Basescan transaction links to the trades tape.
- Added explicit safety evidence states (`not-checked`, passed, warning, failed) and kept the trade panel honest while Issue 89 is open.
- Added table loading, empty, and error states plus Storybook coverage for the header, tape, and safety panel.
- Preserved keyboard navigation and whole-token-card activation in Discover.

## Acceptance matrix

| Issue 64 requirement | Status | Notes |
| --- | --- | --- |
| Public token route | Ready in fixture mode | Resolves the known fixture address and shows a missing state for unknown addresses. |
| Header metrics | Ready in fixture mode | Uses deterministic values; live values await the typed market stream. |
| Candle/volume chart | Ready in fixture and live-source flows | Lightweight Charts consumes typed candle snapshots and deltas for the contract-supported intervals. |
| Live chart/tape subscription | Implemented in PR #105 | Uses Issue 63's shared stream manager and generated types; live candle backfill remains dependent on Issue 80. |
| Safety panel | Ready as explicit pending state | Live sellability/tax results await Issue 89. |
| Loading/empty/stale/error states | Ready in frontend | Stream and table surfaces expose meaningful status text. |
| Storybook stories | Ready in frontend | Header, trades tape, and safety evidence each cover their required states. |

## Next integration step

The shared client is now the only stream boundary. Keep `getTokenWorkspaceSnapshot` limited to presentation fixtures, map generated snapshots and deltas into the view model, and verify route unsubscribe/resubscribe, interval reloads, stale heartbeat, and reconnect behavior against the backend topics. Issue 64 should only be marked complete after the review gate passes and the live candle/safety dependencies are available.
