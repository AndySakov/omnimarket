# OmniMarket Spec

**Status:** Building, since 2026-09-29. Specs stay the source of truth: code that disagrees with a spec or D-entry changes both in the same commit.

OmniMarket is a proof of concept (D85) for a multi-chain EVM trading terminal backend: token discovery, live pricing, best-route swaps, fast execution, and automated orders across Base, BNB Chain and MegaETH. It's modelled on what an on-chain trading terminal like Trojan needs as it expands into EVM.

**New here?** Start at the root [README](../../README.md), then [highlights](../highlights.md) and [product](product.md).

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
| [triggers.md](triggers.md) | Order catalogue, event triggers, copy trading | Draft |
| [data.md](data.md) | Stores, keys, Kafka topics, retention, snapshots | Draft |
| [slas.md](slas.md) | Latency targets and per-step budgets | Draft |
| [loadtest.md](loadtest.md) | Named load scenarios and chaos fuzzing | Draft |
| [infra.md](infra.md) | Environments, production layout, tooling, CI | Draft |
| [frontend.md](frontend.md) | Screens to mirror, what the backend adds, API contract | Draft |
| [observability.md](observability.md) | Lineage, replay, independent watcher, brakes | Draft |
| [security.md](security.md) | Assets, controls, signing model, incident response | Draft |
| [verification.md](verification.md) | Results of checking every (verify) marker, with sources | Living |
| [decisions.md](decisions.md) | Decision log — what we chose, what we rejected, why | Living |


Talking points: [../highlights.md](../highlights.md), the design choices worth explaining to others.

Build plan (approved, D63): [../build-plan.md](../build-plan.md).

Research on where EVM trading happens and what goes wrong there: [../market.md](../market.md).

Background primers (not spec) live in [../primers/](../primers/): [indexing](../primers/indexing.md), [case study: iTRY](../primers/case-study-itry.md).

## Working rules

- One decision at a time, biggest first. Every decision lands in `decisions.md` with the rejected options and the reason.
- Tight over thorough. If a section doesn't change what we'd build, cut it.
- Anything unverified is marked **(verify)**. Open questions stay listed until closed.
