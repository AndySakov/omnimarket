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

---

## D13 — Bootstrap reads are batched

**Date:** 2026-09-28 · **Status:** Decided (implementation deferred)

**Decision:** Pool bootstrap and promotion read state in batches (multicall, or a lens contract that returns many ticks per call), never one RPC call per pool or per tick.

**Rejected:**
- *One call per pool/tick.* Uniswap v3 liquidity is spread across many ticks; thousands of pools becomes hundreds of thousands of calls per cold start.

**Why:** Cold-start time and RPC spend (D16) both scale with call count, not data size.

**Consequence:** Implementation details (multicall vs lens, batch size, per-provider limits on call gas and response size) go in `indexer.md` when we build.

---

## D14 — Uniswap v4 hooks: full support in phase 1

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Hook pools are supported in phase 1. Where a hook's pricing can be replicated in memory, we do so. Where it can't, the pool is an **opaque venue** quoted by simulation against live state.

**Rejected:**
- *Exclude hook pools in phase 1.* Simpler, but v4 is where new liquidity is going on our chains, so routes would miss it.
- *Opaque-only for every hook pool.* Simulation for all of them costs latency and RPC calls where a local model works.

**Why:** The router already asks venues for quotes without assuming how they're computed (D7). This makes the opaque venue type a phase 1 requirement instead of a phase 2 one.

**Consequence:** Quote-by-simulation is on the phase 1 hot path, so simulation calls count toward the RPC budget (D16). Hook classification (replicable vs opaque) is a per-hook registry. → `pricing.md`, `routing.md`

---

## D15 — History job: build it ourselves

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** The history job (D9) is custom: the same `getLogs` loop as the reconciler, run over past block ranges, writing replay-safe into ClickHouse.

**Rejected:**
- *Envio HyperSync.* ~$70–480/mo.
- *Goldsky.* Billed per worker-hour after a $100 credit.

**Why:** Rule for build vs buy: if a managed service costs extra money, build. Here building costs almost nothing extra: it reuses reconciler code and runs on the RPC plan we pay for anyway (D16). It also covers skills worth having: backfill throughput, the backfill → live hand-off, and idempotent writes.

**Consequence:** ClickHouse writes must be idempotent (keyed by chain, block hash, log index) so backfills can restart and overlap the live feed. **Backfill depth: 30 days per chain at launch**, enough for charts, volume, and PnL on actively traded tokens; deeper backfills can run later without design changes. → `indexer.md`, `data.md`

---

## D16 — RPC: Chainstack primary, QuickNode fallback, per-block streams

**Date:** 2026-09-28 · **Status:** Decided (numbers **(verify)** by measurement)

