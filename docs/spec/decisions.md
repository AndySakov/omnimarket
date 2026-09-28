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
- **MegaETH** — real-time chain with ~10ms mini-blocks and ~1s EVM blocks (verified, MegaETH docs). Stress-tests ingestion throughput and what "confirmed" means.
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
| Base | `newFlashblocks` tick + one filtered `getLogs` (pending) per 200ms flashblock (amended, see consequences) | ~26M, fixed |
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
- Base (checked 2026-09-28, see [verification.md](verification.md)): receipts were removed from the Flashblocks WebSocket payload in Base's v1 upgrade, and an open issue asks to bring them back. So `newFlashblocks` is used as a **tick** only: on each flashblock, one filtered `eth_getLogs` at the `pending` tag fetches our events. Still fixed cost (~26M requests/month: tick + call). Fallback if that proves unreliable: filtered `pendingLogs` (per-log billing).

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
| PancakeSwap Infinity CL / bin pools (+ hooks) | BNB | CL: v4-like · bin pools: own math, to spec (added after verification) |
| Aerodrome volatile / stable | Base | Constant product / stable curve (x³y + y³x) |
| Aerodrome Slipstream | Base | v3-style concentrated liquidity |
| MegaETH venues | MegaETH | Kumbaya: concentrated-liquidity (v3-like) but with non-standard pool bytecode and unverified source, so **quoted by simulation until our math passes the shadow check**. Algebra-based pools: Algebra's own math (dynamic fees). |

**Rejected:**
- *Simulate every quote.* Always exactly right, but an RPC round trip per quote (milliseconds, and D16 budget) where in-memory takes microseconds. Can't keep up with triggers and routing at MegaETH rates.

**Why:** Industry standard for terminals and routers. The router explores many route options per quote; only in-memory math makes that affordable.

**Consequence:**
- Each pool type needs a quoter whose results match the contract to the wei, tested against on-chain simulation (fork tests in CI; D17 keeps them free).
- Shadow-check mismatch rate is a monitored SLA line; a pool type that drifts is demoted to simulated until fixed.
- Fee-on-transfer and rebasing tokens break reserve math; they need detection (from the safety check) and quoting by simulation. → `routing.md`

---

## D22 — Recompute cadence: prices and triggers on every update, screens throttled

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Token price and trigger evaluation:** on every pool update, no batching. Triggers are kept in levels sorted per token, so an update only checks orders between the old and new price.
- **Client pushes:** throttled per token (default ≤10/s, latest value wins). The ≤100ms tick → client target still holds.
- **Quote-asset moves (e.g. ETH/USD):** trigger levels are stored in the pool's quote asset. A quote-asset price change converts the USD levels at the new rate and checks only orders between the old and new converted boundary, instead of repricing every token and scanning every trigger.

**Rejected:**
- *Coalesce per mini-block / block.* Saves a little CPU, adds up to a block of delay to triggers (D20 is instant).
- *Push every update to clients.* Up to ~100 messages/s per token on MegaETH that nobody can read, multiplied by every subscriber.
- *Eagerly reprice all tokens on a quote-asset move.* Thousands of recomputes and trigger scans per ETH tick, almost all of which fire nothing.

**Why:** Spend work only where speed changes an outcome (triggers), and cap it where it doesn't (human eyes).

**Consequence:**
- Trigger index is keyed by (token, quote asset, level) with a USD view derived from quote-asset price. → `triggers.md`
- Display-price consumers that need USD (UI, PnL) compute it lazily from quote price × quote-asset USD price.

---

## D23 — Fair price: display price, or cross-chain weighted for a curated asset list

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- A single-chain token's fair price is its display price (D18).
- Assets on several chains (phase 1 list: ETH/WETH, BNB/WBNB, USDC, USDT, wrapped BTC) get a liquidity-weighted average of their per-chain display prices, weighted by ±2% depth (D24).
- Which contracts are "the same asset" comes from a hand-maintained address map, never from names or symbols.
- A small aggregator consumes each chain's price updates from Kafka and publishes the fair price.

