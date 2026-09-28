# OmniMarket Spec

**Status:** Planning. Docs only — no code until we explicitly switch to build mode.

OmniMarket is a multi-chain EVM trading terminal backend: token discovery, live pricing, best-route swaps, fast execution, and automated orders across MegaETH, Base, and BNB Chain. It's modelled on what an on-chain trading terminal like Trojan needs as it expands into EVM.

## Documents

| Doc | Covers | Status |
|---|---|---|
| [product.md](product.md) | Vision, users, journeys, feature scope, non-goals | Draft |
| [wallets.md](wallets.md) | Custody models, industry landscape, our choice | Draft |
| [architecture.md](architecture.md) | System shape, hot/cold split, component responsibilities | Draft |
| [indexer.md](indexer.md) | Live state vs history, tip following, bootstrap | Draft |
| [pricing.md](pricing.md) | Display, trigger, and execution prices; per-DEX math | Draft |
| [routing.md](routing.md) | Route shapes, cue-driven route choice | Draft |
| [execution.md](execution.md) | Submission paths, signing, nonces, gas, tracking | Draft |
| [verification.md](verification.md) | Results of checking every (verify) marker, with sources | Living |
| [decisions.md](decisions.md) | Decision log — what we chose, what we rejected, why | Living |

Planned: `triggers.md`, `data.md`, `infra.md`, `slas.md`, `loadtest.md` (k6 + custom usage scenarios against shadow execution).

Talking points: [../highlights.md](../highlights.md), the design choices worth explaining to others.

Background primers (not spec) live in [../primers/](../primers/): [indexing](../primers/indexing.md), [case study: iTRY](../primers/case-study-itry.md).

## Working rules

- One decision at a time, biggest first. Every decision lands in `decisions.md` with the rejected options and the reason.
- Tight over thorough. If a section doesn't change what we'd build, cut it.
- Anything unverified is marked **(verify)**. Open questions stay listed until closed.
