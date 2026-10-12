# Token Workspace Design System

**Surface:** OmniMarket Token Workspace  
**Mode:** Operate  
**Status:** Implemented fixture-first design contract  
**Owner:** Jutin

This document is the source of truth for the Phase 1 token workspace surface.
The attached terminal screenshots are structural references only. They inform
composition, density, and familiar trading patterns; OmniMarket keeps its own
palette, typography, labels, data boundary, and branding.

## Product intent

The workspace helps a trader make a fast, defensible decision without leaving
the terminal: identify the token, inspect price and liquidity, compare recent
market activity, review safety evidence, and prepare a trade draft. Fixture data
must be clearly labelled and never imply that a wallet action has occurred.

## Visual tokens

Use the global OmniMarket variables in `src/styles.css` as the only palette
source:

| Role | Token |
|---|---|
| Canvas | `--omni-bg` |
| Surface | `--omni-surface` |
| Raised/hover surface | `--omni-surface-raised`, `--omni-surface-hover` |
| Dividers | `--omni-border`, `--omni-border-strong` |
| Primary text | `--omni-text` |
| Secondary/subtle text | `--omni-text-muted`, `--omni-text-subtle` |
| Base-blue action/focus | `--omni-blue`, `--omni-blue-strong`, `--omni-blue-soft` |
| Positive movement | `--omni-green` |
| Negative movement | `--omni-red` |
| Warning/provisional | `--omni-amber` |

No gradients or decorative shadows are used. Surfaces are separated by one-pixel
rules and spacing rather than card stacks. Rounded corners are reserved for
controls and small grouped surfaces, generally 5–9px.

## Typography and density

- Inter is used for labels, headings, and actions.
- JetBrains Mono/SFMono fallback is used for prices, addresses, counts, chart
  values, and trade inputs.
- Token names and the current price are the strongest hierarchy.
- Metadata is never smaller than 9px in the terminal context; primary controls
  remain at least 11px.
- Numeric columns use tabular, monospace treatment so rows align while values
  update.
- Dense rows use 35–45px vertical rhythm; primary sections use 12–16px insets.

## Composition contract

```text
TokenPage
  TokenWorkspaceHeader
    identity + chain/address + freshness + dense market metrics
  TokenWorkspace
    ContextRail
      watchlist + recent discovery + signals
    DecisionWorkspace
      TokenChartPanel
        interval/chart toolbar + candle/volume chart + freshness footer
      TokenMarketTabs
        Trades · Positions · Orders · Holders · Top Traders · Dev Token
    TradePanel
      preset + wallet row + Buy/Sell/Auto + mode tabs
      amount + presets + quote + safety + review state
```

Desktop proportions use a 208px context rail, a flexible center, and a 306px
trade panel. Between 881px and 1250px those fixed regions compress to 184px and
284px. The center remains the primary visual surface.

## Interaction and state rules

Every interactive element is a native button, input, checkbox, tab, or table
control with an accessible name and visible `:focus-visible` treatment.

Required states:

- Workspace: ready, loading, missing, stale, and provisional/low-liquidity.
- Chart: delayed fixture stream, interval selection, and utility controls.
- Market tabs: populated table, hover row, horizontal overflow for comparison,
  and fixture-live indicator.
- Trade: idle, invalid amount, quote loading, quote fresh, quote error/expired,
  review-ready, and signing-disabled.
- Safety and movement meaning is always paired with text; color is never the
  only signal.

Motion is limited to small color/background transitions. No chart animation,
decorative entrance choreography, or movement is used for frequent keyboard or
tab actions. Reduced-motion users receive the same information without motion.

## Responsive contract

- Desktop: three columns; the trade panel stays aligned with the workspace and
  may remain sticky without obscuring the center content.
- Tablet: the same three regions with reduced fixed widths and wrapping controls.
- Mobile: one natural document flow. Recent tokens become a horizontal strip,
  chart controls wrap, market tabs remain horizontally scrollable, and the trade
  panel follows the market content in order.
- The token page owns its document scroll; no nested primary chart/table scroll
  container is introduced. Dense tables use intentional horizontal overflow.
- No supported viewport may create horizontal document overflow or clip the
  primary trade action.

## Data and component boundary

The UI consumes `TokenWorkspaceFixture` and its typed chart/market rows from
`src/mocks/tokenWorkspaceFixtures.ts`. The domain types in
`src/domains/market/tokenWorkspace.ts` define the future adapter boundary:

- `TokenMarketTab`
- `TokenCandle`, `TokenVolumePoint`, `TokenChartData`
- `TradePanelMode`, `TradeQuoteState`, `TradeExecutionState`

The Lightweight Charts adapter stays inside `TokenChartPanel`; no chart-library
types leak into route or backend code. Wallet connection, signing, broadcasting,
real quote calculation, streaming, and generated API clients remain out of scope.

## Review checklist

Before merging changes to this surface, confirm:

- Reference topology is recognizable without copied branding or assets.
- OmniMarket tokens remain the only color source.
- Header metrics, chart, tables, and trade panel align to the same edges.
- All states are understandable without relying on color alone.
- Keyboard focus, labels, table headers, and mobile tap targets are present.
- Typecheck, lint, unit, build, Playwright, visual, and Axe checks pass.
