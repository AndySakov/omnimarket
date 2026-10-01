# Build Plan

**Status:** Approved (D63). In build since 2026-09-29, at M0. Implements the decisions in [spec/decisions.md](spec/decisions.md).

## Approach: one chain end to end, then widen

Build a **walking skeleton on Base first**: every layer thin but real, from chain events to a landed trade and a firing stop-loss, demoable in shadow mode (D5). Then add depth (venues, order types, safety) and breadth (BNB, MegaETH).

**Why Base first:** the first chain's job is to prove the architecture.
- Base is measured, free to follow (public RPC, D17) and has reorgs to exercise the reconciler and tiered undo (D10, D12). It follows canonical blocks only, so Base's planned switch from Flashblocks to 200ms blocks doesn't change the design (D77).
- The fewest unverified pieces: Permit2, Privy and the own-node plan (D44) are all confirmed.
- Uniswap v2/v3/v4 math carries over to PancakeSwap on BNB and Kumbaya on MegaETH.
- No public mempool, so private submission (D30), four.meme and Infinity bin pools can wait for BNB.

## Repository layout

```
omnimarket/
├── proto/                    Protobuf schemas: events, lineage IDs, API types (D41, D53)
├── crates/                   Rust workspace (D45)
│   ├── types/                Chain IDs, amounts, IDs, lineage IDs
│   ├── proto/                Rust types generated from proto/ (prost + protox)
│   ├── telemetry/            tracing → OTLP → Tempo setup shared by every binary (D53, D70)
│   ├── det/                  Deterministic runtime: Clock, Rng, EventSource, Rpc, Signer, Broadcaster, Store traits;
│   │                         real, recorded and simulated implementations (D49, D54)
│   ├── chain-io/             Providers, subscriptions, reconciler inputs, failover (D10, D16)
│   ├── venues/               Pool state + exact math per venue: v2, v3, v4 (+hooks), Aerodrome, Algebra,
│   │                         PancakeSwap Infinity, bonding curves (D21, D36)
│   ├── pricing/              Display price, USD conversion, fair price (D18–D24)
│   ├── routing/              Route search, cues, penalties, own-flow overlay (D25)
│   ├── safety/               Simulation checks, inspection, behavioural signals (D29)
│   ├── triggers/             Order catalogue, sorted index, firing, exit guarantee (D37–D39, D60)
│   ├── engine/               Chain Engine binary: composes the above; snapshots, lease + fencing (D6, D40)
│   ├── execution/            Execution binary: intents, executors, nonce ledger, submission (D30–D35, D42, D59–D61)
│   ├── api/                  REST + WebSocket gateway (D62)
│   ├── candles/, history/    Cold-path services (D15, D41)
│   ├── watcher/              Independent watcher: shares only proto/, by design (D55)
│   └── sim/                  Deterministic simulation harness and chaos fuzzer (D49)
├── contracts/                Foundry: router, intent verification, invariant tests (D26, D58, D59)
├── web/thin/                 Thin prototyping UI (project lead)
├── web/terminal/             Full terminal UI (Jutin, D62)
├── loadtest/                 k6 scripts, scenarios, fuzz configs (D48, D49)
├── deploy/                   Terraform, Helm, Argo CD (D50, D51)
└── docs/
```

## Build-mode rules (from the spec)

1. **Core logic is deterministic.** Time, randomness, network, RPC and signing only through `det` traits (D49, D54). A lint or review check enforces it from the first commit.
2. **Every record carries lineage IDs** (D53); schemas define them, services can't forget them.
3. **Every input is recordable** (D54): each `det` implementation has a recording wrapper.
4. **Shadow mode is the default** everywhere outside the real-funds demos (D5).
5. **Decisions stay logged:** anything that changes a D-entry gets an amendment note and a new entry.
6. **Boring Rust (D45):** well-known crates, plain structs and enums, no clever generics or custom macros in core logic, explicit error types, comments on non-obvious ownership or async. Readable by someone learning Rust on the job.
7. **Single-threaded cores:** engine and execution cores are state machines driven by one task; I/O runs on tasks around them (D49).

## Milestones

Each milestone ends with a **shadow-mode demo**, tests, dashboards, and docs updated.