**Rejected:**
- *Match assets by symbol.* Trivially spoofed by copycat tokens.
- *Aggregator inside each Chain Engine (cross-engine calls).* Couples engines that D6 keeps independent, for consumers (UI, PnL) that tolerate milliseconds of Kafka lag.

**Why:** Covers D7's phase 1 requirement with the smallest possible surface. Nearly every token users trade is single-chain.

**Consequence:** Phase 2's prop AMM needs a lower-latency fair-price feed than Kafka; that design belongs to phase 2 (D7: phase 1 does no phase 2 work).

---

## D24 — Liquidity measure: ±2% depth in USD

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** A pool's liquidity is its **±2% depth**: the USD value that can be traded before its price moves 2% (buy side + sell side). Used for display-price weighting (D18), fair-price weighting (D23), and the D11 liquidity floor.

**Rejected:**
- *Total value locked.* Counts v3/v4 liquidity parked far from the current price, which does nothing for trades today.
- *Active-tick liquidity only.* Too narrow: one tick can be empty while the next is deep.

**Why:** The standard depth measure on crypto data sites (e.g. CoinGecko's ±2% order-book depth). Works identically across v2, v3, v4 and Aerodrome, so pools of different types compare fairly.

**Consequence:** Computed in memory from reserves or ticks, refreshed on mint/burn and whenever price crosses a tick. D11's liquidity floor is expressed in ±2% depth per chain (value still TBD in tuning).

---

## D25 — Routing: adaptive hybrid, chosen per order from situational cues

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** The router can produce three route shapes (single pool, multi-hop, and split across pools) and picks per order. Everything is quoted in memory (D21), so the router prices the candidates and chooses by:

> **score = output − extra gas − Σ risk penalty per extra pool**

The risk penalty is set by cues inferred from the order, the market, the pools, and our own order flow. Limits: at most 2 hops, intermediate tokens only from the D19 quote-asset set, at most 3 pools in a split, split search in 5% chunks.

**Phase 1 cues:**
1. **Size vs depth.** Trade < ~1% of the best pool's ±2% depth → single pool; skip the split search.
2. **Order origin.** Stop-loss/trailing: reliability first (fewest pools, no simulated venues). Manual: default. Take-profit/limit: price first. New-pair buy: single pool, speed. Copy trade: the leader's pool where possible.
5. **Liquidity concentration.** One pool holds >90% of depth → skip the split search.
8. **Venue trust.** Penalise or exclude opaque (simulated) venues, very new pools, and fee-on-transfer tokens; excluded outright for urgent orders.
11. **Own-flow awareness.** Our in-flight orders are applied to the in-memory pool state before quoting the next order, so copy-trade fan-out and stop cascades see realistic prices and spread across pools.

**Later cues:** (3) user's slippage setting as an urgency signal, (4) pool heat (recent update rate), (6) live gas price, (7) chain MEV profile (splits reduce sandwich profit on BNB's public mempool), (9) state confidence (provisional or just-reorged pools), (10) recent revert history per pool.

**Tuning:** every routing decision and its outcome (quoted vs filled, reverts) is logged to ClickHouse; shadow mode (D5) replays the same order flow under different penalties to compare.

**Rejected:**
- *Single pool only (Trojan-style).* Simplest and fastest, but loses price on large trades in multi-pool tokens.
- *Full aggregator search (1inch/0x/Odos-style).* Best price on large or unusual trades, but the search is too costly at MegaETH update rates, and long exotic paths are where traps hide.
- *One fixed shape for every order.* A stop-loss in a crash and a patient limit buy want opposite trade-offs.

**Why:** Most trades are small and get the fast single-pool path automatically, as on Trojan. Large or patient orders get aggregator-quality prices. Own-flow awareness is something an outside aggregator can't do, because only we see our order flow.

**Consequence:**
- Splits across different DEXes need our own router contract. → routing question 3.
- The in-flight order overlay needs execution (D8) to report submitted, landed and failed orders back to the engine promptly, so the overlay is removed when trades land or fail.

---

