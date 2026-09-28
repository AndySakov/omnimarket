# Decision Log

Newest last. Format: decision, alternatives rejected, reasoning.

---

## D1 — Build an EVM trading terminal, not an exchange

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** OmniMarket is a trading terminal that routes user swaps through existing on-chain liquidity (DEX pools), with indexing, pricing, routing, execution, and automated orders.

**Rejected:**
- *Hybrid CLOB exchange (off-chain matching, on-chain vault settlement).* Impressive in general, but trading terminals don't run order books. It leaves the key gap unaddressed and adds a custody contract we'd have to trust-model and secure.
- *Prediction market.* Same problem, plus oracle/resolution scope.

**Why:** The target experience is a high-throughput EVM trading terminal (indexing, routing, performance, stability). A terminal hits that directly and builds on existing indexer/pricing-engine experience. Kafka, ClickHouse, CQRS, and k8s still earn their place in the indexing → pricing → trigger pipeline.

---

## D2 — Chains: MegaETH, Base, BNB Chain

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Support three EVM chains from day one.

**Why:** Three chains with very different profiles force a real multi-chain abstraction instead of a single-chain design with a chain ID bolted on:
- **MegaETH** — real-time chain with ~10ms mini-blocks and ~1s EVM blocks **(verify)**. Stress-tests ingestion throughput and what "confirmed" means.
- **Base** — the main EVM memecoin venue. OP-stack L2, sequencer-ordered, private-ish mempool.
- **BNB Chain** — high retail volume, public mempool, so MEV/sandwich protection matters. PancakeSwap-dominated.

**Consequence:** Per-chain config for block time, finality/reorg depth, DEX set, gas model, and transaction submission path.

---

## D3 — Wallet model: embedded wallets with keys in enclaves, delegated signing under policy

**Date:** 2026-09-28 · **Status:** Decided (vendor: see D4)

**Decision:** Each user gets platform-created wallets whose keys live in a vendor's secure enclave (TEE). Users can export their keys. The backend signs for automated orders only through scoped, policy-limited delegation. Full reasoning in [wallets.md](wallets.md).

**Rejected:**
- *Server-held encrypted keys (classic Telegram-bot model).* Fastest, but the operator is a single point of total loss — a honeypot for attackers.
- *Connect-your-own-wallet (MetaMask).* Every trade needs a user click, which kills automated orders and speed.
- *Smart accounts with session keys (ERC-4337 / EIP-7702).* Promising and elegant, but not yet what terminals ship, and support varies across our three chains. Kept as a stretch comparison.

---

## D4 — Wallet vendor: Privy, behind our own Signer boundary

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Privy embedded wallets with server-side signing via authorization keys and policies. All signing goes through an internal `Signer` boundary with two implementations: Privy (product/demo path) and a local encrypted keystore (load tests, mainnet forks).

**Rejected:**
- *Turnkey.* Stronger verifiability and policy engine, per-signature pricing suits high volume. Not chosen because Privy reportedly matches Trojan's stack and bundles auth, so we don't build login.

**Why:** Stack parity with the target team, less undifferentiated work (auth), and the `Signer` boundary keeps vendor lock-in and test-environment constraints contained.

**Consequence:** Must verify Privy server-side signing on MegaETH, Base, and BNB. Signing latency to Privy sits on the hot path and needs its own SLA line.

---

## D5 — Environments: live reads, shadow execution, real-funds proof

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Three execution modes over one live read path.
- **Live read path.** Indexer, pricing, router, and triggers always run against real MegaETH, Base, and BNB mainnet data.
- **Shadow execution.** Full pipeline (quote → build → simulate against live state → sign with local signer) that stops before broadcast. Target of all k6 and custom load scenarios.
- **Real funds.** Roughly $50 per chain in Privy wallets for recorded end-to-end demos.
- **Local forks.** Failure-injection testing only: nonce gaps, stuck transactions, simulated reorgs, RPC outages.

**Rejected:**
- *Forks only.* A fork drifts from live state within seconds; no real mempool, inclusion, or MEV.
- *Real funds only.* Can't load test; bugs cost money.

**Why:** Keeps the read path authentic, makes execution load-testable at scale with zero spend, and still produces a real on-chain proof. Shadow mode also enables scripted usage scenarios (flash crowd on a new pair, mass stop-loss cascade, copy-trade fan-out) driven by k6 and custom generators.

