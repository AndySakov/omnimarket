# Frontend Implementation Plan

**Status:** Frontend working plan. Owner: Jutin. Phase 0 foundation is
implemented and verified on `codex/terminal-foundation`; Phase 1 Token Workspace
is implemented as a fixture-first design-system slice on `codex/token-workspace`.

This is the implementation plan for the full OmniMarket terminal. It uses the
repository product contract in `docs/spec/frontend.md` as upstream context. A
frontend plan does not alter that contract; raise any proposed shared change
with the project collaborator before editing repository-level specifications.

## Product goal

The terminal gets an active trader from discovery to a protected position
quickly and safely:

```text
Discover token → assess safety and liquidity → inspect chart → get quote
→ sign trade → set exits → monitor position
```

Familiar patterns from Trojan, Axiom, Photon and GMGN reduce switching cost.
OmniMarket differentiates with evidence: safety proof, route and execution
detail, trade receipts, per-step timing, and an explanation of every trigger
firing.

## Stack

| Concern | Choice |
|---|---|
| App | React, TypeScript and Vite |
| Routing | React Router |
| Styling | Tailwind CSS with CSS-variable design tokens |
| Accessible primitives | shadcn/ui and Radix primitives |
| REST/query state | TanStack Query |
| Streaming state | Zustand stores using `useSyncExternalStore` |
| Forms | React Hook Form and Zod |
| Charts | TradingView Lightweight Charts, with OmniMarket order overlays |
| Exact financial values | A fixed-decimal domain type or `decimal.js`; never JavaScript `number` |
| API types | TypeScript generated from the repository Protobuf schemas |
| Wallet/authentication | Privy React SDK |
| Component development | Storybook |
| Tests | Vitest, Testing Library and Playwright |

Vite is selected because the terminal is a client-first, authenticated,
high-frequency application whose API belongs to the backend. A future public
marketing site may use a separate content-oriented stack.

## Code structure

```text
web/terminal/
├── src/
│   ├── app/          # router, providers, layouts and app composition
│   ├── pages/        # route-level screens
│   ├── features/     # user capabilities: quote-trade, discovery-feed, orders
│   ├── domains/      # market, token, quote, order, position, wallet, execution
│   ├── api/          # generated types, REST client, WebSocket client, mappers
│   ├── shared/       # design-system UI, hooks, formatters, utilities and types
│   ├── mocks/        # mock-server adapters and deterministic fixtures
│   ├── styles/       # tokens and global styles
│   └── tests/
├── .storybook/
└── public/
```

Dependencies flow in one direction:

```text
shared → domains → features → pages → app
```

Features may depend on domains, but domains must not depend on feature or page
code. `shared/ui` contains reusable primitives only; token, order and trade
components belong in their relevant domains or features.

## State and API model

Keep three state classes separate:

| State | Examples | Owner |
|---|---|---|
| Query snapshot | token metadata, positions, settings | TanStack Query |
| Streaming data | ticks, trades, order/execution status | WebSocket manager plus normalised Zustand stores |
| Local UI state | selected wallet, panel visibility, chart interval, order draft | component state or a small UI store |

The WebSocket manager fetches an initial snapshot, subscribes only to visible
data, merges deltas into normalised stores, batches display updates when needed,
and reconnects/resubscribes after a dropped connection. The UI must visibly
represent provisional, confirmed, corrected, stale, reconnecting and unavailable
data. Market ticks must not cause an application-wide rerender.

## Design system

The terminal is dark-first, restrained and data-dense. It uses semantic tokens
for surfaces, borders, text, focus, positive/negative movement, warnings and
provisional state. Colour is never the sole carrier of meaning.

Build primitives first: buttons, icon buttons, inputs, amount inputs, token
selectors, selects, tabs, badges, tooltips, toasts, dialogs, sheets, tables,
skeletons and status indicators. Then build domain components such as token
rows, trade panels, route breakdowns, safety-evidence panels, chart order lines,
position rows, execution timelines, trade receipts and a “Why did this fire?”
drawer.