## D26 — Our own router contract, immutable, approvals via Permit2

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Every swap goes through our own router contract, deployed on all three chains.
- **One call per route:** executes any D25 route (single, multi-hop, split across DEXes), enforces `minOut` and deadline, and takes the platform fee in the same transaction.
- **Holds nothing:** no funds between transactions; every call must end with a zero balance, or it reverts.
- **Immutable:** no admin keys, no upgrade proxy. A new version is a new deployment.
- **Approvals via Permit2:** users approve Uniswap's Permit2 once per token; each trade carries a signed, exact-amount, short-lived permit for our router. Native-token buys need no approval. *(Amended by D31: sells use a per-position, capped, 7-day allowance instead.)*
- **Same address everywhere:** deployed via CREATE2 so the router has one address on every chain.

**Rejected:**
- *DEX routers only.* No contract risk, but no cross-DEX splits (D25), and fees need a separate transfer.
- *Direct unlimited approvals to our router.* One approve per token and no signing per trade, but a router bug could then drain every approved balance.
- *Upgradeable proxy.* Easy fixes, but an admin key that can change the code holding approvals is the biggest target in the system.

**Why:** Industry norm for EVM trading bots (Maestro, Banana Gun, Sigma use their own routers), and required for cross-DEX splits. Permit2 limits exposure to the amount and time window of each trade. Because approvals point at Permit2, not the router, shipping a new router version needs no re-approvals.

**Consequence:**
- Contract work enters phase 1: Solidity router with fork tests per DEX type, fuzzing, and invariant tests (zero residual balance, `minOut` always enforced).
- Permit2 is deployed at its canonical address on MegaETH, Base and BNB (verified).
- Signing a permit adds a signature per trade; with Privy delegated signing (D4) that's on the hot path, so its latency needs measuring. → `execution.md`
- Fee design (rate, taken in input or output token) → routing open question.

---

## D27 — Quotes rebuilt at send time; slippage defaults set by situation

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Quote lifetime:** a displayed quote is never executed. The route is re-quoted at the moment of sending, and `minOut = fresh quote × (1 − slippage)`. If the fresh quote is already worse than the displayed one by more than the user's slippage, the trade is not sent; the user sees the new quote instead. Trigger orders quote at firing time.
- **Slippage defaults by situation** (always user-adjustable; starting values, tuned via the D25 logging loop):

| Situation | Default |
|---|---|
| New pair (inside D11 grace window) or thin token (D20) | 15% |
| Established token (pool above liquidity floor) | 3% |
| Major / stable pair (quote-asset set, D19) | 0.5% |
| Stop-loss / trailing stop sell | The row's value × 2 (landing matters more than price) |
| Fee-on-transfer token | + the detected tax |

**Rejected:**
- *Trojan's flat 15%.* Right for sniping, but generous everywhere else. On BNB's public mempool, sandwich bots can take up to the full allowance.
- *Execute the displayed quote.* Prices move every 10–200ms; a quote seen on screen is already stale when the user clicks.

**Why:** Slippage is both a fill guarantee and the amount a sandwich bot can take. The right trade-off depends on how fast the market is moving and how much the order needs to land, and we already know both from the D25 cues.

**Consequence:**
- The UI shows the default chosen and why ("new pair: 15%").
- Tax detection (fee-on-transfer) must run before routing. → token safety checks, routing open question.

---

## D28 — Fees: Trojan's 1%, taken in the native/quote asset

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Mirror Trojan: **1% per successful trade** (0.9% with a referral). The router contract (D26) takes the fee in the same transaction, always in the native or quote asset (ETH, BNB, stablecoin):
- **Buys:** taken from what the user pays, before the swap.
- **Sells:** taken from the native/quote proceeds, after the swap.

Quotes, `minOut` and the UI show amounts net of the fee.

**Rejected:**
- *Take the fee from the token on sells* (literally "on pay" for both sides). Leaves the treasury holding memecoins that must be sold later (extra price impact, possible transfer taxes, honeypot risk), and adds a token transfer per sell.
- *Fee on output for buys.* Same problem: the fee would be in the memecoin.

