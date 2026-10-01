# OmniMarket Global Header Design System

**Status:** Phase 0 implementation slice. **Owner:** Jutin.

This document defines the global terminal header before discovery and trading
surfaces are added. The attached Axiom screenshot is the composition reference;
OmniMarket owns the identity, colors, copy, and components.

## Product job

The header gives an active trader reliable context before any market decision:
where they are, which chain is selected, whether the connection is live, how to
search for a token, and where to manage funds.

## Anatomy

```text
Primary bar (72px desktop)
  Brand → Primary navigation → Search → Chain → Connection → Deposit → Favorites → Wallet

Utility rail (38px desktop)
  Terminal tools · How it works          Engine status · market tickers · gas
```

The primary bar is always one visual line on desktop. The utility rail is a
secondary context surface: it must not compete with route navigation or the
deposit action.

## Tokens

| Token | Value | Use |
|---|---|---|
| `--omni-bg` | `#080a0f` | App and primary header background |
| `--omni-surface` | `#0d1118` | Menus and mobile navigation |
| `--omni-surface-raised` | `#111721` | Future overlays and focused surfaces |
| `--omni-surface-hover` | `#161d28` | Hover and selected control background |
| `--omni-border` | `#202632` | Standard separators |
| `--omni-border-strong` | `#2c3544` | Input and control boundaries |
| `--omni-text` | `#f4f6fb` | Primary labels and values |
| `--omni-text-muted` | `#8d98aa` | Secondary labels and inactive routes |
| `--omni-text-subtle` | `#667286` | Utility metadata |
| `--omni-blue` | `#6d7cff` | Active route, focus, selected chain, primary action |
| `--omni-green` | `#43d69a` | Connected/live/positive state |
| `--omni-red` | `#ff6678` | Negative market movement or failure |
| `--omni-amber` | `#f4bd5d` | Warning/provisional state |

## Component contracts

### Brand

- Original OmniMarket mark and wordmark only.
- Mark is a semantic navigation button back to Discover.
- It is never used as a decorative logo inside data rows.

### Primary navigation

- Routes: Discover, Portfolio, Trackers, Wallets, Settings.
- Active route uses blue text and a 2px bottom indicator.
- Inactive routes use muted text and a quiet hover surface.
- `aria-current="page"` identifies the active route.

### Search control

- Accepts a ticker or verified contract address.
- Search is an input with a visible placeholder and keyboard shortcut hint.
- On smaller screens it becomes a toggleable full-width control below the bar.
- Search must preserve the surrounding layout while loading or showing results.

### Chain context

- Current mock chain is Base.
- The chain indicator is a small outlined mark plus text, never color alone.
- Future chain changes must retain the same control width to avoid header shift.

### Connection and wallet

- Connection status is the market-data connection (#63, D94), text plus a
  semantic dot: `Live` (green), `Replay` or `Fixtures` (blue), `Connecting`,
  `Stale` or `Reconnecting` (amber), `Unavailable` (red). Live or Replay comes
  from the engine's own mode; fixture builds always say `Fixtures`.
- Wallet address and balance use tabular/monospace numerals.
- Deposit is the primary action; wallet management is a separate control.

### Utility rail

- Utility buttons require accessible labels and visible focus.
- **Engine status** (#71) comes from the engine's `status` topic, once per
  block: Base, the head block, lag to head, pools tracked, shadow checks agreed
  out of those run (D21), and the engine's mode as text (`Live`, `Replay`, or
  `Fixtures` in fixture builds, like the connection status). Its tooltip, also
  reachable by keyboard focus, adds core instance, recording and uptime. It
  goes stale (amber `Stale`, numbers dimmed and struck through) when heartbeats
  stop, and also when the head hasn't moved for 10 seconds (five Base blocks)
  while heartbeats continue.
- **How it works** (#71) opens a dialog from the rail, on every route and at
  every width: what is live and what is shadow, the data path (chain engine →
  Kafka → API → terminal), the engine's status now, and links to the README,
  the decision log and #52. Its copy describes the backend, so it changes with
  the backend.
- Market tickers remain secondary: they give way below 1320px so the engine
  status isn't squeezed, and the whole ticker area hides at tablet widths.
- The rail is allowed to be absent in compact mobile mode.

## State coverage

| State | Header behavior |
|---|---|
| Live / Replay / Fixtures | Green or blue status naming the data source; wallet menu enabled |
| Stale | Amber status: heartbeats stopped, numbers on screen may be old |
| Reconnecting | Amber status, new trade actions pause elsewhere |
| Unavailable | Red status with recovery message; wallet action disabled |
| Search open | Search control expands without moving the primary route hierarchy |
| Mobile menu open | Routes move into an explicit, keyboard-reachable menu |
| Keyboard focus | 2px blue focus ring with 2px offset |
| Reduced motion | Transitions collapse to near-zero duration |

## Responsive contract

- Desktop `>1024px`: one-line primary navigation and visible utility rail.
- Tablet `640–1024px`: route navigation becomes a menu, search becomes a
  toggleable control, and ticker context hides.
- Mobile `<640px`: compact brand, chain mark, deposit, wallet avatar, menu, and
  utility tools remain reachable; nonessential wallet detail is hidden rather
  than squeezed.

## Reference translation

The screenshot informs hierarchy and proportions only. OmniMarket does not copy
Axiom's logo, names, token assets, exact copy, or brand colors. The next slice
will place the approved discovery workspace beneath this shell and reuse these
tokens for filters, stream columns, and token rows.
