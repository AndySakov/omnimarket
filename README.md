# OmniMarket

**A proof of concept: the backend a Solana-first trading terminal like Trojan would need to run on EVM chains.**

I'm Obafemi (Temi) Teminife, a senior backend engineer. I built OmniMarket to show how I design and ship trading infrastructure. It uses Trojan's product as its reference spec: the same order types, fee model and wallet provider (Privy). The question it answers is what it takes to deliver that product on Base and BNB Chain, fast enough for memecoin trading and without putting user funds at risk.

It isn't a commercial product. There are no users, no token, no fundraising and no launch plan ([D85](docs/spec/decisions.md)).

---

## In 60 seconds

| | |
|---|---|
| **What it is** | The engine behind a trading terminal: it watches new tokens appear on chain, prices them live, finds the best route for a trade, sends it fast and safely, and runs stop-losses and take-profits while the trader is offline. |
| **Chains** | Base (being built now), then BNB Chain, then MegaETH. |
| **What works today** | It follows Base live, block by block, and keeps the live state of Uniswap v2 and v3 pools in memory. It checks itself against the chain as it runs, and it records every input so any run can be replayed exactly. |
| **What's designed** | The rest of the terminal: pricing, routing, trade execution, trigger orders, token safety checks and copy trading. Each piece has a written spec and a milestone. |
| **Tech** | Rust, Kafka, Postgres, ClickHouse, Foundry (Solidity), Google Cloud. |
| **Started** | 2026-09-28 (design), 2026-09-29 (build). |

---

## What maps to Trojan's product

Every feature is modelled on something Trojan traders already use, then built for how EVM chains work.

| Trojan traders rely on | OmniMarket's EVM design | Status |
|---|---|---|
| **Speed** | One in-memory engine per chain; a price move reaches a broadcast trigger order in ≤ 75ms internally ([slas.md](docs/spec/slas.md)) | Chain following built; budgets set |
| **New pairs and launches** ("Trenches") | New pools seen the moment they're created; four.meme bonding curves on BNB priced directly, so "buy on migration" works | Base pool discovery built |
| **Limit orders, TP/SL, trailing stops, auto-sell** | The full order set, with each firing guaranteed to happen exactly once, even across a crash or failover | Designed (M6) |
| **Copy trading** | Copies land one block behind the leader, spotted from data the engine already reads, never front-running | Designed (M10) |
| **MEV protection** | On BNB, trades go privately to several block builders at once, so they can't be sandwiched | Designed (M8) |
| **Wallets on Privy** | Same provider. Users sign each trade's terms (amount, minimum price, deadline) and the contract that moves funds enforces them; it has no owner and holds no balance | Designed (M3, M5) |
| **1% fee** | Taken in the same transaction, always in ETH, BNB or a stablecoin, never in the memecoin | Designed (M3) |
| **Safety badges** | Every token is test-sold in a simulation before anyone can buy it; honeypots and hidden taxes show up with evidence | Designed (M7) |

---

## Where it stands (2026-09-30)

| Milestone | Status |
|---|---|
| **M0 Foundations:** Rust workspace, deterministic runtime, CI, local stack, tracing | Done |
| **M1 Base indexer:** follow Base's head, track Uniswap v2 and v3 pools, record and replay | In progress: following and v2/v3 tracking merged; reorg undo and an hour-long exact replay next |
| **M2 to M5:** live prices, router contract, quotes, execution, first real-funds trade on Base | Planned |
| **M6 to M13:** triggers, safety, BNB, copy trading, MegaETH, hardening, production readiness | Planned |

A one-minute live run on 2026-09-30 followed 17 Base blocks and 1,324 events, tracked 56 pools, and passed 12 of 12 spot checks against the chain. The test suite has 75 tests, all passing.

Full milestone table with demos: [docs/build-plan.md](docs/build-plan.md). Live progress: [issues](https://github.com/AndySakov/omnimarket/issues) and [pull requests](https://github.com/AndySakov/omnimarket/pulls?q=is%3Apr).

---

## How the work is run

- **Decisions are written down.** 80+ numbered decisions, each with the options rejected and why ([decisions.md](docs/spec/decisions.md)). Claims about chains and vendors are checked against sources ([verification.md](docs/spec/verification.md)).
- **Nothing risks real money by default.** Outside a few recorded demos with about $50 per chain, every trade runs in shadow mode: the full pipeline against live chain data, stopping just before sending.
- **Every incident can be replayed.** The engine's core logic is deterministic and every input is recorded, so any moment can be re-run exactly and stepped through.
- **Every change is gated.** A change merges only after CI passes and an independent review checks it against its issue and the specs ([D81](docs/spec/decisions.md)). Development uses AI coding agents under that gate.

---

## Where to look

| If you have | Read |
|---|---|
| 2 minutes | This page |
| 10 minutes | [docs/highlights.md](docs/highlights.md): the design choices worth talking about, each with a one-line summary |
| 20 minutes | [docs/spec/product.md](docs/spec/product.md) and [docs/build-plan.md](docs/build-plan.md) |
| An engineer with you | [docs/spec/](docs/spec/README.md) (subsystem specs) and [crates/](crates/) (the code) |

---

## For engineers

```bash
./scripts/setup.sh                                     # once per clone: git hooks and tooling
./scripts/verify.sh                                    # fmt, clippy, tests, determinism checks
cargo run -p engine -- follow --minutes 1 --check-every 5   # follow Base live on public RPC
```

| Path | Role |
|---|---|
| [crates/](crates/) | Rust workspace: `engine`, `chain-io`, `venues`, `det` (deterministic runtime), `sim`, `telemetry`, `types`, `proto` |
| [proto/](proto/) | Event and API schemas, with lineage IDs on every record |
| [docs/spec/](docs/spec/README.md) | Subsystem specs and the decision log |
| [CONTEXT.md](CONTEXT.md) | Vocabulary used across code and docs |
| [CLAUDE.md](CLAUDE.md) | Working rules for contributors and agents |

The terminal UI is built separately by Jutin ([#56](https://github.com/AndySakov/omnimarket/pull/56)).