**Decision:**
- **Development:** free tiers and public feeds (Base public Flashblocks WebSocket, MegaETH public endpoint).
- **Production (and any load test that free quotas can't carry, per D17):** Chainstack Pro (~$199/mo, all three chains) as primary; a QuickNode Build key (~$49/mo) as failover on a separate provider.
- **Fast-loop streams are one message per block, not one per log, wherever the chain allows** (amends D10):

| Chain | Fast stream | Notifications/month |
|---|---|---|
| MegaETH | Filtered `logs` subscription (mini-block latency) | Scales with swap volume |
| Base | `newFlashblocks` (one payload per 200ms flashblock) | ~13M, fixed |
| BNB | `newHeads` + one `getLogs` per block | ~11.5M, fixed |

**Rejected:**
- *Alchemy.* Bills WebSocket pushes by bytes (0.04 CU/byte), which makes streaming the most expensive thing we do.
- *Flat-rate plans* (Chainstack Unlimited from $149 at 25 RPS, QuickNode flat rate from $799). Pushed events likely count against the RPS cap, so the cheap tier would throttle the fast stream in bursts, and the adequate tiers cost 3–4× more.
- *dRPC.* Competitive price, but MegaETH Realtime API support unconfirmed and no Flashblocks upstreams on the free tier.
- *Per-log subscriptions on every chain.* Every provider bills per pushed event; per-log streams on all three chains are an estimated 100–250M events/month that grows with trading volume.
- *Self-hosted nodes now.* ~$150–250/mo per chain plus real ops work, and it's unclear whether MegaETH's replica node is available to outside operators. Revisit for Base when simulation volume grows.

**Why:** Pushed events are over 80% of the estimated load, and every provider bills them. Per-block streams make the Base and BNB cost fixed regardless of volume, and bring the whole budget to ~50–70M requests/month, which fits a per-request plan.

**Consequence:**
- Before paying: measure real event rates per chain (one day of `getLogs` over the active pool set) and confirm the plan tier.
- Before paying: verify on each pricing page the per-event cost for WebSocket pushes, and that Chainstack serves MegaETH mini-block `logs`.
- The engine needs a provider abstraction with failover per chain (primary → fallback), and the reconciler fills any gap left by a switch.
- Base: the raw Flashblocks feed carries receipts (logs) in its `metadata` object, which Base marks as unstable. **(verify)** that the provider's `newFlashblocks` subscription returns logs in a stable shape; if not, fall back to filtered `pendingLogs` (per-log billing) for Base.

---

## D17 — Dev and staging run on free resources where possible

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Every non-production environment (local dev, CI, staging, shadow-mode load tests) uses free resources by default: free RPC tiers and public endpoints, free service tiers, self-hosted open-source components (Kafka, ClickHouse, Postgres in containers), and local forks. A paid resource enters dev or staging only when a free option can't do the job, and the reason is recorded here.

**Rejected:**
- *Mirror production (paid) in staging.* Most realistic, but doubles the bill for an environment that mostly runs idle.

**Why:** Spend goes where it buys something: production reliability and the real-funds demos (D5). Free tiers' limits are also useful pressure: they force the batching (D13) and per-block streaming (D16) work to be done early.

**Consequence:**
- Config must make provider endpoints swappable per environment (free → paid is a config change, not code).
- Free tiers rate-limit and drop connections more often, so dev and staging exercise the failover and gap-fill paths (D10, D16) constantly. That's a feature, but it means flakiness there isn't automatically a bug.
- Load tests large enough to exceed free quotas are the likely first exception; they buy the D16 production plan early rather than a separate staging plan.

---

## D18 — Three prices; display price is the liquidity-weighted mid

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Each token has three distinct prices, each with one job.

| Price | Used for | Definition |
|---|---|---|
| **Display** | UI ticker, token pages, PnL marks, fair price (D7) | Liquidity-weighted mid across the token's active pools above the liquidity floor (D11). A single-pool token gets that pool's price. |
| **Trigger** | Stop-loss, take-profit, limit orders | The display price itself, evaluated instantly (D20). |
| **Execution** | What a trade actually gets | Router quote at the trade's size. Never a "price". |

Chart candles are built from actual swap prices (the universal convention), not from the mid. The token page also shows the main (deepest) pool's price so users comparing with DexScreener can see where any gap comes from.

**Rejected:**
- *Last trade price.* Flickers, and anyone can move it with one odd swap on a tiny pool.
- *Deepest pool only.* The industry default (Uniswap's subgraph, DexScreener-style pair pages) and identical to our choice for single-pool tokens. Rejected for multi-pool tokens because the price jumps when the deepest pool changes, unless we add switch hysteresis.
- *Volume- or time-weighted average (VWAP/TWAP).* Manipulation-resistant, as oracles and CoinGecko use, but lags by design and can't price a pool before it trades. May reappear inside the trigger price.
- *Median of pools.* Meaningless with one or two pools, which covers most memecoins.
- *Best executable quote.* Depends on size, and costs a routing pass per update at MegaETH rates.
- *One price for everything.* Perpetual exchanges split display (last) from risk (mark) for a reason: a display price that stop-losses fire on can be hunted.

**Why:** Fresh at millisecond speed, costs one in-memory recompute per pool update, smooth when liquidity moves between pools, and harder to move than any single pool.

**Consequence:**
- Every stop-loss explanation must be showable on the chart: the UI can draw the trigger price alongside the display price.
- Pools below the liquidity floor don't count toward the weighted mid. If a token has *no* pool above the floor, it is priced from its deepest pool and flagged **thin** in the UI (amended by D20, which keeps triggers working on new memecoins). → `pricing.md`

---

## D19 — USD conversion: fixed reference pools, stablecoins pinned unless they diverge

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Path to USD:** each chain has a small, fixed set of reference pools: the deepest native/stablecoin pools (e.g. WETH/USDC, WETH/USDT on Base and MegaETH; WBNB/USDT, WBNB/USDC on BNB). The native token's USD price is their liquidity-weighted mid (same method as D18). A token's USD price = its price in its quote asset × that quote asset's USD price.
- **Stablecoins:** pinned at $1 while the chain's reference stablecoins stay within ~0.5% of each other. If they diverge past that, the engine prices each stablecoin from its pools against the others, and the UI shows a depeg warning.

**Rejected:**
- *Best-path search per token.* Flexible, but slower, and every extra hop is another pool a manipulator can lean on.
- *Always pin at $1.* Simplest, and what most terminals appear to do, but during a depeg (USDC, March 2023) every price on the platform is quietly wrong.
- *Always float stablecoins.* Honest, but adds noise to every price for a case that is rare.

**Why:** A fixed reference set keeps conversion cheap (one multiply per update) and hard to manipulate, because reference pools are the deepest on the chain. The divergence check costs almost nothing and catches the rare depeg.

**Consequence:**
- Reference pool lists are per-chain config, reviewed when liquidity moves. The tokens that quote everything else (quote assets) are limited to the native token and the reference stablecoins in phase 1. A token paired only with some other token is unpriced until promoted into that set **(verify coverage on each chain)**.
- A change in native/USD reprices every token on the chain at once; the recompute path must handle that fan-out (→ recompute cadence question).

---

## D20 — Triggers: Trojan-style, instant on the display price

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Match what Trojan ships. Trigger orders evaluate the display price (D18) and fire the moment the level is hit, on provisional state, with no persistence window or smoothing.
- **Trigger types:** price, market cap (price × supply), or ± % change from entry; trailing stop-loss (% below the highest price since the order was created); optional expiry.
- **Execution guard:** each order carries a slippage limit (user-set, sensible default; Trojan defaults to 15%). That's the only protection between trigger and fill.
- **Thin tokens:** triggers work on tokens with no pool above the liquidity floor (priced from the deepest pool, flagged thin), because new memecoins are exactly where users set stops.

**Rejected:**
- *Persistence window (~300ms) by default, fast mode opt-in.* Harder to stop-hunt, but slower than the product we're modelling, and an extra concept to explain.
- *Smoothed "mark price" (perps style).* Lags in real crashes; users can't see why an order fired.
- *Confirmed blocks only.* Adds a full block (1–2s) and throws away the fast stream (D10).

**Why:** Parity with the reference product. Speed is what memecoin traders pay for, and they accept wick risk. The slippage limit bounds the damage of a bad fill.

**Consequence:**
- Stop hunting on thin pools is a known, accepted risk. The UI states that triggers fire on the live price.
- The ≤300ms price move → broadcast target (product.md) stands as written.
- Market-cap triggers need a supply figure per token (total supply at bootstrap, tracked via mint/burn if it changes). → `triggers.md`
- Trojan's event triggers (e.g. bonding-curve migration, dev sell) and scheduled orders are candidates for `triggers.md`, not decided here.
- Protected mode (persistence window) stays a possible later addition; the engine should keep the trigger rule pluggable per order.

---

## D21 — Quotes computed in memory; simulation only for opaque venues

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Every pool type we can model is quoted by in-memory math that reproduces the contract exactly, including its integer rounding. Simulation against chain state is used only for venues we can't model (opaque v4 hooks per D14, and later prop AMMs). A background **shadow check** samples live quotes, simulates the same swap on-chain, and alerts on any mismatch.

| Pool type | Chains | Math |
|---|---|---|
| Uniswap v2 + forks (PancakeSwap v2) | All | Constant product, fee on input |
| Uniswap v3 + forks (PancakeSwap v3) | All | Concentrated liquidity, tick walk |
| Uniswap v4 standard | All | v3 math, singleton PoolManager |
| Uniswap v4 hooks | All | Per hook: modelled, or opaque → simulated (D14) |
| Aerodrome volatile / stable | Base | Constant product / stable curve (x³y + y³x) |
| Aerodrome Slipstream | Base | v3-style concentrated liquidity |
| MegaETH venues | MegaETH | Kumbaya (largest by TVL) and Algebra-based pools **(verify which forks and fee models)** |

**Rejected:**
- *Simulate every quote.* Always exactly right, but an RPC round trip per quote (milliseconds, and D16 budget) where in-memory takes microseconds. Can't keep up with triggers and routing at MegaETH rates.

**Why:** Industry standard for terminals and routers. The router explores many route options per quote; only in-memory math makes that affordable.

**Consequence:**
- Each pool type needs a quoter whose results match the contract to the wei, tested against on-chain simulation (fork tests in CI; D17 keeps them free).
- Shadow-check mismatch rate is a monitored SLA line; a pool type that drifts is demoted to simulated until fixed.
- Fee-on-transfer and rebasing tokens break reserve math; they need detection (from the safety check) and quoting by simulation. → `routing.md`
