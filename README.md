# OmniMarket

**A proof of concept: the backend a Solana-first trading terminal like Trojan would need to run on EVM chains.**

I'm [Teminife (Temi) Obafemi](https://www.linkedin.com/in/obafemiteminife), a senior backend engineer. I built OmniMarket to show how I design and ship trading infrastructure. It uses Trojan's product as its reference spec, with the same order types, fee model, and wallet provider (Privy). It works out what it takes to run that product on Base and BNB Chain, fast enough for memecoin trading and without putting user funds at risk.

Before this, at Inverter Network I owned the transaction path for iTRY, a tokenized fund. When I joined, transactions were dying on-chain. I rebuilt our nonce handling, and failed submissions dropped by 90%. Before that, at JobStash, I ran Privy as our auth and embedded-wallet provider for two years.

OmniMarket isn't a commercial product. There are no users, no token, no fundraising, and no launch plan ([D85](docs/spec/decisions.md)).

---

## In 60 seconds

| | |
|---|---|
| **What it is** | The engine behind a trading terminal: it watches new tokens appear on chain, prices them live, finds the best route for a trade, sends it fast and safely, and runs stop-losses and take-profits while the trader is offline. |
| **Chains** | Base (being built now), then BNB Chain, then MegaETH. |
| **What works today** | It follows Base live, block by block, and keeps live state in memory for the Uniswap v2 and v3 pools that trade. It can check itself against the chain as it runs (`--check-every`), and it records every input so any run can be replayed exactly. A 15-minute live run tracked 576 pools across 450 blocks, matched the chain on all 3,445 spot checks, and replayed with identical output ([#52](https://github.com/AndySakov/omnimarket/pull/52)). |
| **What's designed** | The rest of the terminal: pricing, routing, trade execution, trigger orders, token safety checks, and copy trading. Each piece has a written spec and a milestone. |
| **Tech** | Rust, Kafka, Protobuf, OpenTelemetry, and object storage in use. Postgres, ClickHouse, Foundry (Solidity), and Google Cloud come in later milestones. |
| **Started** | 2026-09-28 (design), 2026-09-29 (build). |

---

## What maps to Trojan's product

Each row starts from something Trojan traders already use, then works out how it runs on EVM chains.

| Trojan traders rely on | OmniMarket's EVM design | Status |
|---|---|---|
| **Speed** | One in-memory engine per chain, with Kafka kept off the hot path. The budget from a price move to a broadcast trigger order is 75ms ([slas.md](docs/spec/slas.md)) | Chain following built. Budgets set, not yet measured end to end |
| **New pairs and launches** ("Trenches") | New pools seen the moment they're created. On BNB, four.meme bonding curves are priced directly, so "buy on migration" works | Base pool discovery built |
| **Limit orders, TP/SL, trailing stops, and auto-sell** | The full order set, with each firing guaranteed to happen exactly once, even across a crash or failover | Designed (M6) |
| **Copy trading** | Copies land one block behind the leader, spotted from data the engine already reads, never front-running | Designed (M10) |
| **MEV protection** | On BNB, trades go privately to several block builders at once, so they never touch the public mempool, where sandwich bots watch | Designed (M8) |
| **Wallets on Privy** | Same provider. Users sign each trade's terms (amount, minimum price, and deadline), and the contract that moves funds enforces them. It has no owner and holds no balance | Designed (M3, M5) |
| **1% fee** | Taken in the same transaction, always in ETH, BNB, or a stablecoin, never in the memecoin | Designed (M3) |
| **Safety badges** | Every token is test-sold in a simulation before anyone can buy it. Honeypots and hidden taxes show up with evidence | Designed (M7) |

---

## What building on live Base turned up

- **A rate limit that looks like an answer.** Base's public endpoint allows about 20 `eth_call`s per 30 seconds, however big the batch. It answers the rest with HTTP 429 and a JSON-RPC error body, which a client reads as the call's result unless it checks. Pool reads failed in bulk until the call worker retried rate limits instead of passing them on, and the reads moved to PublicNode ([D82](docs/spec/decisions.md)).
- **A block that disappears mid-read.** If a block is replaced between reading its header and reading its logs, the node answers "block not found" for good. A naive follower retries forever and stops seeing the chain. The fix, in review, reads the height again and carries on with whatever is canonical now ([#57](https://github.com/AndySakov/omnimarket/pull/57)).
- **Pools that events can't rebuild.** A Uniswap v3 pool first seen mid-life can't be rebuilt from its events. They only record changes to its tick table. The engine reads the pool once at that block in three batched rounds, then applies later events on top ([#52](https://github.com/AndySakov/omnimarket/pull/52)).

---

## Where it stands (2026-09-30)

| Milestone | Status |
|---|---|
| **M0 Foundations:** Rust workspace, deterministic runtime, CI, local stack, and tracing | Done |
| **M1 Base indexer:** follow Base's head, track Uniswap v2 and v3 pools, record and replay | In progress: following, v2/v3 tracking, and the input archive merged. Reorg undo and an hour-long exact replay are next |
| **M2 to M5:** live prices, router contract, quotes, execution, and the first real-funds trade on Base | Planned |
| **M6 to M13:** triggers, safety, BNB, copy trading, MegaETH, hardening, and production readiness | Planned |

Every change passes fmt, clippy and the full test suite in CI before it merges.

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
| 15 minutes | [#52](https://github.com/AndySakov/omnimarket/pull/52): one change from problem to live measurement (tracking Uniswap v3 pools on Base) |
| 20 minutes | [docs/spec/product.md](docs/spec/product.md) and [docs/build-plan.md](docs/build-plan.md) |
| An engineer with you | [docs/spec/](docs/spec/README.md) (subsystem specs) and [crates/](crates/) (the code) |

---

## For engineers

```bash
./scripts/setup.sh                                     # once per clone: git hooks and tooling
./scripts/verify.sh                                    # fmt, clippy, tests, and determinism checks
cargo run -p engine -- follow --minutes 1 --check-every 5   # follow Base live on public RPC
```

The engine reads blocks from Base's public endpoint and sends its pool reads to PublicNode's free one ([D82](docs/spec/decisions.md)). If either endpoint can't answer, `engine follow` exits with an error naming its flag, `--rpc` or `--call-rpc`: within about 8 seconds if it's down at the start, about a minute after it stops answering otherwise ([D88](docs/spec/decisions.md)). Pass that flag with another Base RPC URL. Base's own endpoint rate-limits calls too tightly for `--call-rpc`: a run against it stops with its rate-limit error.

| Path | Role |
|---|---|
| [crates/](crates/) | Rust workspace: `engine`, `chain-io`, `venues`, `det` (deterministic runtime), `sim`, `telemetry`, `types`, `proto` |
| [proto/](proto/) | Event and API schemas, with lineage IDs on every record |
| [docs/spec/](docs/spec/README.md) | Subsystem specs and the decision log |
| [CONTEXT.md](CONTEXT.md) | Vocabulary used across code and docs |
| [CLAUDE.md](CLAUDE.md) | Working rules for contributors and agents |

Jutin, a frontend engineer, is building the terminal UI in parallel ([#56](https://github.com/AndySakov/omnimarket/pull/56)).