| # | Milestone | Scope | Demo |
|---|---|---|---|
| **M0** | Foundations | `proto/`, `types`, `det` and `sim` crates only (D70); CI on GitHub Actions (D76); `det` runtime (real, recorded, simulated); local stack (compose + Anvil); observability skeleton (lineage, traces); Base **measurement tasks** (below) | CI green; a simulated-clock test replays identically |
| **M1** | Base indexer | Canonical blocks via `newHeads` + `getLogs` (D77), gap fill and reorg detection in the follower and core (no separate reconciler, D84), reorg handling, tiered undo, bootstrap with batched reads, v2/v3 pools, input recorder | Live Base pool state; a recorded hour replays exactly |
| **M2** | Pricing + feeds | Display price, USD conversion, candle service, discovery and token WebSocket feeds, API contract v0 + **mock server for Jutin** | Thin UI shows live Base prices, candles and new pools |
| **M3** | Router contract | Intents (Permit2 witness), submitter field, D59 terms, fee and gas caps; Foundry fuzz + invariants, Slither, Aderyn | Invariant suite green; router deployed on a Base fork |
| **M4** | Routing + quotes | Single, 2-hop, split; cues; quote API; shadow check vs simulation | Quotes ≤ 10ms p99; shadow mismatch dashboard |
| **M5** | Execution (skeleton complete) | Executor pool, nonce ledger, blocking simulation, submission, tracking, user-session signing via Privy | Manual buy → landed trade in shadow; **first real-funds trade on Base** |
| **M6** | Triggers | Order catalogue core (limit, TP/SL, multi-level TP, trailing, auto-sell), exactly-once firing, exit guarantee | Stop-loss cascade scenario (D48 #3) in shadow |
| **M7** | Safety | Four safety layers (simulation, inspection, liquidity, behaviour), safety evidence in UI | Honeypot blocked in shadow; safety evidence shown |
| **M8** | BNB (D64) | PancakeSwap v2/v3/Infinity, four.meme curves + migration, private builder fan-out, execution-quality report (D65) | New four.meme token → buy → graduation → sell, in shadow; sandwiches avoided shown in the report |
| **M9** | Base depth | v4 hooks + launchpad hook pools (Clanker, Zora, Flaunch), Aerodrome | Launch-fee decay quoted exactly in shadow |
| **M10** | Copy trading + event orders | Copy trading, dev-sell, migration, scheduled orders | Copy fan-out scenario (D48 #4) |
| **M11** | Chain onboarding kit + MegaETH (D67, D69) | Chain adapter kit and checklist; MegaETH (Realtime API, Kumbaya simulated until verified, Algebra) as its first use | MegaETH onboarded through the kit; firehose replay at 5× |
| **M12** | Hardening | Engine + execution failover, brakes, independent watcher, audit-log anchoring, chaos fuzz in CI, full load suite, security pass | Failover under load with zero duplicate or missed firings |
| **M13** | Production readiness (D85) | GCP production layout, SLOs and alerts, runbooks, public status page | Production canaries green on BNB and Base |

*(Order revised by D64 and D69 after the research in [market.md](market.md): BNB moves ahead of Base depth, MegaETH moves behind copy trading. M13 trimmed by D85: this is a proof of concept, so nothing launches.)*

**Frontend track (parallel, Jutin):** starts at M2 against the mock server; switches to the real API per milestone.

## Measurement tasks (need live network access)

From [verification.md](spec/verification.md), placed at the milestone that uses each result (D70):

| Milestone | Tasks |
|---|---|
| M0 | Base event rates · Base provider delivery delay |
| M4 | Chainstack state overrides (simulation for the shadow check) |
| M5 | Privy signing latency (server and browser) · Base sequencer direct submission · Base sequencer location |
| M8 | BNB event rates, delivery delay and quote-asset coverage · BNB builder inclusion latency and locations |
| M11 | Chainstack MegaETH mini-block `logs` · MegaETH finality lag · Kumbaya launchpad |

Base quote-asset coverage is measured in M2, where USD conversion lands.

## Relative effort (no timeline commitment, D63)

Relative size of each stretch of milestones, for one backend developer with the frontend in parallel:

| Milestones | Rough size |
|---|---|
| M0–M5 (walking skeleton on Base, first real trade) | 6–8 weeks |
| M6–M7 (triggers, safety) | 4–5 weeks |
| M8–M11 (BNB, Base depth, copy trading, MegaETH) | 6–8 weeks |
| M12–M13 (hardening, production readiness) | 3–5 weeks |
| **Total** | **~5 months** |

Rough relative sizes only, before AI assistance; no dates are committed (D63). The walking skeleton (M5) is the first point where the project is demoable end to end.
