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

New and Trending are the discovery modes (#65). New lists pools by creation;
Trending ranks by 5m volume and txns above a liquidity floor (#81). The active
tab uses a restrained surface and visible text contrast.

### Sort and filters

Sort by feed order, age, volume, txns, liquidity or market cap; the direction
toggles. Ties fall back to the newer pool, then the address, so the order is
stable. Filters: minimum liquidity, maximum age, and "safety check passed". A
token that hasn't been checked never counts as passed.

### Live rows

The order holds still while the pointer or keyboard focus is in the table: no row
jumps under it. New rows wait (the footer counts them), and rows that leave the
feed stay in place, dimmed, until the hold ends. A new row gets a brief
highlight, and a price flashes up or down when it moves; both honour reduced
motion. Ages count from the server's clock (the latest heartbeat).

### Timeframe controls

The timeframe changes the interpretation of row sparklines and transaction
volume. It does not change the table geometry. The selected value uses the
primary blue control state.

### Chain rail

Base is live. BNB and MegaETH show as coming: `aria-disabled`, with the reason
as their description. On mobile the rail
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
| Stale | Banner says the rows may be out of date; rows dim and `DataStatus` reads Stale; rows stay until heartbeats return |
| Inserted | New row highlights briefly; held rows wait, counted in the footer |
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

The table reads the `discovery` topic through the stream client (#63), and
`liveRows.ts` maps each `DiscoveryRow` to the domain `DiscoveryToken` in
`src/domains/market/token.ts`, so the row components don't see the wire shape.
In fixture mode `src/mocks/api/discoveryFeed.ts` serves a six-row feed that
moves prices every second and adds a new pool every 6s. Stories for each state
are in `DiscoveryPage.stories.tsx`.

For `VITE_DATA_SOURCE=live`, the same generated stream hooks consume the API's
`discovery` snapshot and `discoveryRow` deltas; no fixture client or alternate
row contract is used. The live-source Playwright coverage in
`tests/e2e/live/discovery.spec.ts` proves a Base pool can arrive during the
session without a reload and that its generated safety verdict replaces the
initial `Not checked yet` state. Until backend #89 supplies evidence, an absent
verdict must remain `Not checked yet` and must never be presented as passed.
