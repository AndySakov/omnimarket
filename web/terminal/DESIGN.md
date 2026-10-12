# OmniMarket Terminal Design System

This is the canonical visual contract for the frontend terminal. Surface-specific
contracts live in `docs/header-design-system.md`, `docs/discovery-design-system.md`,
and `docs/token-workspace-design-system.md`; those documents may refine layout and
interaction behavior but must use the typography roles defined here.

## Visual direction

OmniMarket is an operate-mode trading terminal: dark charcoal surfaces, restrained
Base-blue action states, semantic green/red/amber evidence, tabular financial data,
and deliberate density. Familiar terminal topology is allowed; competitor logos,
copy, assets, and branding are not.

## Typography

Inter Variable is the UI face for navigation, headings, labels, controls, and prose.
JetBrains Mono Variable is the data face for prices, addresses, counts, timestamps,
chart values, and trade inputs. Both are self-hosted through the installed
`@fontsource-variable` packages and imported from `src/main.tsx`.

The type scale is semantic rather than a collection of one-off pixel values:

| Role | Size | Use |
|---|---:|---|
| Display | 22px | Workspace-level emphasis and major identity values |
| Title | 20px | Brand, token identity, primary headings |
| Navigation | 15px | Primary routes and stream headings |
| Body | 14px | Normal UI copy and readable controls |
| Control | 13px | Search, buttons, segmented controls, table values |
| Data | 13px | Numeric movement and market values |
| Metadata | 12px | Symbols, secondary labels, timestamps |
| Caption | 11px | Evidence labels, trade-panel copy, utility context |
| Badge | 10px | Compact badges and nonessential markers only |

Visible primary, body, and decision-support text must not be reduced below the
metadata role. Nine-pixel text is not part of the system. Ten-pixel text is reserved
for compact badges or utility markers that are not required to understand or act.

Data roles use tabular numerals and the data face. UI roles use the UI face. The
system does not use a global `transform: scale()` or `zoom` to create density; density
comes from layout, spacing, and the semantic type roles above.

## Accessibility and responsive behavior

- Focus rings use the existing Base-blue treatment and remain visible on keyboard focus.
- Text must remain usable at 125% and 200% browser zoom without document-wide overflow.
- Primary controls retain usable hit areas even when their visible glyphs are compact.
- Desktop reference checks use 1918×744 and 1440×900; compact desktop uses 1280×720;
  mobile uses 390×844.
- The document stays horizontally stable. Only explicitly data-dense tables may scroll.

## Review order

Visual review is performed in this order: geometry and proportions, typography and
hierarchy, contrast and color, then spacing and polish. Baselines are deterministic
and updated only in an explicit visual-change commit.