**Consequence:** Execution mode is a first-class config per request/environment, not a test hack. Paid RPC is needed for head-following three mainnets — budgeted in `indexer.md`.

---

## D6 — Hot/cold split: one in-memory engine per chain, Kafka alongside

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Each chain gets a stateful engine that follows the chain head directly and holds live pool state in memory. Pricing, routing/quoting, and trigger evaluation run in-process. The engine publishes every state change to Kafka, which feeds all cold-path consumers (ClickHouse, Postgres, UI feeds). Kafka is the system of record and fan-out, never a hop between a price change and an order.

**Rejected:**
- *Everything through Kafka.* Clean and replayable, but adds broker hops and consumer lag to the latency-critical path; the router could quote on stale pools.
- *Faster bus (NATS / Redis Streams) on the hot path.* An extra system to operate, to save milliseconds that the in-process design never spends.

**Why:** The hot path's latency budget (see draft targets in product.md) doesn't fit broker hops, and the hot path's data (pool state) is naturally per-chain and fits in memory.

**Consequence:** The engine is stateful, so we must spec: recovery (snapshot pool state + replay recent blocks), a warm standby per chain with failover, and indexed trigger evaluation (price-sorted structures, not scans). → `architecture.md`, `triggers.md`

---

## D7 — Two phases: terminal first, prop AMM on MegaETH second

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Phase 1 (≈1 month):** the trading terminal as specced — the taker side.
- **Phase 2:** a small proprietary AMM on MegaETH — the maker side. An on-chain pool whose price is pushed several times per block by our own quoting service. It reuses the Chain Engine's cross-chain fair prices as its pricing signal, and our own router trades against it as an opaque venue.

**Rejected:**
- *Terminal only.* Strongest fit for trading-terminal roles, but ignores where on-chain latency engineering is actually heading.
- *Pivot to a prop AMM now.* Closest to literal on-chain HFT, but a weak fit for terminal roles, needs a pricing model and capital to be real, and discards the terminal spec.

**Why:** On-chain "HFT" is limited by block time on most chains. The real latency games are inclusion (terminals) and top-of-block repricing (prop AMMs). Covering both — consuming prop-AMM liquidity, then providing it — spans both hiring pools. About half of phase 2 is already built by phase 1 (head follower, pool state, fair pricing, execution/nonce management).

**Consequence:** Phase 1 does **no** phase-2 work. Phase 2 stays cheap only because phase 1 already needs:
- a published fair price per token (UI, PnL, and triggers consume it)
- a router that asks venues for quotes without assuming how the quote is computed
- nonce management that allows several in-flight transactions per wallet (trigger bursts, copy-trade fan-out)

Framing: OmniMarket is built and presented as a startup attempt in the space (public repo, write-ups, recorded real-funds demos), not a tutorial project.

---

## D8 — Execution: separate service per chain, called directly by the engine

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Each chain has an Execution service (build → simulate → sign → submit → track) that the Chain Engine and the API call directly over gRPC. It is the single owner of every wallet's nonce on its chain. It publishes execution outcomes to Kafka for the cold path.

**Rejected:**
- *Inside the Chain Engine.* Zero hops, but slow network I/O (Privy, RPC) shares a process with the pricing loop, an execution bug can take down pricing, and execution can't scale independently.
- *Via Kafka.* Puts a broker hop on the hot path (contradicts D6).

**Why:** Isolates slow and failure-prone I/O from the in-memory engine for the cost of one sub-millisecond in-cluster hop, and gives nonce ownership one obvious home.

**Consequence:** Must spec exactly-once trigger firing across the engine → execution boundary: every firing carries a unique ID, execution ignores IDs it has already handled, and the engine retries any firing not acknowledged. → `execution.md`

---

## D9 — Indexing is two jobs: live state (built) and history (build or buy)

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Live state** (hot path, inside the Chain Engine, built in-house): bootstrap each pool's current state with contract reads at a known block, then follow the chain tip and apply events in memory. Needs no history.
- **History** (cold path, separate job): backfill past swaps and pool events into ClickHouse for charts, volume, and PnL. Can lag and restart. Managed tooling is acceptable here.

**Rejected:**
- *One pipeline for both* (e.g. Ponder / Envio style). Frameworks are built for events → database, not millisecond in-memory state. Coupling the two means backfill problems can stall live pricing.
- *iTRY-style managed webhooks for the live path.* Right for a few known contracts at low volume with minutes of tolerance (see [case study](../primers/case-study-itry.md)); wrong for thousands of unknown pools, ms latency, and act-on-tip.

