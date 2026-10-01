# Token Workspace Brief

**Status:** Fixture-ready remediation slice implemented locally on `codex/token-workspace`; live acceptance remains blocked by Issues 63, 76, 77, 80 and 89.
Owner: Jutin. This is the Phase 1 frontend slice after the foundation PR.

The audit-to-implementation record is in
[issue-64-remediation-plan.md](issue-64-remediation-plan.md).

The surface-level design contract is documented in
[token-workspace-design-system.md](token-workspace-design-system.md).

## Product goal

Help a trader answer three questions without leaving the terminal:

1. Is this token worth inspecting?
2. What evidence supports the current market and safety view?
3. What would a trade draft look like before signing?

The page prepares a reviewable trade draft only. It never signs, broadcasts or
claims that a token is safe based on fixture data.

## Information architecture

```text
TokenPage
  TokenWorkspaceHeader
    Back to Discover · identity · chain/address · freshness · dense market summary
  TokenWorkspace
    ContextRail
      Watchlist context · Recent discovery
    DecisionWorkspace
      TokenChartPanel
        Chart interval controls · candles/volume · price trend · freshness note
      TokenMarketTabs
        Trades · Positions · Orders · Holders · Top Traders · Dev Token
    TradePanel
      Buy/Sell/Auto · Market/Limit/DCA/Advanced · amount input · presets ·
      quote breakdown · safety state · review state
```

Desktop uses the approved three-region layout: a compact context rail, a
chart/detail centre and a persistent trade panel. Mobile stacks the same
regions in decision order and keeps the trade panel reachable below the detail
content; it does not compress the desktop columns.

## Fixture and API boundary

`src/mocks/tokenWorkspaceFixtures.ts` mirrors the intended domain shape without
pretending to be the generated backend contract. It contains token identity,
price points, candle/volume data, market rows, pool context and safety evidence.
The UI labels the chart and trade panel as fixture/mock data until M2 supplies
generated API types and the backend mock server.

The next integration review must confirm:

- token identity and address representation;
- quote freshness and expiry fields;
- safety evidence confidence and correction states;
- activity, holder and pool pagination;
- WebSocket snapshot/delta ownership.

## State matrix

| Surface | Implemented first | Deferred to integration |
|---|---|---|
| Token page | ready, loading, missing, stale, error, provisional banner | corrected stream, unavailable API |
| Chart | deterministic Lightweight Charts candles/volume, 1s–1D interval tabs, delayed/paused label | live candles, order overlays |
| Details | Trades, Positions, Orders, Holders, Top Traders and Dev Token tabs | paginated API data and reconnect states |
| Trade panel | Buy/Sell/Auto, modes, amount validation, presets, quote summary, loading/error states and review-ready state | wallet connection, signing, submitted/pending/landed/failed |
| Safety | explicit not-checked/passed/warning/failed evidence with explanation | backend simulation receipt and confidence history |

## Acceptance criteria

- A Discover token opens its Token Workspace without changing the shell route.
- The header shows name, symbol, chain, address, freshness, price, liquidity,
  volume, fees, supply, curve and tax with tabular-number treatment.
- Desktop preserves the context rail / centre chart / trade panel proportions.
- Mobile stacks content and keeps the trade panel usable without horizontal
  document overflow or overlapping chart content.
- Market tabs expose Trades, Positions, Orders, Holders, Top Traders and Dev Token
  surfaces with dense terminal-style tables.
- Trade review requires a positive amount and clearly states that signing is
  disabled in this mocked slice; fixture quote loading and errors remain visible.
- Keyboard focus states and serious/critical Axe checks pass at mobile size.
- Typecheck, lint, unit, production build and focused Playwright flows pass.

## Out of scope

Do not add wallet/signing behaviour, live market data, generated API clients,
WebSocket subscriptions, real quote math, transaction submission or automated
exits in this slice. Those belong to the M2/M4/M5 integration and trading
milestones.