Desktop token trading uses a discovery area, a centre chart/detail workspace and
a persistent trade panel. The Token Workspace extends this with a dense metric
header, reference-style market tabs and a Lightweight Charts candle/volume
surface. Mobile is a deliberately stacked flow with natural page scrolling and
the trade panel below the market content, never a compressed three-column desktop
layout. See [token-workspace-design-system.md](token-workspace-design-system.md)
for the surface-level tokens and layout contracts.

Every substantial screen implements populated, loading, empty, error, disabled,
stale/reconnecting, keyboard-focus and overflow states from the start.

## Design reference workflow

Before a feature is built, audit the matching flow in the agreed reference
terminals at consistent desktop and mobile sizes. Record the recognisable
pattern, the information visible before action, the advanced detail hidden after
action, and friction worth avoiding. Build low-fidelity wireframes first, then
high-fidelity Figma screens and component states, then the same primitives in
Storybook. References guide familiar interaction patterns; they do not justify
copying branding, assets, code or copy.

## Delivery sequence

### Phase 0 — frontend foundation

- Create `web/terminal` and the development, linting, testing and Storybook setup. **Complete.**
- Establish the frontend verification lane, design tokens, terminal shell and
  foundational components. **Complete.**
- Complete the reference audit and wireframes for discovery, token detail and
  trade flows. The approved result is in
  [frontend-reference-layout-brief.md](frontend-reference-layout-brief.md).
- Build a mocked terminal shell using realistic data and failure states.
  **Complete for the global header and Discover surface.**

The remaining Phase 0 handoff is operational rather than a new UI feature:
commit the verified foundation, push `codex/terminal-foundation` to the personal
fork, and open the collaborator review PR. Do not start live API or wallet
integration from this branch.

### Phase 1 — M2: discovery and token detail

- Discovery feed and responsive token table. **Complete as a deterministic
  mocked surface.**
- Token Workspace: dense token header/summary, candle/volume display, context
  rail, reference-style market tabs and the persistent mocked trade-panel shell.
- Connection status and WebSocket reconnection behaviour.
- Build against the backend mock server.

The Token Workspace design gate and implementation notes are in
[token-workspace-brief.md](token-workspace-brief.md), with the design-system
contract in [token-workspace-design-system.md](token-workspace-design-system.md).
The connection-aware fixture adapter and its replacement plan are documented in
[token-workspace-stream-plan.md](token-workspace-stream-plan.md).
Live generated API types,
mock-server integration and WebSocket behaviour begin when the backend reaches
M2.

### Phase 2 — M4/M5: trading

- Buy/sell panel, presets, quote, price impact, fees, intent signing and
  pending/success/failure execution states.
- Trade receipt and position handoff.

### Phase 3 — M6/M7: automation and safety

- Chart-native order controls, TP/SL/trailing-stop forms, auto-sell settings,
  order timeline, safety evidence and trigger explanation.

### Phase 4 — M8 onward: product breadth

- BNB execution context, wallets, cross-chain portfolio, copy trading,
  execution-quality reporting and public system/brake status.

### Phase 5 — hardening and launch

- Performance profiling under tick volume, keyboard workflows, mobile usability,
  accessibility review, visual regression coverage, execution-flow end-to-end
  tests and signing/security review.

## Definition of done

A frontend feature is complete when its primary trader action is obvious,
financial values are exact, interactions are keyboard accessible, all material
data states are represented, desktop and mobile behaviour is intentional, and
the typed API contract, tests and relevant specs are updated.

## Current checkpoint

The foundation currently includes the global navigation shell, OmniMarket
identity assets, the Discover market table, deterministic token fixtures,
responsive table-owned scrolling, loading/empty/error states, accessibility
coverage and visual baselines. Typecheck, lint, unit tests, production build,
Playwright flows, visual checks and the Discover accessibility audit have passed.

The foundation checkpoint is **PR-ready** and is under collaborator review in
PR #56. The current follow-up checkpoint is a mocked Token Workspace; quote
signing, live market data and backend integration remain explicitly out of
scope for that slice.
