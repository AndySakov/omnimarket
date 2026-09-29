# Phase 0: Reference Audit and Layout Brief

**Status:** Approved foundation deliverable. Owner: Jutin.

This brief defines the first terminal experience to design and implement:
discovery, token evaluation and manual trade. It is an interaction and layout
reference, not a visual copy of another product.

## Outcome

```text
Discover token → assess safety and liquidity → inspect chart → get quote
→ sign trade → see execution progress and resulting position
```

The first design and code scope ends at a mocked manual trade. Automated orders,
portfolio, wallet management and copy trading are later phases.

## Design principles

1. **Actionable data wins.** Show facts needed for the next decision; hide
   secondary inspection behind a tab or drawer.
2. **Speed must be visible.** Show quote freshness and execution progress; never
   make a delayed result look complete.
3. **Risk is evidence, not a badge.** Every safety state links to its cause.
4. **Market data has confidence.** Label provisional, confirmed, stale,
   reconnecting and unavailable data.
5. **Familiarity without imitation.** Reuse proven interaction patterns, never
   another product's branding, assets, code or copy.

## Reference audit

| Reference | Observed pattern | Adopt | Improve for OmniMarket |
|---|---|---|---|
| [Axiom Pulse](https://docs.axiom.trade/axiom/finding-tokens/pulse) | Three lifecycle streams—New Creations, Final Stretch, Migrated—with filters and quick buy | Three independently filterable discovery streams | Make each quick-trade path open a reviewed quote and preserve chain-specific safety context |
| [Axiom Market](https://docs.axiom.trade/axiom/swap/market) | Search leads into a compact trade flow with fee, slippage and MEV settings | Search by ticker or contract address; put trade settings near the action | Explain situational defaults rather than making the trader infer their effects |
| [Trojan Trade Page](https://docs.trojan.com/trading-on-trojan/trade-page) | Central chart, activity/order overlays, audit and trade tables | Chart as the centre of the token workspace | Keep first-release chart controls focused; defer advanced tools |
| [Trojan Limit Orders](https://docs.trojan.com/trading-on-trojan/trojan-swaps/limit-orders) | Form trigger edits move the corresponding chart line; order sizing uses presets | Later: bidirectional order form and chart-line behaviour | Use clear labels for trigger, display and execution prices |
| [GMGN Token Workspace](https://docs.gmgn.ai/index/token-page-chart-multicharts-activity-trading-system) | Left context lists, centre chart/activity analysis, persistent right trading module | Three-region desktop token workspace | Use tabs/drawers to avoid putting every data category on the default screen |

Refresh this audit from live reference screens before visual sign-off. Official
documentation supplies the interaction evidence; live products are the visual
reference if documentation screenshots are old.

## Information architecture

### Global shell

| Region | Purpose |
|---|---|
| Utility bar | Chain context, token/contract search, connection state, wallet menu |
| Primary navigation | Discover, Portfolio, Trackers, Wallets, Settings |
| Workspace | Route-specific discovery or token decision surface |
| Trade panel | Quote, review and execution action on token routes |

Search accepts a ticker or verified contract address. Ambiguous ticker results
always show chain and address before navigation.

### Discovery layout

The discovery screen answers: **what changed, what meets my rules, and what can
I inspect now?** It is a market radar, not a metrics dashboard.

```text
Desktop ≥ 1280px
┌──────────────────────────────── utility bar ──────────────────────────────┐
│ OmniMarket  Discover  Portfolio  Trackers      [Base ▾] [Search] [Wallet] │
├──────────────┬────────────────────────────────────────────────────────────┤
│ Filters      │ New                     Final stretch        Migrated       │
│ Chain: Base  │ 3m · liquidity · safety  curve progress       newest first  │
│ Age          │ ┌────────────────────┐  ┌──────────────────┐ ┌────────────┐│
│ Liquidity    │ │ token row           │  │ token row        │ │ token row  ││
│ Safety       │ │ price / Liq / Vol   │  │ progress / Liq   │ │ price / age││
│ Holders      │ │ safety · view       │  │ safety · view    │ │ safety/view││
│ [Reset]      │ └────────────────────┘  └──────────────────┘ └────────────┘│
└──────────────┴────────────────────────────────────────────────────────────┘
```

Each stream owns its filters, with a visible filter summary. A row navigates to
the token page. A secondary quick-trade control is allowed only after sufficient
quote and safety data exists; it opens a reviewed trade panel, not a broadcast.

A token row shows: name, symbol, chain, age, price/market-cap movement when
reliable, liquidity, volume, transaction momentum, lifecycle status, compact
safety evidence and an explicit stale/provisional state.

### Token workspace layout

The token screen answers: **is this worth trading, what would it cost, and can
I act safely now?**

```text
Desktop ≥ 1280px
┌───────────────────────────────────────────────────────────────────────────┐
│ Utility bar                                                               │
├───────────────┬─────────────────────────────────────┬─────────────────────┤
│ Watch /       │ Token header                        │ Trade panel         │
│ discovery     │ $TOKEN · Base · verified address    │ [Buy] [Sell]        │
│ context       │ Price · 1h change · Liq · FDV        │ Pay / Receive       │
│               ├─────────────────────────────────────┤ Presets             │
│ Recent tokens │ Chart toolbar: 1m 5m 1h drawing…    │ Quote breakdown     │
│ Watchlist     │                                     │ Impact / fees       │
│               │             Candle chart            │ Slippage / tip      │
│               │    position and order overlays      │ [Review trade]      │
│               ├─────────────────────────────────────┤ Freshness / status  │
│               │ Activity | Safety | Holders | Pools │ Execution timeline  │
└───────────────┴─────────────────────────────────────┴─────────────────────┘
```

The centre is the market decision surface. The right panel is stable while
streaming activity updates, so the primary action never shifts.

### Mobile layout

Mobile is a task-oriented flow, not a squeezed desktop grid.

```text
Discovery                         Token detail
┌────────────────────────┐       ┌────────────────────────┐
│ [Base ▾] [Search]       │       │ ‹ Discover   $TOKEN    │
│ New | Stretch | Migrated│       │ Price · Liq · safety   │
│ [Filters]               │       │ ┌────────────────────┐ │
│ token row               │       │ │ Chart               │ │
│ token row               │       │ └────────────────────┘ │
└────────────────────────┘       │ Activity Safety Pools  │
                                 │ [Buy]       [Sell]     │
                                 └────────────────────────┘
                                      ↓ opens a sheet
                                  ┌────────────────────────┐
                                  │ amount / quote / review │
                                  │ [Sign and submit]       │
                                  └────────────────────────┘
```

The trade sheet preserves the draft if a quote expires or signing fails. The
token page retains all metadata below the chart rather than silently omitting it.

## Interaction contracts

### Trade

```text
Enter amount → request quote → show route, impact, fees and expiry
→ review intent → Privy signature → submitted → pending → landed or failed
```

- Final submission is disabled until a valid, unexpired quote and funding wallet
  are selected.
- Quote loading reserves its layout; it never moves the action button.
- The review names input, minimum output, fees, deadline and chain.
- A duplicate click cannot cause a duplicate submission.
- Failure retains entered values and proposes the right recovery: retry, refresh
  quote, reconnect wallet or inspect the receipt.

### Data confidence

| State | UI treatment | Trade effect |
|---|---|---|
| Confirmed | Normal timestamp and state label | Eligible |
| Provisional | “Provisional” label and explanation | Only eligible if backend permits |
| Stale | Age indicator and refresh hint | Quotes not actionable |
| Reconnecting | Compact connection banner | New trade requests pause |
| Corrected | Correction note in activity/price | Reconcile visibly; do not hide it |
| Unavailable | Plain-language error and retry | Trade disabled |

### Safety evidence

The compact state says **Sell simulation passed**, **Needs review**, or
**Trading unavailable**. Its detail view contains the simulated sell outcome,
observed tax, owner-power signals, liquidity status and timestamp. It never
claims a token is simply “safe.”

## Component tree

```text
TerminalApp
  AppShell
    UtilityBar
      ChainSelector · TokenSearch · ConnectionStatus · WalletMenu
    PrimaryNavigation
    DiscoveryPage
      DiscoveryToolbar
        DiscoveryFilters · SavedFilterMenu
      DiscoveryStreamGrid
        DiscoveryStream (New | Final stretch | Migrated)
          TokenRow
    TokenPage
      TokenWorkspaceHeader
      ContextRail
        Watchlist · RecentDiscoveryList
      TokenDecisionWorkspace
        TokenSummary · ChartToolbar · MarketChart
        TokenDetailTabs
          ActivityFeed · SafetyEvidencePanel · HoldersTable · PoolsTable
      TradePanel
        TradeSideTabs · AmountInput · TradePresets · QuoteBreakdown
        TradeReviewDialog · ExecutionStatusTimeline
```

## Initial design system

| Token family | Starting rule |
|---|---|
| Surfaces | Near-black neutral background, one muted elevated surface and 1px separators rather than heavy cards |
| Type | Strong primary text, muted metadata and tabular/monospace numerals for prices and quantities |
| Status | Positive, negative, warning, info and provisional tokens paired with text/icon labels |
| Spacing | 4px base: 8/12px compact controls, 16px panels, 24px workspace groups |
| Radius and shadow | 6–8px controls; no shadow for static panels, limited shadow for menus and sheets |
| Density | Comfortable by default; compact mode comes later only if research supports it |

## Required states

| Surface | States to design first |
|---|---|
| Discovery | loading, empty due to filters, disconnected, error/retry, long token/address, provisional row |
| Token | loading shell, missing token, stale stream, corrected data, low-liquidity warning |
| Trade | disconnected wallet, invalid amount, quote loading/expired, signing, submitted, pending, landed, failed |
| Safety | loading, no evidence yet, passed with evidence, warning with evidence, trading blocked |

## Critical self-review

| Risk | Correction |
|---|---|
| Competitor imitation | Keep only interaction principles; build OmniMarket branding and components from scratch |
| Unreadable density | Reserve the default view for decision data; place detail in tabs and drawers |
| Unsafe speed | Require fresh quote and an intelligible review before signing |
| Jumpy streaming UI | Keep trade layout stable, batch updates and label confidence state |
| Compressed mobile desktop | Use dedicated tabs, sheets and a persistent labelled action area |

## Acceptance criteria and next task

The foundation is ready when the desktop/mobile layouts, reference patterns,
component names, essential data, failure states and mock-contract needs are
agreed. The next implementation task is to create `web/terminal` with the
approved stack, Storybook, design tokens, primitives and deterministic fixtures.
Do not build a live discovery feed until generated API types and the mock
contract exist.