**Why:** Current pool state doesn't need history, so correctness of the hot path shouldn't depend on it. Backfill depth becomes a product choice (how much chart history), not a correctness requirement.

**Consequence:** The engine needs a bootstrap procedure (discover pools, read state at block N, apply events from N+1). Positions and PnL carry provisional → confirmed status. → `indexer.md`

---

## D10 — Tip following: fastest stream per chain + canonical block reconciler

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Each Chain Engine runs two loops.
- **Fast loop** — subscribes to *filtered logs* (only the event types we process) on the chain's fastest stream: MegaETH mini-blocks (~10ms, Realtime API), Base Flashblocks `pendingLogs` (~200ms), BNB blocks (0.45s). Updates state immediately; everything it applies is provisional.
- **Reconciler loop** — reads canonical blocks (`getLogs` by block), confirms or corrects fast-loop state, detects reorgs via parent hashes, and fills gaps from dropped subscriptions.

A dropped preconfirmation is treated as a reorg at depth zero: one correction mechanism for both.

**Rejected:**
- *Canonical blocks only.* Uniform and simpler, but latency = block time (≈100× slower than possible on MegaETH, 10× on Base). Throws away the edge a terminal competes on.
- *Managed streams.* Adds delivery delay, likely can't expose mini-blocks, and outsources the component that most needs to be ours.
- *Full-block firehose on the fast path.* Mostly data we'd discard, at MegaETH rates. Completeness belongs in the reconciler.

**Consequence:** Providers must support MegaETH Realtime API and Base Flashblocks subscriptions (feeds the RPC decision). State must be undoable over the reconciler window.

---

## D11 — Pool coverage: discover everything, fully track only active pools

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Two tiers.
- **Known:** every pool of a supported DEX type, recorded from creation events (metadata only).
- **Active:** full in-memory state, pricing, and routing. A pool is active if any of:
  - paired with a base asset (WETH, WBNB, USDC, USDT) and liquidity ≥ floor
  - within a grace window after creation (new-pairs feed)
  - referenced by a user: open position, trigger order, or copied wallet trading it

Pools are demoted when they stop qualifying and promoted on a swap or liquidity spike.

**Rejected:**
- *Track everything.* Memory, bootstrap cost, and router noise scale with spam.
- *Allowlist.* Breaks new-token discovery, the core journey.

**Why:** Memory and compute follow real activity, not spam, without losing the "see it the moment it's created" experience. Same shape as cache admission / tiered storage.

**Consequence:** Promotion needs a warm-up path (read state at block N, apply buffered events), the same procedure as bootstrap. Thresholds (liquidity floor, grace window) are tuning parameters set in `indexer.md`.

---

## D12 — Tiered undo: automatic to finality, memory holds only the recent window

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Undo is automatic all the way to L1 finality, stored in three tiers.

| Tier | Covers | Storage | Speed |
|---|---|---|---|
| 1 Hot | Provisional + last ~10s of canonical blocks | Engine memory (per-block previous values) | µs |
| 2 Warm | ~10s → L1 finality | Kafka: every published pool update carries before + after state | Sub-second to seconds |
| 3 Rebuild | Inconsistency, or Kafka unavailable | Bootstrap from a known-good block | Seconds |

A tier-2 undo to block A restores each touched pool's *before* value from its first change after A. The affected chain's engine pauses quotes and trigger evaluation until caught up; other chains are unaffected. BNB needs only tier 1 (fast finality ~1.1s). The cold path tracks a per-chain finality watermark; records move provisional → confirmed → final.

**Rejected:**
- *Undo held in memory until finality.* Automatic, but ~20 minutes of undo at MegaETH rates, for events that almost never happen.
- *Short undo + rebuild for anything deeper.* Lean, but deep reorgs cost a full rebuild instead of a targeted rollback.
- *Rebuild on every correction.* Dropped preconfirmations are frequent enough to cause constant stalls.
- *Tier 2 on the engine's local disk.* Independent of Kafka, but a second log to build, and the standby can't share it.

**Why:** Keeps full automation with memory bounded by a short window. The system-of-record log doubles as the undo log, so no new storage is introduced.

**Consequence:**
- Pool-update events carry before + after state (larger payloads).
- Kafka retention on pool-update topics must exceed each chain's finality window.
- After any tier-2 undo, spot-check restored pools against contract reads; a mismatch escalates to tier 3.