**Why:** Parity with the reference product. Taking fees only in native/quote assets keeps the treasury clean and the router simple.

**Consequence:** Referral tiers (Trojan has multi-level referrals) are a product feature for later; the router takes a fee rate per trade so referral discounts need no contract change.

---

## D29 — Token safety: simulate, inspect, and watch; block only confirmed honeypots

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Four layers of checks, results cached per token and shown as badges.

| Layer | What | How | When |
|---|---|---|---|
| 1. Round-trip simulation | Honeypot (can't sell), buy tax, sell tax, max-tx / max-wallet limits | One `eth_call` with a state override: a simulator contract injected at a throwaway address, funded with native coin, buys then sells against the live pool | New pool discovered; pool promoted (D11); every few minutes while active; immediately on a behavioural alarm |
| 2. Contract inspection | Owner not renounced; mint, blacklist, pause, set-fee / set-tax, max-tx functions; upgradeable proxy | Bytecode function-selector scan + owner read | Once per token, again on ownership change |
| 3. Liquidity safety | LP burned or locked (v2), deployer-owned share of liquidity (v3/v4), pool age | Reads from state the engine already holds | On pool discovery and liquidity events |
| 4. Behavioural signals | Sells stop succeeding while buys continue; realised tax drifts from simulated; sudden liquidity pull | Derived from the swap and transfer stream we already index | Continuously, free |

**Policy:**
- **Confirmed honeypot** (buy simulates, sell reverts or returns dust): **buys blocked**, no override.
- **Everything else:** warn with badges (Trojan-style), never block.
- **Sells are never blocked** by our checks: a user must always be able to try to exit.
- Measured taxes feed slippage defaults (D27) and switch the token to simulated quoting (D21).
- **Optional second opinion:** GoPlus Security API (free, 30 calls/min) for Base and BNB, asynchronously, never on the trade path. GoPlus doesn't list MegaETH; there, the Etherscan API (chain ID 4326) supplies verified-source checks instead.

**Rejected:**
- *Static analysis only.* Misses honeypots whose sell-block only triggers at runtime.
- *Third-party API as the primary check.* Rate limits, added latency, and chain coverage we don't control.
- *Simulate before every trade.* Adds an RPC round trip to the hot path; the router's `minOut` (D26) already protects each trade, and behavioural signals catch changes between re-checks.
- *Block all risky tokens.* Most memecoins have some red flag; blocking would empty the product.

**Why:** Simulation is the only check that catches runtime traps, and state overrides make it free of deployments and gas. Behavioural signals turn data the indexer already has into a continuous safety monitor.

**Consequence:**
- `eth_call` state overrides: supported by the node software on all three chains and documented for MegaETH (QuickNode also documents `eth_simulateV1`, which simulates a buy and sell as two real transactions without an injected contract). Chainstack support to confirm with the first test call.
- Simulation calls count toward the RPC budget (D16); the re-check interval is a tuning parameter.
- Holder concentration (top-10 share, deployer balance) needs a holder index or GoPlus; deferred.
- Token taxes can change at any block; badges show when each check last ran.

---

## D30 — Transaction submission: private fan-out on BNB, parallel providers elsewhere

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**

| Chain | Submission path | Why |
|---|---|---|
| BNB | Same signed transaction sent in parallel to 3–4 private builder RPCs (e.g. 48 Club, PancakeSwap MEV Guard, bloXroute, Blockrazor). Never the public mempool. | Public mempool = sandwiches. Several private builders together cover most block production, so inclusion stays fast. |
| Base | Primary and fallback provider in parallel; priority fee set by situation | Single sequencer, no public mempool; ordering is by priority fee within each 200ms flashblock. Redundancy covers provider hiccups. |
| MegaETH | `realtime_sendRawTransaction` via primary, fallback in parallel | Returns the receipt in the same call (~10ms). |

**Rejected:**
- *Public mempool on BNB.* Widest reach, but every trade becomes sandwich food up to its full slippage.
- *A single private RPC on BNB.* Private, but inclusion depends on one builder network winning the block.

**Why:** Fan-out of an identical signed transaction is safe (one nonce, so it can land only once) and buys both privacy and inclusion speed. Industry bots (Maestro, Banana Gun, Sigma) offer the same "anti-MEV" routing on BNB.

**Consequence:**
- On BNB, D27's slippage stops being a budget for sandwich bots.
- Builder RPC list per chain is config; inclusion latency per builder is logged to choose and prune the set.
- On Base, flashblock visibility still lets bots react one flashblock later (backruns, snipes); priority fee is our lever there. → gas policy question.

---

## D31 — One signature on the hot path: per-position Permit2 allowances + fire-ready orders

**Date:** 2026-09-28 · **Status:** Decided (amends D26)

**Decision:**
- **Per-position allowance.** When a buy lands, the execution service signs, off the hot path, a Permit2 *allowance* for the router: that token only, capped at the position size, expiring in 7 days, renewed in the background while the position is open. Every later sell, stop-loss or take-profit needs only the transaction signature.
- **Caller check.** The router spends a wallet's allowance only in a transaction sent by that wallet (`owner == msg.sender`). No one else can trigger it, even through a router bug.
- **Fire-ready orders.** For each armed trigger, the transaction data layout, gas estimate and allowance are prepared ahead of time. At fire time only the fresh quote (D27), the nonce and one signature remain. The nonce comes from the execution service's in-memory counter (D8 makes it the sole sender), not an RPC call. Nonces are **not** reserved per order: an unused reserved nonce would block every later transaction from the wallet.
- Token-funded buys (e.g. paying in USDC) keep a per-trade permit unless a standing allowance for that token already exists.

**Rejected:**
- *Per-trade permit on every spend (D26 as written).* Two sequential Privy signatures (~20–100ms each) on sells, the most urgent orders.
- *Pre-signed permits per trigger order.* Covers triggers but not manual sells; long-dated permits amount to allowances with more bookkeeping.
- *Reserve a nonce per armed order.* Freezes the wallet if the order never fires.

**Why:** Halves hot-path signing for the orders that most need speed, while keeping exposure bounded: one token, one position's size, one week, and only spendable by the wallet's own transaction.

**Consequence:**
- D26's "no standing permission" becomes "no *open-ended* permission".
- Measure Privy signing latency per region early; it's the largest unknown in the 300ms trigger budget.
- Allowance renewals and revocations (position closed → allowance set to zero) are background jobs with their own signing budget.

---

## D32 — Nonces: per-wallet sequencer, durable nonce ledger, gap watchdog

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Per-wallet sequencer.** Inside each chain's execution service (the sole sender, D8), every wallet has a queue that assigns nonces strictly in order from an in-memory counter.
- **Durable nonce ledger.** Every assigned nonce is recorded with its status: `assigned → signed → submitted → landed | replaced | filled`. Status writes are compare-and-set, and terminal states are never overwritten. The ledger is what a restarted instance or the standby recovers from.
- **Gap watchdog.** A nonce submitted but not landed within a few blocks is re-sent with the same nonce and a higher fee. If its purpose has gone stale (e.g. its quote expired), it is replaced by a **filler**: a 0-value transfer to self that uses up the nonce so later transactions can land.
- **Re-sync.** On startup, failover, or any "nonce too low / too high" error, the wallet's count is re-read from the chain (`pending` tag) and reconciled against the ledger before anything else is sent.
- **In-flight cap.** At most 5–10 unlanded transactions per wallet, so one stuck transaction can't strand a long queue.

**Rejected:**
- *Ask the RPC node for the nonce each time.* A round trip per trade, and wrong as soon as two transactions are in flight.
- *Reserve nonces per armed order.* Freezes the wallet if the order never fires (D31).
- *In-memory counter only.* Fast, but a crash or failover loses track of what was sent.

**Why:** The counter keeps the hot path free of RPC calls; the ledger makes it survive crashes and failover; the watchdog keeps a dropped transaction from blocking a wallet.

**Consequence:**
- Where the ledger lives (Postgres, per chain) and its write latency on the hot path → `data.md`. Writing `assigned` must not add a network round trip before signing (write-behind, recovered by chain re-sync if lost).
- Fillers and re-sends are sent from the user's wallet, so their gas comes from the user's native balance (the platform can't pay gas for a wallet it doesn't own without sponsorship). Logged per chain; reimbursing users is a product choice.
- Local forks (D5) test nonce gaps, drops, and failover explicitly.

---

## D33 — Priority fees by situation, tracking live tips per chain

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** The priority fee (tip) is chosen per transaction from the order's situation (D25 cues). Levels are relative to recently landed tips on that chain, not fixed numbers.

| Situation | Tip level |
|---|---|
| New-pair buy (sniping) | Aggressive |
| Stop-loss / trailing stop | High |
| Manual buy / sell | Medium: recent median + margin |
| Take-profit / limit order | Low |
| Gap-watchdog re-send (D32) | Previous tip + 25% |

- **Cap:** a per-trade maximum fee as a share of trade value, so a fee spike can't eat a small trade.
- **Override:** users can set the level per order (Trojan-style).
- **Who pays:** all gas comes from the sending wallet, i.e. the user's.

**Rejected:**
- *One fixed tip per chain.* Overpays on patient orders and underpays on urgent ones.
- *Always maximum.* Wastes user money where position in the block doesn't matter.

**Why:** On Base the sequencer orders each flashblock by tip; on BNB private builders favour higher payers. Tip is the only lever for position within a block, and its value depends on urgency, which we already infer.

**Consequence:**
- Each execution service keeps a rolling view of landed tips per chain (from the blocks the indexer already reads).
- On MegaETH, fees are tiny and tips rarely change ordering; levels are kept for consistency.

---

## D34 — Trade tracking: simulate while signing, detect landing from the indexer, retry by order type

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Lifecycle:** `signed → submitted → preconfirmed → confirmed → final`, or `reverted | dropped | replaced`. Tracked per trade, above the nonce ledger (D32).
- **Simulate in parallel with signing.** The unsigned transaction is simulated while Privy signs it; hot-path cost is the slower of the two, not the sum. A failing simulation cancels the send.
- **Landing detection from our own indexer.** Our swaps appear in the pool events the engine already streams; matching by transaction hash gives preconfirmation in 10–200ms at no extra RPC cost. MegaETH's `realtime_sendRawTransaction` returns the receipt directly.
- **Reorgs:** a landed trade reorged out returns to `submitted`; the D32 watchdog handles it if it doesn't re-land.
- **Reverts** are diagnosed (price past `minOut`, deadline, tax change, allowance expired, insufficient balance) and handled by order type:

| Order type | On revert |
|---|---|
| Stop-loss / trailing | Auto-retry with a fresh quote, up to 3 times |
| Take-profit / limit | Re-arm; fires again if the level still holds |
| Manual | No auto-retry; tell the user why, show a fresh quote |
| Copy trade | One retry, then skip and notify |

**Rejected:**
- *Simulate, then sign (sequential).* Adds a full simulation round trip to every trade.
- *Poll receipts per transaction.* An RPC call per trade per poll, for information the indexer already has.
- *One retry policy for all orders.* A stop-loss must get out; a manual trade must not surprise the user.

**Why:** Keeps the hot path to one signature plus submission, reuses data we already ingest, and matches retry behaviour to what each order is for.

**Consequence:** The engine keeps a set of our pending transaction hashes per chain to match against incoming events; outcomes go to Kafka and user notifications.

---

## D35 — Exactly-once trigger firing: deterministic firing IDs, dedupe in execution, orders in Postgres

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
1. **Deterministic firing ID** = hash(order ID, per-order firing count). The main engine and the standby compute the same ID for the same firing.
2. **Execution dedupes by firing ID**, stored when the firing is accepted (alongside the nonce ledger, D32). A repeat gets "already handled", not a second trade.
3. **Engine retries until acknowledged.** Always safe because of 2.
4. **One firing per order at a time.** A fired order is `firing` until execution reports the outcome; further price crossings are ignored until then.
5. **Orders are durable in Postgres.** The engine's in-memory trigger index (D22) is a cache rebuilt from Postgres on startup and failover.

**Rejected:**
- *At-most-once (fire and forget).* A lost call means a stop-loss that never fires.
- *Random firing IDs.* The standby would generate different IDs and double-fire after failover.
- *Orders only in engine memory.* A crash loses every armed order.

**Why:** Turns "exactly once" into two simple rules: retries are always safe, and duplicates are always recognised.

**Consequence:** Closes the architecture open questions on exactly-once firing and on where trigger orders live durably. Postgres schema for orders and firings → `data.md`.

---

## D36 — Bonding curves are a phase 1 venue type (four.meme first)

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Launchpad bonding curves join DEX pools as a venue type. Phase 1 covers **four.meme** on BNB; MegaETH's Kumbaya launchpad mechanics **(verify)**. Base's main launchpads (Clanker, Zora, Flaunch) launch directly into Uniswap v4 hook pools, already covered by D14.
- **Indexer:** discovers curve tokens from the launchpad's create events and tracks curve state like any pool (bootstrap read + events).
- **Pricing / quoting:** the curve formula in memory (D21), shadow-checked like other venues.
- **Routing:** swaps go through the launchpad contract via our router (D26).
- **Safety:** same round-trip simulation (D29).
- **Migration:** a graduation event marks the curve closed and promotes the new PancakeSwap pool to active immediately (D11), with no gap in pricing.

**Rejected:**
- *DEX pools only.* Misses BNB's newest memecoins during their most active phase, and leaves the migration trigger nothing to watch.

**Why:** On BNB, "new pair" mostly means "new four.meme curve". Trojan's migration trigger and the new-pairs feed both depend on it.

**Consequence:** Launch-phase fees on Base v4 hooks (e.g. Zora's 99% → 1% over ten seconds) must be modelled exactly or quoted by simulation; sniping into a decaying fee is a real user risk and the quote must show it.

---

## D37 — Trigger order catalogue: Trojan parity

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Phase 1 ships the full Trojan-style order set.

| Type | Behaviour |
|---|---|
| Limit buy / sell | At a price, market cap, or % change (D20) |
| Stop-loss / take-profit | % from the position's entry |
| Multi-level take-profit | Several levels, each selling a share (e.g. 50% at 2×, 25% at 5×) |
| Trailing stop | % below the highest price since the order was created |
| Auto-sell on buy | Each buy arms preset TP/SL orders |
| Expiry | Optional on any order |
| Event: dev sell | Fires when the token's dev wallet sells |
| Event: migration | Fires when a bonding-curve token graduates (D36) |
| Scheduled | Fires at a set time |

- **Entry price:** positions keep a cost basis (average entry) from their own fills; "% from entry" orders use it.
- **Dev wallet:** recorded per token at discovery (deployer, or the creator the launchpad records). Dev sells are detected from the swap stream the indexer already has.
- **Market cap:** price × supply. Supply read at discovery; tokens with mint/burn powers (D29) track it from mint/burn events.
- **Evaluation:** price-level orders use the sorted index in quote-asset units (D22); event orders are keyed by (token, event type); scheduled orders by time. All fire through the exactly-once path (D35).

**Rejected:**
- *Price orders only.* Event and scheduled triggers are part of the reference product and cheap given the data we already index.

**Why:** Parity with the reference product; every trigger reuses data and machinery already specced.

**Consequence:** Multi-level TP means one position can own several armed orders; fills update the position and cancel or resize siblings. → `triggers.md`

---

## D38 — Copy trading from our own indexer, never from the mempool

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Detection:** leader swaps are matched in the pool and curve events we already stream. On Base and MegaETH the copy can land one flashblock / mini-block after the leader; on BNB (private submission) we see the leader's trade when its block lands.
- **Settings (per follow):** fixed or proportional size, max per trade, token filters (min liquidity, safety badges), buy-only or mirror sells, auto TP/SL on copied buys (D37).
- **Fan-out:** many followers of one leader are routed with own-flow awareness (D25), so later copies see realistic prices and spread across pools.
- **Never from the mempool:** no copying of pending transactions.

**Rejected:**
- *Copy from BNB's public mempool.* Faster, but it is frontrunning the leader, and private submission hides most leaders anyway.
- *Poll leader wallets via RPC.* Slower and costs RPC calls for data the indexer already has.

**Why:** Fastest honest signal available, at no extra ingestion cost.

**Consequence:** Followed wallets are a D11 promotion reason: any pool a followed wallet trades is activated immediately, so its state is ready before the copies are routed.

---

## D39 — Trigger mechanics: take-profit sizing, trailing-stop recovery, limits and cascades

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Multi-level take-profit:** each level sells a % of the **original** position, capped at what is held when it fires. When the position empties (manual sell-all or a stop-loss), all sibling orders are cancelled. A stop-loss always sells everything that remains.
- **Trailing-stop high-water mark:** persisted write-behind whenever it rises by more than ~0.5%. On recovery: the higher of the saved value and the max price since that save, from ClickHouse 1s candles.
- **Limits:** 200 active orders per user, 20 per token per user. 100k concurrent orders (product.md) is ~10MB of index per chain.
- **Cascades:** every crossed order fires; nothing is throttled. Firings enter a priority queue: stop-loss and trailing first, then take-profit and limit, then scheduled. Own-flow awareness (D25) spreads the trades across pools with realistic prices.

**Rejected:**
- *TP levels as % of the remaining position.* "25% at 5×" would shrink after every earlier level, which isn't what users mean.
- *Persist the high-water mark on every tick.* A write per price move for data that can be rebuilt from candles.
- *Throttle firings in a cascade.* Delaying a stop-loss is worse than the extra load.

**Why:** Matches how traders think about their orders, survives failover without per-tick writes, and keeps urgent exits first under load.

**Consequence:** The 1s candle table becomes part of engine recovery, so it needs to be complete up to at least the last trailing-stop save. → `data.md`

---

## D40 — Engine recovery: snapshot + Kafka replay; standby fed from Kafka, lease + fencing

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**

**Recovery (target: serving again in < 10s)**
1. Every ~30s, a background snapshot of all active pool state (reserves, ticks, liquidity), stamped with the block it reflects.
2. On restart: load the latest snapshot at block B, then replay pool updates published to Kafka since B. Each carries its after-state (D12), so replay is value application: no RPC, no recompute.
3. Resubscribe to the fast streams; the reconciler fills the last few blocks via `getLogs`.
4. Orders reload from Postgres (D35); trailing highs recover per D39.

**Standby (failover ≈ 3–5s detect + ~1s catch-up)**
- **Fed from Kafka:** the standby applies the primary's published pool updates, staying milliseconds behind with identical state and no extra RPC cost. On takeover it opens its own subscriptions and the reconciler closes the gap.
- **Leader lease:** only the holder of a short Kubernetes lease (renewed ~1s) may fire triggers.
- **Fencing token:** each lease carries an increasing epoch; every firing includes it; execution rejects firings from an older epoch, so a primary that stalls and wakes after losing the lease can't fire.
- **Overlap:** deterministic firing IDs (D35) mean any firing both engines emit near the switch executes once.

**Rejected:**
- *Standby with its own subscriptions.* Fully independent, but doubles pushed-event cost (D16) and can drift from the primary.
- *Cold standby (start on failure).* Recovery time instead of failover time.
- *Replay from the chain only.* Slower and RPC-heavy; Kafka already holds the after-states.
- *Lease without fencing.* A paused process can still act after losing leadership.

**Why:** Reuses what exists (Kafka's after-states, deterministic firing IDs, Postgres orders) so recovery and failover add almost no new machinery.

**Consequence:**
- Snapshot storage and format (versioned, compressed) → `data.md`.
- Kafka pool-update retention must cover at least the snapshot interval plus recovery time (already exceeded by D12's finality requirement).
- Failover drills (kill the primary under load) belong in `loadtest.md`.
