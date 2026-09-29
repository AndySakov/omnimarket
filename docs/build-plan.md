# Build Plan

**Status:** Approved (D63). Build mode not yet started. Implements decisions D1–D69 in [spec/decisions.md](spec/decisions.md).

## Approach: one chain end to end, then widen

Build a **walking skeleton on Base first**: every layer thin but real, from chain events to a landed trade and a firing stop-loss, demoable in shadow mode (D5). Then add depth (venues, order types, safety) and breadth (BNB, MegaETH).

**Why Base first:** the first chain's job is to prove the architecture.
- Base's Flashblocks exercise the full provisional → confirmed → undo machinery (D10, D12); BNB has no preconfirmations to test it with.
- The fewest unverified pieces: Permit2, Privy, a free public Flashblocks feed (D17) and the own-node plan (D44) are all confirmed.
- Uniswap v2/v3/v4 math carries over to PancakeSwap on BNB and Kumbaya on MegaETH.
- No public mempool, so private submission (D30), four.meme and Infinity bin pools can wait for BNB.

## Repository layout

```
omnimarket/
├── proto/                    Protobuf schemas: events, lineage IDs, API types (D41, D53)
├── crates/                   Rust workspace (D45)
│   ├── types/                Chain IDs, amounts, IDs, lineage IDs
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
| **M0** | Foundations | Repo layout, CI on Blacksmith, proto, `det` runtime (real + simulated), local stack (k3d, compose, Anvil), observability skeleton (lineage, traces), **measurement tasks** (below) | CI green; a simulated-clock test replays identically |
| **M1** | Base indexer | Flashblocks tick + pending `getLogs`, reconciler, reorg and dropped-preconfirmation handling, tiered undo, bootstrap with batched reads, v2/v3 pools, input recorder | Live Base pool state; a recorded hour replays exactly |
| **M2** | Pricing + feeds | Display price, USD conversion, candle service, discovery and token WebSocket feeds, API contract v0 + **mock server for Jutin** | Thin UI shows live Base prices, candles and new pools |
| **M3** | Router contract | Intents (Permit2 witness), submitter field, D59 terms, fee and gas caps; Foundry fuzz + invariants, Slither, Aderyn | Invariant suite green; router deployed on a Base fork |
| **M4** | Routing + quotes | Single, 2-hop, split; cues; quote API; shadow check vs simulation | Quotes ≤ 10ms p99; shadow mismatch dashboard |
| **M5** | Execution (skeleton complete) | Executor pool, nonce ledger, blocking simulation, submission, tracking, user-session signing via Privy | Manual buy → landed trade in shadow; **first real-funds trade on Base** |
| **M6** | Triggers | Order catalogue core (limit, TP/SL, multi-level TP, trailing, auto-sell), exactly-once firing, exit guarantee | Stop-loss cascade scenario (D48 #3) in shadow |
| **M7** | Safety | Four safety layers (simulation, inspection, liquidity, behaviour), safety evidence in UI | Honeypot blocked in shadow; safety evidence shown |
| **M8** | BNB (launch beachhead, D64) | PancakeSwap v2/v3/Infinity, four.meme curves + migration, private builder fan-out, execution-quality report (D65) | New four.meme token → buy → graduation → sell, in shadow; sandwiches avoided shown in the report |
| **M9** | Base depth | v4 hooks + launchpad hook pools (Clanker, Zora, Flaunch), Aerodrome | Launch-fee decay quoted exactly in shadow |
| **M10** | Copy trading + event orders | Copy trading, dev-sell, migration, scheduled orders | Copy fan-out scenario (D48 #4) |
| **M11** | Chain onboarding kit + MegaETH (D67, D69) | Chain adapter kit and checklist; MegaETH (Realtime API, Kumbaya simulated until verified, Algebra) as its first use | MegaETH onboarded through the kit; firehose replay at 5× |
| **M12** | Hardening | Engine + execution failover, brakes, independent watcher, audit-log anchoring, chaos fuzz in CI, full load suite, security pass | Failover under load with zero duplicate or missed firings |
| **M13** | Launch readiness | GCP production, SLOs and alerts, runbooks, public status page, audit contest / bug bounty, fees + referral tiers (D68) | Production canaries green; BNB launch first |

*(Order revised by D64 and D69 after the market research in [market.md](market.md): BNB moves ahead of Base depth, MegaETH moves behind copy trading.)*

**Frontend track (parallel, Jutin):** starts at M2 against the mock server; switches to the real API per milestone.

## Measurement tasks (M0, need live network access)

From [verification.md](spec/verification.md): event rates per chain · Chainstack MegaETH mini-block `logs` · quote-asset coverage · MegaETH finality lag · Privy signing latency (server and browser) · BNB builder inclusion latency · provider delivery delay · Base sequencer direct submission · sequencer and builder locations · Kumbaya launchpad · Chainstack state overrides.

## Relative effort (no timeline commitment, D63)

Relative size of each stretch of milestones, for one backend developer with the frontend in parallel:

| Milestones | Rough size |
|---|---|
| M0–M5 (walking skeleton on Base, first real trade) | 6–8 weeks |
| M6–M7 (triggers, safety) | 4–5 weeks |
| M8–M11 (BNB, Base depth, copy trading, MegaETH) | 6–8 weeks |
| M12–M13 (hardening, launch) | 4–6 weeks |
| **Total** | **~5–6 months** |

Rough relative sizes only, before AI assistance; no dates are committed (D63). The walking skeleton (M5) is the first point where the project is demoable end to end.
