# Issue 64 remediation plan

Issue 64 asks for the first token workspace slice: a routable token page with a dense market header, live-shaped chart, trades tape, pool provenance, and safety evidence. The current frontend remains fixture-only until the shared API/WebSocket work is available.

## Delivered in this frontend slice

- Added the public `/base/token/:address` route and browser back/forward handling.
- Expanded the header model with USD and ETH price, market cap, liquidity, ±2% depth, 24-hour volume and transaction count, fees, supply, curve, and tax.
- Added pool venue/fee/address links with Basescan destinations.
- Added deterministic 1-second chart interval support and preserved the existing candle/volume fixture boundary.
- Added price and Basescan transaction links to the trades tape.
- Added explicit safety evidence states (`not-checked`, passed, warning, failed) and kept the trade panel honest while Issue 89 is open.
- Added table loading, empty, and error states plus Storybook coverage for the header, tape, and safety panel.
- Preserved keyboard navigation and whole-token-card activation in Discover.

## Acceptance matrix

| Issue 64 requirement | Status | Notes |
| --- | --- | --- |
| Public token route | Ready in fixture mode | Resolves the known fixture address and shows a missing state for unknown addresses. |
| Header metrics | Ready in fixture mode | Uses deterministic values; live values await the typed market stream. |
| Candle/volume chart | Ready in fixture mode | Lightweight Charts with 1s, 1m, 5m, 15m, 1h, 4h, and 1D controls. |
| Live chart/tape subscription | Blocked | Requires Issue 63's shared stream manager and Issues 76/77/80's backend topics. |
| Safety panel | Ready as explicit pending state | Live sellability/tax results await Issue 89. |
| Loading/empty/stale/error states | Ready in frontend | Stream and table surfaces expose meaningful status text. |
| Storybook stories | Ready in frontend | Header, trades tape, and safety evidence each cover their required states. |

## Next integration step

When Issue 63 lands, replace `getTokenWorkspaceSnapshot` and `createTokenWorkspaceStream` with the typed REST/WebSocket adapters. Keep the component contracts unchanged, map snapshot plus deltas into `TokenMarketSnapshot`, and verify route unsubscribe/resubscribe, interval reloads, stale heartbeat, and reconnect behavior against the backend topics. Issue 64 should only be marked complete after that integration and the live acceptance checks pass.
