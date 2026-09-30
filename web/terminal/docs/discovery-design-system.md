# OmniMarket Discover Design System

**Status:** Phase 0 implementation slice. **Owner:** Jutin.

The Discover screen is a market radar. It helps a trader answer three questions
quickly: what changed, what meets my rules, and what can I inspect next? It is
not a metrics dashboard and it does not execute a trade directly.

## Reference translation

The supplied scanner reference informs composition only. OmniMarket keeps its
own charcoal surfaces, Base-blue accent, semantic status colors, typography,
copy, and original token presentation.

```text
Discovery controls
  lifecycle streams · timeframe · visibility · refresh · wallet · quick-buy draft · presets

Discovery workspace
  chain rail · aligned comparison table · row evidence · reviewed action
```

## Visual tokens

The screen reuses the global tokens from `styles.css`:

| Token | Use |
|---|---|
| `--omni-bg` | Page and terminal background |
| `--omni-surface` | Filter panel, selected stream, hover surface |
| `--omni-border` | Table and control separators |
| `--omni-border-strong` | Focused input and active control boundary |
| `--omni-text` | Token names and primary financial values |
| `--omni-text-muted` | Secondary metadata |
| `--omni-text-subtle` | Column labels and utility metadata |
| `--omni-blue` | Active stream, selected chain, quick-buy action |
| `--omni-green` | Positive movement, buys, passed evidence |
| `--omni-red` | Negative movement, sells, blocked evidence |
| `--omni-amber` | Provisional and needs-review evidence |

Spacing uses a 4px base. Controls use 6–8px radii. Static table surfaces use
1px separators instead of shadows or nested cards.

The terminal shell is viewport-locked. Header, discovery controls, chain rail,
and table footer stay in place; only the token table region owns vertical and
horizontal scrolling. This preserves the scanner workspace model from the
reference instead of making the whole page drift while rows are compared.

## Component contracts

### Stream tabs

Top, Trending, Radar, Callouts, and Streamers are route-local discovery modes.
The active tab uses a restrained surface and visible text contrast. `NEW` is a
semantic product label, not decoration.

### Timeframe controls

The timeframe changes the interpretation of row sparklines and transaction
volume. It does not change the table geometry. The selected value uses the
primary blue control state.

### Chain rail

All, Base, BNB, Solana, and Ethereum are always reachable. On mobile the rail
becomes a horizontal chip strip; it never becomes an inaccessible icon-only
column.

### Token row

Every row has the same order:

1. Pair identity: avatar, name, symbol, age, chain, confidence, tags.
2. Market cap and change.
3. Liquidity and change.
4. Volume and change.
5. Transaction total, buy/sell split, and direction.
6. Evidence: liquidity, mint status, holder count, tax/ownership state.
7. A reviewed Buy draft action.

Rows may be selected for future token-page navigation. Buy is never a broadcast;
it creates a local draft notice until quote review and signing exist.

### Safety evidence

Use text and icons with color. A row can be `passed`, `review`, or `blocked`.
Avoid a generic “safe” badge: evidence names the observed condition and keeps
provisional state visible.

### Asset strategy

Fixture rows use local raster artwork in `public/assets/tokens` and chain marks
in `public/assets/chains`. The domain contract exposes these as URLs through
`avatarSrc`, so a live market adapter can provide CDN or API image URLs later.
Local assets are intentional for Phase 0: they keep demos and visual tests
stable while the data boundary is still mocked. The token thumbnails are
illustrative fixture art, not claims about live token identity.

The desktop comparison table places the trend sparkline beside market cap, where
it acts as the visual lead for price movement. Token names, financial values, and
evidence labels use a readable density rather than cockpit-sized microcopy. The
action column keeps a wide, pill-shaped draft button so the scan ends in a clear
next step without implying a signed transaction.

## State matrix

| State | Treatment |
|---|---|
| Populated | Dense comparison table with realistic fixture values |
| Loading | Stable table region with a clear loading message |
| Empty | Explains that filters removed results and suggests broadening them |
| Error | Plain-language recovery message and retry action |
| Disconnected | Connection banner belongs in the global shell; quick-buy actions pause |
| Provisional | Amber `Provisional` label remains beside the token age/chain |
| Selected | Blue inset edge and quiet row background |
| Long content | Token name truncates safely; full value remains available to the token route |

## Responsive contract

- Desktop `≥1280px`: vertical chain rail and aligned comparison table inside a
  viewport-locked shell; the table body is the scroll region and its header is
  sticky.
- Tablet `720–1279px`: toolbar wraps, table keeps a controlled horizontal scroll,
  and column proportions remain stable.
- Mobile `<720px`: chain rail becomes horizontal, table rows become stacked
  records with visible field labels, and the Buy action becomes full-width.

## Data boundary

Fixtures live in `src/mocks/discoveryFixtures.ts` and use domain-shaped token
types from `src/domains/market/token.ts`. The UI does not assume a backend
response shape beyond those types. Streaming/query adapters can replace the
fixtures later without changing the row components.
