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

*(Amended by D64 and D69: BNB is the second chain, ahead of Base depth; MegaETH is deprioritised until it has real volume.)*

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
- *Smart accounts with session keys (ERC-4337 / EIP-7702).* Promising and elegant, but not yet what terminals ship, and support varies across our three chains. Kept as a stretch comparison. *(Revisited in D47: EIP-7702 is now live on all three chains; still set aside for blast radius and latency.)*

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

*(Amended by D63: the "≈1 month" estimate for phase 1 is withdrawn; no timeline commitment.)*

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

*(Amended by D42: the service owns executor wallets and their nonces; user wallets sign intents rather than transactions.)*

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

*(Amended by D84: chains followed by canonical blocks (Base, BNB) have no separate reconciler loop.)*

*(Amended by D16 and the verification pass: Base's fast loop uses a Flashblocks tick + pending `getLogs`; BNB uses `newHeads` + `getLogs`.)*

*(Amended by D77: Base follows canonical blocks only, like BNB. No Flashblocks feed.)*

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

*(Note from M1: v2 pairs need no state read at all, since `Sync` carries full reserves; their batched read is `token0`/`token1` for the CREATE2 proof (D80). See `indexer.md`.)*

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

*(Amended by D44: production also runs our own Base node.)*

*(Amended by D77: Base's fast stream is `newHeads` + one `getLogs` per 2s block, ~2.6M requests/month; the Flashblocks tick and its ~26M are dropped.)*

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

*(Amended by D77: triggers fire on canonical blocks, not provisional state. The rest stands.)*

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

*(Amended by D43: client pushes use a leading-edge throttle at 20/s.)*

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

*(Amended by D58: each intent names who may submit it, our executor set or the user.)*

*(Amended by D42: the router executes signed intents submitted by our executor wallets.)*

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

*(Amended by D59: pre-signed intents get fresh-quote protection through a submitter-tightened minimum.)*

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

*(Amended by D68, then D85: the per-trade fee stays; referral tiers and cashback are out of scope.)*

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

*(Superseded by D42: intents replace per-position allowances; fire-ready preparation carries over.)*

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

*(Amended by D42: sequential nonces now belong only to our executor wallets; user intents use Permit2's unordered nonces.)*

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

*(Amended by D42: gas is paid by executor wallets and recovered from the trade.)*

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

*(Amended by D60: stop-loss retries at a fresh quote use the exit guarantee's server re-sign.)*

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

*(Amended by D61: the firing-ID dedupe lives in execution's regional Postgres.)*

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

---

## D41 — Data layout: Postgres for money state, ClickHouse for history, Kafka topics, object storage for snapshots

*(Amended by D61: execution's nonce ledger and firing dedupe move to a regional Postgres per chain. Amended by D75: RustFS replaces MinIO.)*

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**

| Store | Holds |
|---|---|
| **Postgres** (one cluster, primary + read replica, tables keyed by chain) | Users, wallets, follows; orders and firings (D35); nonce ledger (D32); positions and cost basis (D37); token metadata: dev wallet, supply, safety results (D29); trailing highs (D39) |
| **ClickHouse** | Swaps, pool events, candles (1s / 1m / 5m / 1h), 30-day backfill (D15), routing decision logs (D25), execution outcomes |
| **Kafka** | Everything engines and execution publish (D6) |
| **Object storage** (MinIO in dev/staging, D17) | Engine snapshots (D40): versioned, compressed, last 10 per chain |

**ClickHouse reorg handling:** rows keyed by (chain, block hash, log index), so re-inserts are harmless. Each row has a status (provisional → confirmed → final → or removed) and a version; `ReplacingMergeTree` keeps the latest. Candles are built live by a candle service reading swaps from Kafka, stored in ClickHouse, and rebuilt for any window a correction touches.

**Kafka topics** (Protobuf, schemas versioned in the repo, no registry service):

| Topic | Key | Retention |
|---|---|---|
| `pool-updates.<chain>` (before + after) | pool | ≥ 24h |
| `swaps.<chain>` | pool | 7 days |
| `prices.<chain>` | token | 24h |
| `corrections.<chain>` | block | 7 days |
| `executions.<chain>` | wallet | 30 days |

**Retention:** candles forever; raw swaps from the 30-day backfill onward, with downsampling past 90 days an option if storage cost grows.

**Rejected:**
- *One Postgres per chain.* Users hold wallets on all three chains; splitting scatters one user's data.
- *Delete-and-rewrite on reorg in ClickHouse.* Mutations are expensive there; versioned inserts are the idiomatic path.
- *Schema registry service.* One more thing to run; schemas in the repo give the same compatibility checks in CI.

**Why:** Each store does what it's good at. The hot path never waits on any of them: engines and execution hold what they need in memory and write in the background.

**Consequence:** Postgres and ClickHouse schemas are written when we switch to build mode; this decision fixes ownership and keys, not columns.

---

## D42 — Intent-based execution: users sign intents, our executor wallets submit

*(Amended by D59: intents carry a maximum amount and a minimum rate.)*

*(Amended by D57: intents are signed in the user's own session when they're present; server signing only for absent flows. Amended by D58: submitter field.)*

**Date:** 2026-09-28 · **Status:** Decided (supersedes D31; amends D26, D32, D33)

**Decision:**
- **User wallets never send trades.** Each trade is a signed **intent** (a Permit2 witness transfer): exact input token and amount, output token, minimum output, deadline, recipient = the user's own wallet. One Privy signature per trade, whatever the payment token.
- **Trigger intents are signed when the order is created.** A stop-loss is "sell exactly N for at least (stop price − slippage), valid until the order's expiry". When it fires, no user signature is needed.
- **Executor wallets submit.** A pool of executor wallets per chain, keys held by our own `Signer` (local encrypted keystore / KMS, D4), signs and sends the transaction in-process (< 1ms). Firings are spread across executors, so a cascade isn't serialised behind one nonce sequence.
- **The router verifies the intent** (signature, terms, deadline, Permit2 unordered nonce), pulls exactly the signed amount, swaps along the route the executor supplies, enforces `minOut`, and sends the output to the user.
- **Gas** is paid by the executor and recovered in the same transaction from the trade (alongside the D28 fee). Users never need to hold ETH/BNB for gas.
- **Wrapped native balances.** Permit2 can't move native coin, so deposits of ETH/BNB are auto-wrapped to WETH/WBNB (a background transaction from the user's wallet). Sells can unwrap on output if the user wants native.
- **Permit2 approval per token** is sent from the user's wallet in the background: for WETH/WBNB and stablecoins at wallet setup, and for each new token right after the buy lands (so auto-armed TP/SL are live within about one block).
- **Copy trades** can't be pre-signed (the amount is unknown until the leader trades) and take one Privy signature at copy time.
- **Privy policy** is narrowed to signing Permit2 intents for our router (plus the background approve/wrap transactions).

**Rejected:**
- *Per-position standing allowances (D31).* One signature for sells, but token-paid buys still need two, and allowances stay open for days.
- *EIP-7702 delegation.* Removes permits, but hands broad power to delegated code; smart accounts were already set aside in D3.
- *Standing allowance to executors without intents.* No per-trade user signature at all, but a compromised executor key could then trade user funds at any price.

**Why:**
- Trigger path loses the Privy round trip entirely (~130ms → ~15–30ms internal).
- Every trade is one signature; no standing allowances; no per-user nonce gaps.
- Executor keys hold only gas money: with them an attacker can execute only intents users already signed, on the signed terms.
- Same model as UniswapX, CoW Swap and 1inch Fusion; unlike Trojan or Maestro, whose user wallets send transactions themselves.

**Consequence:**
- D32's nonce ledger and gap watchdog now manage executor wallets only; executors are funded from treasury and topped up automatically.
- D33's tip policy is unchanged, but the executor pays and recovers it.
- The executor can technically fire a trigger early, never below the signed minimum; the same trust users already place in delegated signing. Every firing is logged with the price that crossed.
- Router contract grows intent verification: more fuzz and invariant tests (output always to the signer, never more than the signed amount pulled, deadline enforced).
- `triggers.md`: arming an order includes signing its intent; editing an order re-signs.

---

## D43 — Free latency levers: co-location per chain, persistent submission, faster screen ticks

**Date:** 2026-09-28 · **Status:** Decided (amends D22)

**Decision:**
- **Co-location:** each chain's engine and execution run in the cloud region nearest that chain's sequencer or builders (engines are already per chain, D6). Execution also sits close to Privy's nearest signing region.
- **Persistent submission:** transactions are sent over already-open WebSocket connections to every endpoint at once (first wins), not new HTTP requests. On Base, also directly to the sequencer's endpoint **(verify)**.
- **Warm connections** to Privy, providers and builders; no per-request TLS or DNS.
- **Screen ticks:** leading-edge throttle at 20/s per token, with delta encoding (a change is sent immediately unless one went out in the last 50ms; otherwise held and merged).

**Why:** Each removes latency for no extra spend.

**Consequence:** Multi-region deployment → `infra.md`. Tick → client target returns to ≤ 100ms p99.

---

## D44 — Production runs our own Base node

*(Amended by D77: the node serves canonical blocks, simulation and state reads. No Flashblocks feed.)*

**Date:** 2026-09-28 · **Status:** Decided (amends D16)

**Decision:** Production runs a Base node (reth with Flashblocks, per Base's node repo) next to the Base engine. It takes the Flashblocks feed directly, serves local simulation (< 5ms), state reads and pending `getLogs`, and backs the reconciler. Chainstack/QuickNode remain as fallback. Dev and staging stay on free tiers (D17).

**Rejected:**
- *Providers only* (D16's original stance). Every simulation and state read is a network round trip, and Flashblocks arrive via an extra hop.

**Why:** The largest remaining latency and RPC-cost lever: removes a hop from ingestion, makes simulation local, and cuts most of Base's request volume from the paid plan.

**Consequence:**
- ~$150–250/mo for a dedicated machine (16+ cores, 4TB+ NVMe), plus node operations (upgrades, resyncs, monitoring).
- Same move for BNB and MegaETH is evaluated after launch (MegaETH replica node availability still unknown).

---

## D45 — Rust for engines and execution

**Date:** 2026-09-28 · **Status:** Decided (confirmed after weighing Go; see consequence)

**Decision:** The Chain Engine and Execution service are planned in Rust. Cold-path services (candles, history job, API) are chosen per service in build mode.

**Why:** p99 targets are dominated by tail latency, and garbage-collection pauses are the usual cause of p99 spikes in Go or Java. The EVM tooling is strong in Rust too (reth, alloy, revm for local simulation).

**Consequence:**
- **Go was weighed and set aside.** Go would cost roughly 1–5ms of a 20–75ms budget and is easier for the project lead to read, but Rust was kept for tail latency, compile-time safety in money code, and the Rust EVM stack (alloy, revm). The learning curve is the lead's to absorb, with AI assistance.
- **"Boring Rust" rule** so the codebase stays readable while the team learns: well-known crates (tokio, alloy, revm, serde, prost), plain structs and enums over clever generics, no custom macros in core logic, explicit error types, and comments on anything non-obvious about ownership or async.
- **Deterministic cores:** each engine and execution core is a single-threaded state machine (inputs in, decisions out) with I/O on tasks around it (D49).

---

## D46 — Latency targets: measured internally, budgeted per step

*(Amended by D57: the click path's signing step moves to the user's browser session; its latency is measured before the ≤ 100ms target is confirmed.)*

**Date:** 2026-09-28 · **Status:** Decided (hardens product.md's draft targets)

**Decision:**
- **Two clocks:** *internal latency* (our engine receives the event or request → broadcast) is what targets measure. *End-to-end* (chain timestamp → broadcast) is reported, not targeted, since provider delivery is outside our control.
- **Targets (p99):**

| Metric | Draft (product.md) | Target |
|---|---|---|
| Price move → trigger broadcast (internal) | ≤ 300ms | **≤ 75ms** (Base ~25ms; BNB/MegaETH ~35–50ms, bounded by remote simulation) |
| Click → broadcast | ≤ 150ms | **≤ 100ms** (any payment token) |
| Quote latency | ≤ 25ms | **≤ 10ms** |
| Price tick → client | ≤ 100ms | ≤ 100ms |
| Indexer lag | ≤ 1 block (Base/BNB), ≤ 250ms (MegaETH) | unchanged |
| Engine recovery / failover | — | < 10s / ≤ 5s (D40) |
| Trigger firing | — | exactly once: zero duplicates, zero missed (D35) |
| Quote accuracy | — | < 0.1% shadow-check mismatches (D21) |
| Concurrent orders | 100k | 100k per chain |

- **Simulation always blocks the send** (revised same day at the user's call): nothing is broadcast without a passing simulation. On Base it's local (< 5ms, D44); on BNB and MegaETH it's a call to a co-located provider (~10–30ms). Manual trades run it in parallel with the Privy signature.
- **Measurement:** every trade carries per-step timestamps as a trace; Prometheus + Grafana (free, D17) chart p99 per step.

**Rejected:**
- *Keep the draft targets.* After D42–D44 they'd hide regressions behind 5–10× headroom.
- *Target end-to-end latency.* Mixes provider delay we can't control into our numbers.

**Why:** Targets close to the budgets make regressions visible, and step-level traces point straight at the cause.

**Consequence:** Budgets per step → `slas.md`; load scenarios that exercise them → `loadtest.md`. Lever for later: own BNB/MegaETH nodes (D44 evaluation) or in-process simulation would bring BNB/MegaETH down to Base's number.

---

## D47 — Intent carrier: Permit2 in phase 1; EIP-7702 delegate as a phase 2 candidate; no ERC-4337 smart wallets

**Date:** 2026-09-28 · **Status:** Decided (confirms D42 after reviewing Privy's smart-wallet and EIP-7702 support)

**Context:** Privy supports ERC-4337 smart wallets (Kernel/ZeroDev, Safe, Alchemy, Biconomy, Coinbase, Thirdweb) with session keys, signing EIP-7702 authorizations for embedded wallets, native gas sponsorship, and a policy engine that can restrict EIP-712 signing by domain. EIP-7702 is live on all three chains: BNB (Pascal, March 2025), Base (Isthmus), MegaETH (Rex hardfork, based on Isthmus).

**Decision:**
- **Keep D42's model:** every trade carries a minimum output signed by the user (or signed at order creation for triggers).
- **Phase 1 carrier: Permit2 intents.** Privy's policy engine restricts delegated EIP-712 signing to our router's domain.
- **Phase 2 candidate: an EIP-7702 delegate.** The user's EOA delegates to our own minimal, immutable contract that verifies the same signed intent and executes it. It removes WETH wrapping and per-token Permit2 approvals (native coin works directly), at the cost of our code controlling the whole account.
- **ERC-4337 smart wallets: rejected** for the trade path.

| Option | Signed minimum per trade | Latency | Extra costs | Blast radius if our code or key fails |
|---|---|---|---|---|
| **Permit2 intents** (chosen) | Yes | Executor signs locally | Wrap native; one approval per token | Only signed intents, on their terms |
| EIP-7702 delegate + intents | Yes | Same | New delegate contract to audit | Delegate bug touches the whole account |
| EIP-7702 / 4337 session keys (no per-trade intent) | No: key can trade within caps at any price | Same (7702) or + bundler hop (4337) | Session-key policy design | Compromised key can dump holdings at bad prices, within caps |
| ERC-4337 smart wallets (Privy-native) | Optional | + bundler hop per trade | Bundler/paymaster per chain; new account addresses | Depends on account + session setup |

**Why:**
- Session keys trade per-trade user protection for convenience; a signed minimum on every trade is the security core of D42.
- Bundlers add a hop that the latency budget (D46) can't afford.
- Permit2 is audited and widely deployed; a 7702 delegate is new code with account-wide power, better introduced after launch with the router proven.
- Privy's gas sponsorship isn't needed: executors already pay gas and recover it (D42).

**Consequence:**
- D3's reason for setting smart accounts aside ("support varies across our three chains") no longer holds; the reason now is blast radius and latency.
- Copy trades keep one Privy signature at copy time (a session key would remove it, but without a signed minimum).

---

## D48 — Load testing: swappable event source, forked-node simulation, nine named scenarios

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Event source is an interface** with three implementations: *live* (mainnet feeds), *replay* (recorded streams at 1×/5×/10×), and *synthetic* (scripted events over real pool state: crashes, dropped preconfirmations, reorgs).
- **Execution in tests** runs the full pipeline (signed intent, route, blocking simulation, executor-signed transaction) and stops before broadcast (D5). Simulations run against a **local forked node** (Anvil or reth fork), so load tests stay free (D17).
- **Named scenarios:** steady state, flash crowd, stop-loss cascade, copy-trade fan-out, MegaETH firehose, failover under load, reorgs and dropped preconfirmations, provider trouble, 24h soak. Pass criteria are the D46 targets plus exactly-once firing.
- **Tools:** k6, Toxiproxy, Anvil/reth fork, Prometheus + Grafana. Each run publishes its dashboard and a results note to the repo.

**Rejected:**
- *Load test against provider RPCs.* Thousands of simulations per second would exceed free tiers and measure the provider, not us.
- *Live traffic only.* Can't produce crashes, reorgs or flash crowds on demand.

**Why:** Controls both sides, user traffic and what the chain appears to do, so every hot-path decision can be exercised on purpose.

**Consequence:** The engine's event input and execution's broadcast step are interfaces from day one (build mode).

---

## D49 — Chaos fuzzing: randomised combinations of every lever, invariants checked continuously

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** A fuzz mode that pushes the system on all fronts at once, mixing load and failure modes at random to find edge cases no named scenario covers.

**Levers (fuzz dimensions):**

| Category | Levers |
|---|---|
| User traffic | Rate, burst shape, order-type mix, concentration on one token, order edits/cancels racing fills |
| Market | Price paths: crash, pump, whipsaw across trigger levels, one-block wicks, liquidity pulls |
| Chain | Reorg depth and frequency, dropped preconfirmations, delayed or empty blocks, gas spikes, builder non-inclusion |
| Tokens | Tax changes mid-run, honeypot flips, mint/burn supply changes, bonding-curve graduation mid-order |
| Infrastructure | RPC latency, drops and stale responses; Kafka broker loss and consumer lag; Postgres slowness and failover; Privy latency and errors; engine kills; engine ↔ execution partitions; clock skew |

**Invariants (checked continuously; any violation stops the run and saves it):**
- Every trigger whose condition held fires exactly once; none fire from a stale epoch (D35, D40).
- No trade fills below its signed minimum; the router ends every call with a zero balance (D26, D42).
- Pool state after any undo equals state recomputed from canonical events (D12).
- Order and trade state machines only make legal transitions; positions' cost basis matches their fills.
- Executor nonce ledger matches the chain after re-sync; no gap outlives the watchdog (D32).
- Prices are finite and within the range of their pools' mids.
- Memory stays bounded. SLO breaches are recorded (soft invariant), not fatal.

**Search strategy:**
1. Every run has a **seed**; the same seed replays the same run.
2. Levers are drawn at random with weights; intensity **escalates** until an invariant or SLO breaks, recording the breaking point.
3. A failing run is **shrunk**: levers and duration are removed one at a time while it still fails, down to a minimal reproducer.
4. Minimal reproducers join the named scenario library as regression tests.
5. **Coverage feedback:** the fuzzer tracks which state transitions and lever combinations it has seen and biases toward unseen ones.

**Two tiers:**
- **System tier:** the real deployment (Kubernetes, Toxiproxy, forked nodes) under chaos. Slow but real.
- **Deterministic simulation tier:** the engine and execution cores run in one process with a simulated clock, network and RPC, driven by the seed. Thousands of simulated hours per real hour, and every failure replays exactly (the FoundationDB / TigerBeetle approach).

**Rejected:**
- *Named scenarios only.* Only finds the failures we already imagined.
- *Unseeded random chaos.* Finds failures it can't reproduce.

**Why:** The dangerous bugs in this system live in combinations: a reorg during a cascade while Kafka lags, a failover mid-firing. Only randomised combination with reproducible seeds finds them.

**Consequence (build mode):** Engine and execution cores must be deterministic given their inputs: time, randomness, network and RPC behind injectable interfaces, no hidden threads or wall-clock reads in core logic. This is a design constraint from the first line of code, not something that can be added later.

---

## D50 — Infrastructure layout: free dev/CI/staging, per-chain regional clusters, self-hosted data stores

**Date:** 2026-09-28 · **Status:** Decided (production cloud: see D51)

*(Amended by D76: CI stays on GitHub-hosted runners.)*

**Decision:**

| Env | Where | Cost |
|---|---|---|
| Dev | Local: k3d + docker compose, Anvil forks | Free |
| CI | GitHub Actions on **Blacksmith** runners (faster; 3,000 free min/month, then ~$0.004/min): unit tests, contract fork tests, Protobuf compatibility, nightly deterministic-simulation fuzz (D49) | Free tier, then cents |
| Staging | Oracle Cloud Always Free ARM (k3s) on free RPC tiers **(verify current limits)** | Free |
| Prod | Per-chain regional clusters + one central region | Paid (credits: D51) |

- **Per-chain cluster** (region nearest the chain's sequencer/builders, D43): engine + standby, execution, local Kafka for engine topics (feeds the standby, D40), and the Base node (D44).
- **Central region:** Postgres, ClickHouse, API/WebSocket gateway, candle service, history job; per-chain Kafka topics mirrored in.
- **No hot-path call crosses regions.** Postgres writes from execution are write-behind (D32); firing-ID dedupe lives in execution's local store.
- **Self-hosted data stores** on Kubernetes via operators: CloudNativePG, Strimzi (Kafka), Altinity (ClickHouse).
- **Tooling:** Terraform, Helm, Argo CD; Prometheus, Grafana, Loki, Tempo; SOPS in staging, cloud KMS for executor keys in prod (D4, D42).

**Rejected:**
- *Managed databases and Kafka.* Cost more than self-hosting (build-vs-buy rule, D15).
- *One global production cluster.* Every chain pays the distance to the others' sequencers.
- *GitHub-hosted runners.* Slower and, since March 2026, not cheaper.

**Why:** Free everywhere except production (D17); production placed for latency (D43).

**Consequence:**
- **MegaETH plans a rotating sequencer** that moves around the globe with the economic day. A fixed region is only near it part of the day; following it (or accepting the gap) is an open question for after launch.
- Sequencer/builder locations for Base and BNB **(verify)** before picking regions.

---

## D51 — Production cloud: Google Cloud, funded by credits via the Web3 startup program

*(Amended by D85: no grant applications. Production on Google Cloud stays the plan, on the Start tier only.)*

*(Amended by D75: RustFS replaces MinIO in dev and staging.)*

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- Production runs on **Google Cloud**.
- **Credits plan:**
  1. Start tier ($2k, 1 year) once there's a working MVP. Until then dev and staging are free (D17), so nothing is lost by waiting.
  2. Apply for a blockchain foundation grant: **BNB Chain Builder Grant** first (four.meme support, private anti-sandwich submission and fast finality make a BNB-specific pitch), Base Builder Grants and MegaETH grants as follow-ups.
  3. With a grant, move to the **Web3 program's Scale tier: up to $200k over 2 years** (first $100k of year one fully covered), which accepts blockchain foundation grants as qualifying funding.
- **Service mapping:** GKE for the clusters (D50), Cloud KMS for executor keys, Cloud Storage for snapshots (MinIO stays in dev/staging).
- **Tentative regions** **(verify sequencer/builder locations first)**: central region and Base in `us-east4` (N. Virginia); BNB in `asia-northeast1` (Tokyo) or `asia-southeast1` (Singapore); MegaETH starting in `us-east4` until its rotating sequencer is live.

**Rejected:**
- *Azure.* Most credits with no funding at all ($5k), but capped there without an investor referral.
- *AWS.* $1–5k self-funded; the large tiers need an Activate provider (VC or accelerator).
- *Oracle for production.* Discretionary credits; keeps its role as free staging.

**Why:** Only Google counts a blockchain foundation grant as funding, which opens up to 40× the credits of any unfunded option, and a grant is money and credibility in its own right.

**Consequence:**
- The Base node on GCP needs ~4TB+ of local NVMe (several local SSDs), costlier than bare metal; covered by credits, revisit if credits don't materialise.
- If no grant comes through, the Start tier's $2k plus Oracle staging still covers the early months; reassess before paid spend grows.
- Grant applications become a project task alongside build mode.

---

## D52 — Cypherpunk ground rules for every safeguard and every byte of telemetry

*(Extended by D66: anti-snooping rule.)*

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** Five constraints that the observability, safeguard and security designs (D53–D58) must satisfy.
1. **Brakes stop only our automation, never the user.** They can stop our engines firing and our executors submitting; they can never freeze, move or seize funds. Funds stay in users' own wallets, and the router has no owner and no pause (D26).
2. **Users can always leave without us:** export keys (D3), or submit their own signed intents to the router directly (D58), even with every one of our services off.
3. **Every brake is public and time-boxed:** logged to the tamper-evident public log (D55), shown on a public status page, and auto-expiring unless a human renews it.
4. **Verifiable, not just trusted:** open source, reproducible builds, verified contract source, and a verifiable receipt for every trade (intent hash, route, transaction, fill vs signed minimum, fee, gas).
5. **Minimal data about people:** pseudonymous IDs in all telemetry, never emails; per-user encryption keys so deleting a key makes that user's records unreadable (**crypto-shredding**) while the audit log's hash chain stays verifiable.

**Rejected:**
- *Operator-controlled pause or freeze on user funds.* Safer-feeling for us, but it turns a self-custody product into a custodian with a kill switch over users.
- *Collect everything, sort it out later.* Makes us a honeypot for personal data.

**Why:** Paranoid safety and user sovereignty are compatible if brakes act only on our own machinery and users always keep an exit that doesn't need us.

---

## D53 — Observability: end-to-end lineage, wide events, 100% tracing on money paths

*(Extended by D71: lineage ID format.)*

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Stack (self-hosted, free):** Prometheus (metrics), Loki (logs), Tempo (traces), Pyroscope (continuous profiling), Grafana.
- **Lineage:** every record carries the IDs of what caused it: `chain event → pool update → price update → trigger check → firing ID → intent hash → route decision → simulation → executor tx → landing → position update → notification`. Lineage edges are stored in ClickHouse and browsable as a graph, backwards (root cause) and forwards (blast radius).
- **Wide, typed events** (Protobuf) instead of free-text logs: one rich event per unit of work. **Decision records** capture the alternatives considered (route candidates and scores, slippage and tip reasons, safety verdicts and evidence).
- **Sampling:** money paths (trades, firings, signing, top-ups, fees) are traced **100%**; other traffic keeps every slow or failed trace plus a 1% sample.
- **Signals** per area: indexer (lag, reorgs, dropped preconfirmations, gap fills, failovers), pricing (shadow mismatch rate, thin tokens, depeg flag), triggers (armed, firing latency per step, duplicates caught, stale-epoch rejections), execution (simulation failures and reverts by reason, builder inclusion latency, Privy latency, executor gas, nonce gaps), money (volume, fees, executor gas spent vs recovered, treasury), infrastructure.
- **SLOs and alerts:** D46 targets as SLOs with error budgets; burn-rate alerts. Tiers: page (money at risk), ticket (budget burning), info. Pages via Grafana alerting to a Telegram bot.
- **Invariants as production monitors:** the D49 fuzz invariants run continuously in production (executor nonces vs chain, positions vs on-chain balances, router zero balance, fee and refund reconciliation, undo vs recompute).
- **Canaries:** a shadow canary every minute per chain (quote → intent → simulate); a daily real-funds round trip per chain from the D5 demo wallets.
- **Incident console:** search by user, order, transaction, token or time; shows lineage, decisions, traces, and a replay button (D54).

**Rejected:**
- *Sampled tracing everywhere.* The one trade you need to investigate is the one that wasn't sampled.
- *Managed observability (Datadog, Grafana Cloud paid tiers).* Costs money the self-hosted stack doesn't (D15 rule).

**Why:** Any outcome can be explained from its root cause, and any fault's blast radius found, with one query.

**Consequence:** Every service emits lineage IDs from day one; it's part of the event schemas, not added later.

---

## D54 — Flight recorder: every input recorded, any moment replayable exactly

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Input log:** every input to the engine and execution cores (raw provider messages with arrival time, RPC responses, API requests, clock reads, random seeds) is appended to per-chain Kafka topics and archived to Cloud Storage.
- **Exact replay:** because the cores are deterministic (D49), loading the snapshot before an incident and replaying the recorded inputs reproduces every decision exactly, with a debugger and extra logging attached after the fact.
- **Point-in-time state everywhere:** engine (snapshots + input log), Postgres (point-in-time recovery from archived WAL), ClickHouse (versioned rows, D41), configuration (Git, with the config version stamped on every decision record), binaries (signed image digests per deploy).
- **Replay-gated deploys:** a release candidate replays recorded production inputs and every decision that differs from what production did is flagged for review; then shadow alongside production; then one chain first, with automatic rollback on SLO burn.
- **Retention:** input log 30 days hot, 1 year compressed cold; money-path traces and decision records 1 year.

**Rejected:**
- *Logs and traces only.* They show what happened, not why, and can't be re-run with more instrumentation.

**Why:** Investigation stops being guesswork: the incident can be re-run, inspected and fixed against the exact inputs that caused it. The same recordings make every deploy testable against real traffic.

**Consequence:** Nothing in core logic may read time, randomness or the network except through recorded interfaces (reinforces D49).

---

## D55 — Trust nothing, including ourselves: independent watcher, tamper-evident audit log

**Date:** 2026-09-28 · **Status:** Decided

**Decision:**
- **Independent watcher:** a small separate service with its own code, its own RPC provider and ideally its own region. It reconciles every router and executor transaction on-chain against our records: each fill matches a recorded intent, never below its signed minimum; fees and gas refunds are correct; no router transaction appears that we didn't send. **Any mismatch pauses all executors** (D56).
- **Tamper-evident audit log:** signing requests, brake activations, configuration changes, deploys and admin access go into a hash-chained, append-only log (ClickHouse + write-once Cloud Storage). Its root is **anchored on-chain daily** (Base, cents per day), so anyone can verify history was never rewritten.
- **Monitor the monitors:** absent-data alerts, an external dead-man's switch if monitoring goes quiet, and clock-drift alerts (latency figures depend on synced clocks).

**Rejected:**
- *Reconcile inside the main system.* A bug or compromise there could hide itself.
- *Private audit log only.* Unverifiable by users (breaks D52 rule 4).

**Why:** Detects compromise or silent bugs in the main system with code that shares nothing with it, and makes our own history publicly checkable.

---

## D56 — Brakes on the money paths: automatic breakers and four manual levels

**Date:** 2026-09-28 · **Status:** Decided (bounded by D52)

**Decision:**

**Automatic breakers** (smallest scope that fixes the problem, logged publicly):

| Scope | Trips when | Effect |
|---|---|---|
| Per trade | Quote deviates > X% from display price; fee or gas over cap; simulation fails | That trade blocked |
| Per user (automated flows) | Server-signed volume over daily cap; anomalous pattern | That user's automation held, user notified; manual trading and self-submission still work |
| Per pool type | Shadow-check mismatch rate over threshold | Simulated quotes (D21) |
| Per chain | Indexer lag over threshold; primary and fallback disagree on head; reorg deeper than expected; revert or simulation-failure spike | Firing paused on that chain until healthy |
| Executor fleet | Gas spent outrunning recovered; nonce-gap storm; top-up cap hit | Executors paused |
| Global | Independent watcher mismatch (D55) | All executors paused |

**Manual brake levels** (each auto-expires after 1 hour unless renewed; each logged publicly):

| Level | Effect | Users can still |
|---|---|---|
| 1. Caution | Wider safety margins, no splits, no new venues | Everything |
| 2. Stop automation | No triggers or copy trades; orders stay armed | Trade manually, self-submit |
| 3. Stop submissions | Executors off | Self-submit intents, export keys |
| 4. Freeze server signing | Privy policy set to deny | Sign in their own session, export keys |

**On release:** orders whose levels were crossed during a pause fire with a fresh quote, and the user is notified.

**Rejected:**
- *Brakes that freeze user funds or the router.* Violates D52.
- *Brakes without expiry.* A forgotten brake silently breaks the product.
- *Cancel crossed orders on release.* They're still the exits users asked for.

**Why:** Contain damage at the smallest scope, automatically where possible, without ever taking control of user funds.

**Consequence:** Thresholds (X%, caps, lag limits) are tuning values set from measurement; brake state is part of every decision record.

---

## D57 — Security model: shrink every key's power, user-session signing when present

*(Amended by D85: no audit contest or bug bounty for the router; there are no user funds.)*

**Date:** 2026-09-28 · **Status:** Decided (amends D42, D46)

**Decision:**

| Asset (most → least valuable to an attacker) | Controls |
|---|---|
| **Server Privy authorization key** | Used only when the user is absent (copy trades, auto-armed TP/SL after a buy lands, background approve/wrap). When the user is present (manual trade, creating or editing an order), **the intent is signed in the user's own Privy session in the browser.** Server signing sits behind Privy policy: our router's EIP-712 domain only, per-intent cap, per-user daily cap, minimum-output floor relative to the quote. |
| **Router contract** | Foundry fuzz + invariant tests, Slither and Aderyn static analysis, verified source, audit contest or bug bounty (Immunefi) before real user funds. Immutable: the emergency stop is off-chain (D56). |
| **Executor keys** | Encrypted with Cloud KMS at rest, decrypted into memory at startup (KMS signing is too slow for the < 1ms budget). Small gas float per executor, auto top-up with daily caps, regular rotation. Fees and gas refunds go straight to the treasury, never to executors. |
| **Treasury** | Safe multisig per chain; no hot key can move it. |
| **User accounts** | Privy auth; MFA for withdrawals and key export; per-user and per-IP rate limits; D39 order limits. |
| **Infrastructure** | Private GKE clusters, Workload Identity, least-privilege IAM, network policies, Secret Manager; CI deploys via OIDC (no long-lived cloud keys in GitHub or Blacksmith). |
| **Supply chain** (public repo) | `cargo-audit`, `cargo-deny`, Dependabot, pinned dependencies, cosign-signed images with SBOMs, reproducible builds, GitHub secret scanning with push protection, branch protection with required checks. |

**Incident response:** runbooks per scenario, using the D56 brake levels; every action lands in the D55 audit log.

**Rejected:**
- *Server signs every intent (D42 as written).* Simpler and consistently fast, but the server key becomes able to trade for every user at any time.
- *Executor keys signing via KMS.* Safer at rest, but ~10–30ms per signature breaks the trigger budget.

**Why:** A stolen server key can only make small, capped trades inside policy, all visible to anomaly alerts; a stolen executor key holds only gas; nothing hot can reach the treasury.

**Consequence:** Browser signing latency depends on the user's connection to Privy; the click path (D46 ≤ 100ms) is measured with it. If it's too slow, clicks fall back to server signing under the same policy caps.

---

## D58 — Intents name their submitter

**Date:** 2026-09-28 · **Status:** Decided (amends D26, D42)

**Decision:** Every intent includes the set of addresses allowed to submit it: our executor set, and always the user themselves. The router rejects any other submitter.

**Rejected:**
- *Anyone may submit any intent.* A leaked pre-signed stop-loss intent ("sell N for at least X") could be submitted by anyone while the price is still high, selling the position early.
- *Only our executors may submit.* Removes the user's exit that doesn't need us (D52 rule 2).

**Why:** Leaked intents are useless to others, and users can always submit their own.

**Consequence:** Router invariant tests add "only a listed submitter can execute an intent". Rotating the executor set means new intents name the new set; armed orders are re-signed on rotation (user session when present, server signing under policy otherwise).

---

## D59 — Intent terms: maximum amount, minimum rate, submitter may only tighten

**Date:** 2026-09-28 · **Status:** Decided (amends D27, D42; from the final consistency review, G1)

**Decision:** Each signed intent carries:

| Field | Meaning |
|---|---|
| Input token, **maximum** input amount | The submitter may use less |
| Output token, **minimum output rate** | Minimum output scales pro-rata with the amount actually used |
| Deadline, recipient = signer | As before |
| Allowed submitters | Our executor set or the user (D58) |
| **Maximum fee rate, maximum gas refund** | Caps on what the router may deduct (D28, D42) |
| Unordered nonce, order ID | Replay protection (Permit2); lineage (D53) |

The submitter may pass a **tighter** minimum output than the signed one, never a looser one; the router enforces the stricter of the two. Execution always passes fresh quote × (1 − slippage) (D27).

**Rejected:**
- *Exact amount, fixed minimum (D42 as written).* Multi-level take-profits can't sell less than signed after a manual sale; trailing stops keep their starting-level protection; pre-signed orders lose fresh-quote protection.

**Why:** Pre-signed orders stay flexible where the user benefits (smaller fills, tighter protection) and rigid where it protects them (never more than signed, never a worse rate, fees and gas capped).

**Consequence:** Router invariant tests add: amount used ≤ signed maximum; output ≥ max(signed rate × amount, submitter minimum); fee and gas refund ≤ signed caps.

---

## D60 — Exit guarantee for stops

**Date:** 2026-09-28 · **Status:** Decided (amends D34; from G2)

**Decision:** Stop-loss and trailing-stop orders default to **exit guarantee**: if a gap-down means the signed minimum can't be met, the server re-signs a fresh intent at the current quote (with the D27 stop-loss slippage) under the D57 policy caps, and the stop goes through. Users can switch it off per order to get a stop-limit ("never sell below X"). Limit orders and take-profits default to off.

**Rejected:**
- *Signed floor only.* In a crash, the stop sits unfilled, the opposite of why users set it.
- *Always re-sign, no opt-out.* Some traders want a hard floor.

**Why:** Matches what Trojan users expect from a stop-loss (it gets you out), while keeping a stop-limit for those who want one.

**Consequence:** Every re-sign is a server signature and is visible in the order's lineage and the audit log (D53, D55).

---

## D61 — Execution high availability and regional state

**Date:** 2026-09-28 · **Status:** Decided (amends D35, D41; from G3)

**Decision:**
- The execution service runs **leader/standby** per chain with the same lease and fencing epochs as the engine (D40). A stale leader's submissions are rejected by its own state store.
- Execution's durable state (firing-ID dedupe, executor nonce ledger) lives in a small **regional Postgres per chain** (CloudNativePG, synchronous replica in the same region, ~1ms writes).
- The central Postgres keeps users, orders, positions and token metadata.

**Rejected:**
- *Execution state in the central Postgres (D41 as written).* A cross-region hop on the hot path (breaks D50).
- *Local disk store only.* Doesn't survive losing the node, and the standby can't read it.

**Why:** Execution becomes as recoverable as the engine without adding latency.

**Consequence:** Failover drills (D48 #6) cover execution as well as the engine.

---

## D62 — Frontend: thin prototyping UI and a full terminal UI; mirror the leaders, then do better

*(Extended by D91: the API contract v0, its wire format and generated types.)*

**Date:** 2026-09-28 · **Status:** Decided (closes product.md question 3)

**Decision:**
- **Two frontends, one API.** A thin UI for local work and prototyping (project lead). The full terminal UI is owned by **Jutin** (frontend developer), including its stack choices.
- **Mirror the industry leaders** (Trojan, Axiom, Photon, GMGN) wherever users already have muscle memory; innovate where our backend enables something they can't do.
- **User experience first:** every product decision is weighed by its effect on the trader.
- **API first:** the API contract (REST + WebSocket, types generated from our Protobuf schemas) is the boundary between backend and both UIs, and a mock server driven by recorded data (D54) lets the frontend be built without a running backend. Details in `frontend.md`.

**Why:** Traders switch terminals easily and punish unfamiliar layouts; familiarity where it helps, differentiation where it matters.

---

## D63 — Build plan approved: Base-first walking skeleton, milestones M0–M12, no timeline commitment

*(Amended by D70: M0 scope trimmed. Amended by D85: the last milestone is production readiness, not launch.)*

**Date:** 2026-09-28 · **Status:** Decided

**Decision:** The build follows [build-plan.md](../build-plan.md): a walking skeleton on Base first (M0–M5, ending in the first real-funds trade), then depth on Base (M6–M7), BNB (M8), MegaETH (M9), copy trading and event orders (M10), hardening (M11), and launch readiness (M12). The frontend track starts at M2 against a mock server.

**Timeline:** none committed. Development is AI-assisted and pace is measured, not promised. The plan's size estimates stay as a rough sense of relative effort only. D7's "≈1 month" is withdrawn.

**Why:** One chain end to end proves the architecture with the fewest unknowns; widening after that reuses what the skeleton built.

**Consequence:** Build mode starts only when explicitly switched on; until then the repo stays docs-only.

---

## D64 — Build on Base, launch on BNB

*(Amended by D85: nothing launches. BNB keeps its place in the build order as the chain where EVM memecoin trading and sandwiching happen.)*

**Date:** 2026-09-29 · **Status:** Decided (from [market.md](../market.md); amends D2, D63)

**Decision:** The walking skeleton stays on Base (M0–M5). BNB moves ahead of Base depth in the build order, and **BNB is the launch beachhead**: anti-sandwich by default, the full four.meme lifecycle, safety with evidence.

**Why:** Terminal revenue on EVM is concentrated on BNB (GMGN's BSC fees ~$30M vs ~$0.4M on Base), while Base remains the lowest-risk chain to prove the architecture on.

**Consequence:** Milestones reordered: M7 Safety, M8 BNB, M9 Base depth (build-plan.md).

---

## D65 — Public execution-quality report

**Date:** 2026-09-29 · **Status:** Decided

**Decision:** Publish, per chain and over time: quoted vs realised price, sandwich attempts avoided on BNB, fill latency, stop-loss exit rate, revert rate. Each trader also sees their own figures and "what our execution saved you" on each trade. Built from lineage and decision records (D53).

**Why:** Traders lose 15–30% to slippage, taxes and MEV without seeing it, and no terminal reports execution quality. It makes our execution work visible and turns the fee into a net-positive story.

**Consequence:** The report's numbers must be reproducible from public on-chain data plus our published methodology (D52 rule 4).

---

## D66 — Anti-snooping rule

**Date:** 2026-09-29 · **Status:** Decided (extends D52, D57)

**Decision:** No internal tool shows which wallets belong to which user by default. Any lookup of an identity ↔ wallet mapping requires a stated reason and is written to the public, on-chain-anchored audit log (D55), with the user's pseudonymous ID and the reason (never the wallets themselves). Aggregate analytics never expose per-user wallet sets.

**Rejected:**
- *Internal access controls without public logging.* The control at issue in the Feb 2026 insider-tracking allegations against a major terminal ([market.md](../market.md)).

**Why:** Makes "we can't quietly track your wallets" a verifiable property, not a promise.

---

## D67 — Chain onboarding kit: day-zero support for new EVM chains

**Date:** 2026-09-29 · **Status:** Decided

**Decision:** Adding an EVM chain is a packaged process: per-chain config (block timing, finality, streams, submission path), venue adapters reusing the shared math (D21), a verification checklist (Permit2, Privy, state overrides, providers), and shadow-mode acceptance tests. Target: support a new EVM chain within days of its mainnet.

**Why:** Early flow on new chains goes to whoever supports them first (Robinhood Chain: GMGN took 18.5% of volume in three days).

**Consequence:** MegaETH is the kit's first real use (M11).

---

## D68 — Fees: 1% headline with referral tiers and cashback

*(Superseded by D85: referral tiers and cashback are out of scope. D28's per-trade fee stays.)*

**Date:** 2026-09-29 · **Status:** Decided (amends D28)

**Decision:** Keep the 1% headline (D28), with multi-level referrals and activity-based cashback from launch, landing active traders at a lower effective rate. Every trade shows what our execution saved (D65).

**Why:** 1% is market rate on EVM (Banana Gun, GMGN), but the category competes on cashback and referrals; Solana terminals net ~0.45–0.75%.

**Consequence:** Exact tiers are a launch decision (M13). The router's signed fee cap (D59) is per trade, so tiers need no contract change.

---

## D69 — MegaETH deprioritised until it has real volume

**Date:** 2026-09-29 · **Status:** Decided (amends D2)

**Decision:** MegaETH stays in scope but moves behind copy trading (M11), onboarded through the chain kit (D67).

**Why:** ~$1.6M/day DEX volume versus hundreds of millions on BNB. Option value, not a market yet.

---

## D70 — M0 scope: only what the replay demo exercises

*(Amended by D75: RustFS replaces MinIO in the local stack.)*

**Date:** 2026-09-29 · **Status:** Decided (amends D63; from the M0 grilling session)

**Decision:**
- **Core shape:** engine and execution cores call the `det` traits directly (Clock, Rng, EventSource, Rpc, …). Tests and the simulation harness swap in simulated implementations; production wraps real ones in recording wrappers.
- **Crates:** M0 creates `proto/` and the `types`, `det` and `sim` crates only. Every other crate in the build-plan layout is created by the first ticket that needs it.
- **Local stack:** docker compose with Postgres, Kafka (Apache Kafka in KRaft mode, the broker Strimzi runs in production), MinIO, Tempo and Grafana, plus Anvil. k3d waits for the first staging deploy.
- **Proto tooling:** `buf lint` and `buf breaking` against `main` in CI, prost for Rust codegen.
- **Observability skeleton:** `tracing` with an OTLP exporter to Tempo, lineage IDs as span attributes, the toy core's decision records as wide events. Prometheus, Loki and Pyroscope wait until there's a service to watch.
- **Toolchain:** stable Rust pinned in `rust-toolchain.toml`, bumped deliberately.
- **Measurement tasks:** M0 keeps the two Base measurements that gate M1: event rates and provider delivery delay. The rest move to the milestone that first uses the result; the placement table is in [build-plan.md](../build-plan.md#measurement-tasks-need-live-network-access).

**Rejected:**
- *Sans-IO cores* (a pure `step(input) -> outputs`, with every trait in the I/O shell). Makes replay trivially the input stream and rule 6 hold by construction, but turns every multi-step flow (simulate, sign, submit) into hand-written states. Trait calls keep core code readable while the learning curve is steep (D45).
- *Scaffolding every crate up front.* Empty stubs are stale docs.
- *k3d in M0.* Nothing in M0 deploys.
- *Redpanda in dev.* Lighter, but a different broker from production.
- *prost without buf.* Leaves D41's schema compatibility checks for later, when they are cheapest now.
- *All eleven measurement tasks as M0 blockers.* Most need keys and chains M1 doesn't touch.

**Why:** M0's demo is "CI green; a simulated-clock test replays identically". Anything that doesn't serve that demo or unblock M1 delays the first code.

**Consequence:** Because the core calls traits, determinism rests on the trait boundary: the simulated and recorded implementations, plus a check that core crates reach time, randomness and the network no other way (rule 1).

---

## D71 — Lineage IDs are content-derived where a natural key exists

**Date:** 2026-09-29 · **Status:** Decided (extends D53)

**Decision:** A record's lineage ID is derived from its natural key wherever one exists: a chain event from (chain, block hash, log index), a firing from its firing ID (D35), an intent from its hash. Records with no natural key take an ID from the core's seeded `det` Rng.

**Format:** 16 bytes. A content-derived ID is BLAKE3 of the natural key's canonical encoding, truncated to 128 bits; a seeded ID is 16 bytes from the Rng. Every wide event carries a shared `Lineage { id, caused_by[] }` message.

**Rejected:**
- *UUIDv7 everywhere, drawn from the seeded Rng.* Deterministic under replay, but the same chain event seen twice (fast loop, then reconciler, or a re-insert after a reorg) gets two IDs.
- *keccak-256, or 32-byte IDs.* Matches EVM tooling, but doubles the size of every lineage edge for no collision benefit at our volumes.

**Why:** The same fact always gets the same ID, across replays, reprocessing and services. ClickHouse re-inserts stay harmless (D41), and lineage edges join without a lookup table.

**Consequence:** Proto schemas define each record's natural key alongside its lineage ID.

---

## D72 — det runtime in M0: four traits, lint-enforced, recorded to Kafka

*(Amended by D83: every input log starts with the core's configuration.)*

**Date:** 2026-09-29 · **Status:** Decided, except sync vs async (from the M0 grilling session; builds on D49, D54, D70)

**Decision:**
- **Traits in M0:** Clock, Rng, EventSource and Rpc. Store arrives with the first M1 ticket that needs it; Signer and Broadcaster in M5.
- **Sync or async traits: open.** *(Settled by D74: traits that wait are async.)* Settled by a throwaway spike, the first M0 ticket, that runs one toy core with sync traits (core blocks on an I/O task) and with async traits (one task on a current-thread runtime, simulated implementations completing in seed order), and replays both. Expected winner: async.
- **Determinism check (rule 1):** a `clippy.toml` in each core crate bans, via `disallowed-methods` and `disallowed-types`: wall-clock reads (`SystemTime::now`, `Instant::now`), OS randomness (`rand::thread_rng`, `rand::random`), `tokio::time`, `tokio::spawn`, `std::thread::spawn`, and std `HashMap` / `HashSet` (random iteration order; use `BTreeMap` or a fixed hasher). A CI grep rejects any `select!` without `biased;`. Clippy reading a per-crate `clippy.toml` **(verify)**. *(Amended by D73: one workspace-wide `clippy.toml`, with `det` opting out.)*
- **Recording:** every recording wrapper writes `InputRecord`s (source, sequence number, arrival time, payload) through a recording sink. The real sink is the Kafka input log from M0 (D54); tests use an in-memory sink.
- **Input-log layout:** `inputs.<chain>` (and `inputs.sim` for M0) has one partition, keyed by core instance, so replay sees the exact order the core saw. A chain's input rate fits in one partition: measured on Base, over 6x headroom at p99 ([verification.md](verification.md)).
- **Archive:** the Cloud Storage copy of the input log arrives with the engine in M1; M0 relies on Kafka retention.
- **Replay demo:** a toy core in `sim` reads EventSource, Clock, Rng and Rpc and emits decision records. The test asserts that the same seed run twice gives an identical decision digest, and that a recorded run replayed from its recording gives the same digest. CI runs 100 seeds against the in-memory sink; one integration test records a seed to Kafka (a CI service container) and replays it from there.

**Rejected:**
- *All seven traits in M0.* Signer, Broadcaster and Store have no caller until M1 or M5.
- *A dependency ban only (cargo-deny).* Doesn't catch `HashMap` iteration order or wall-clock reads through std.
- *Recording to a local file first.* The input log's real home is Kafka; building the file format first means building the log twice.
- *Every seed through Kafka.* Slow and flaky in CI for no extra coverage past the first seed.
- *Several partitions keyed by source, re-merged by sequence number on replay.* More moving parts for throughput a chain doesn't need.
- *Deciding sync vs async on paper.* It's an empirical question (CLAUDE.md: prototype, don't write a D-entry).

**Why:** M0 has to prove the property everything later depends on: a core driven only through `det` replays exactly, and nothing in core code can quietly break that.

**Consequence:** The spike's result amends this entry with the sync-or-async choice.

---

## D73 — Determinism bans are workspace-wide; det opts out

**Date:** 2026-09-29 · **Status:** Decided (amends D72)

**Decision:** The rule 1 bans live in one `clippy.toml` at the workspace root, so they apply to every crate by default. A crate at the I/O boundary opts out with its own `clippy.toml`; today that is only `det`, whose real implementations read the wall clock and the OS random source. A fixture that breaks each ban on purpose is linted on every verify run, so a weakened ban fails the build.

**Rejected:**
- *A `clippy.toml` in each core crate (D72 as written).* A new crate that forgets the file gets no bans; the safe default is banned.

**Why:** Clippy uses the nearest `clippy.toml` and doesn't merge files, so a root default with explicit opt-outs makes forgetting fail closed.

**Consequence:** Crates that join the I/O boundary later (`chain-io`, `api`) add their own `clippy.toml` with a comment saying why.

---

## D74 — det traits that wait are async; the core is one task on a current-thread runtime

**Date:** 2026-09-29 · **Status:** Decided (settles D72's open question, from the sync-vs-async spike)

**Decision:**
- **Reads stay sync.** `Clock` and `Rng` never wait, so they stay plain methods.
- **Waits are async.** `EventSource`, `Rpc`, and later `Signer`, `Broadcaster` and `Store` return `LocalBoxFuture`s, so cores keep holding `Box<dyn …>` with no generics. A call's future owns what it needs (`'static`), so the core can park it with other in-flight calls and keep handling events.
- **One task.** Each core runs as a single task on a current-thread tokio runtime. It waits with `select!` (always `biased;`) over its event source and a `FuturesUnordered` of in-flight calls.
- **Simulation** starts that runtime with paused time: simulated sources wait on tokio's clock, which jumps to the next timer when the task is idle. Simulated `Clock` reads the same paused clock so waits and reads agree.
- **Cancel safety.** The core drops a pending `EventSource::next()` whenever another branch wins, so sources must be cancel-safe. In production an `EventSource` is a channel receiver fed by the I/O tasks, which is cancel-safe by construction.
- **Replay** enforces the recorded order: a source's future is ready only when its record is next in the log, and a replay that stalls is reported as diverged.

**Evidence** (spike on branch `spike/det-sync-async`, 300 seeds, 2,000 events 10ms apart, one in ten needing a 20–200ms RPC call):

| | Sync traits | Async traits |
|---|---|---|
| Same seed, same digest; recorded run replays exactly | Yes | Yes |
| Worst lag between an event and the core handling it | 7,605ms, growing with run length | 0ms |
| Core code | 156 lines, plain calls | 220 lines: boxed futures, `select!`, an in-flight set, an order-enforcing replay log |

Both variants made identical quote and firing decisions. Removing `biased;` broke determinism at the first seed. An event source that advanced before its wait silently dropped events when cancelled, with no panic.

**Rejected:**
- *Sync traits.* Deterministic by construction, but a blocked core stops handling events for the whole call. Execution waits 100ms+ on signing (D46, D57), so every other trade would queue behind it.
- *Async traits with generics (`async fn` in traits).* Not dyn-compatible, so type parameters spread through every core type; boxed futures keep Boring Rust's plain `dyn` (D45).
- *Sans-IO* was already rejected by D70.

**Why:** Only async keeps the core responsive while calls are in flight, and the spike showed it stays exactly replayable.

**Consequence:** `det` gains a tokio dependency (it's the I/O boundary, D73). The simulated clock moves onto tokio's paused clock when the first waiting trait lands. Cancel safety needs a test per `EventSource`: drop `next()` mid-wait and check nothing is lost.

---

## D75 — RustFS replaces MinIO for dev and staging object storage

**Date:** 2026-09-29 · **Status:** Decided (amends D41, D51, D70)

**Decision:** Dev and staging use RustFS (Apache-2.0, S3-compatible) wherever the spec said MinIO. Production stays on Cloud Storage (D51). Code reaches object storage through one S3/GCS-capable client, so the store is a per-environment config choice.

**Found while building the local stack:** MinIO's open-source repository was archived in April 2026, and `minio/minio` is gone from Docker Hub and has no active tags on quay.io. The maintained edition, AIStor, is a separately licensed product.

**Rejected:**
- *Keep MinIO from an old image or a source build.* Unmaintained from here on.
- *SeaweedFS.* Apache-2.0 and mature, but a larger system (master, volume and filer servers) than a snapshot store needs.
- *fake-gcs-server.* Matches production's API, but staging runs outside Google Cloud (D17), so dev would match neither staging nor the S3 tooling.
- *Defer the choice to M1*, when the input-log archive first writes objects. Leaves the M0 stack without the store the spec lists.

**Why:** RustFS is the closest drop-in for MinIO: the same S3 API and console, a permissive license, and a 1.0 release (2026-09-16).

**Consequence:** RustFS is young; if it misbehaves, SeaweedFS is the fallback, and the S3 client means switching is config only.

---

## D76 — CI stays on GitHub-hosted runners

**Date:** 2026-09-29 · **Status:** Decided (amends D50)

**Decision:** CI runs on GitHub-hosted runners, not Blacksmith. Revisit if the repository goes private or CI time starts to hurt.

**Found while setting up Blacksmith:** Blacksmith supports GitHub organizations only, not personal repositories ([quickstart](https://docs.blacksmith.sh/introduction/quickstart)), and `AndySakov/omnimarket` is a personal repository.

**Rejected:**
- *Move the repository into an organization to use Blacksmith.* A transfer for a speed-up CI doesn't need yet: the verify job takes about 20 seconds.

**Why:** D50 chose Blacksmith for free minutes, but GitHub-hosted runners are free and unmetered for public repositories, so the cost reason is gone.

**Consequence:** Going private brings GitHub's free-tier minute cap; that is the point to reconsider Blacksmith (with an organization) or paid minutes.

---

## D77 — Triggers fire on canonical blocks; Base follows canonical blocks only

**Date:** 2026-09-29 · **Status:** Decided (amends D10, D16, D20, D44)

**Decision:**
- Trigger orders evaluate on state from the chain's canonical blocks, never on provisional state. The display price updates as soon as the engine applies new state.
- The engine checks triggers whenever canonical state changes: a new block from the fast loop, and a block the reconciler confirms or gap-fills.
- Base follows canonical blocks only (`newHeads` + one filtered `getLogs` per 2s block, like BNB). M1 drops the Flashblocks tick, pending `getLogs` and dropped-preconfirmation handling.
- Chains with a preconfirmation layer (MegaETH mini-blocks) revisit this when they are grilled.

**Rejected:**
- *Fire instantly on provisional state* (D20 as written). A dropped preconfirmation leaves a firing whose cause never happened, and a sell cannot be taken back.
- *Per-order opt-in instant mode.* Kept as a later option: D20 already keeps the trigger rule pluggable per order.
- *Wait for L1 finality.* No phantom firings, but 15 to 20 minutes makes a stop-loss useless.

**Why:** A throwaway prototype (branch `prototype/base-tip-following`, `crates/engine/tip-following.PROTOTYPE.html`) modelled the fast loop and reconciler under Flashblocks and under Base's planned 200ms blocks (Denim). Firing on canonical blocks removed every phantom firing caused by a dropped preconfirmation; only reorgs still cause them. The cost is up to one block: 2s on Base today plus ~0.5s delivery (measured, [verification.md](verification.md)), about 200ms after Denim. Flashblocks are also an unstable dependency: the raw feed is for node operators, there is no free public WebSocket (D17), and Denim plans to remove them. Following canonical blocks makes the design the same before and after Denim.

**Consequence:**
- The UI says triggers fire on confirmed blocks. A user can briefly see the display price past their level before the order fires.
- The trigger latency target ([slas.md](slas.md)) starts when the engine applies a canonical block.
- Base stays first (build plan), for being measured, free and having reorgs to exercise the reconciler. It no longer exercises provisional state.
- Reorgs remain the only source of phantom firings: D78.

---

## D78 — Trigger swaps carry an on-chain price guard

**Date:** 2026-09-29 · **Status:** Decided **(verify)** feasibility and gas cost in the router

**Decision:** A trigger order's swap reverts if the pool price at execution is on the wrong side of the trigger level (for a stop-loss, above it). A firing caused by a reorged-out block then costs gas instead of the position.

**Rejected:**
- *Accept reorg phantoms.* The slippage limit bounds how bad a fill is, not whether the order should have fired.
- *Wait N blocks before firing.* Slower, and a deeper reorg still gets through.

**Why:** Under D77, reorgs are the only way a trigger fires on a price that never settled, and the chain itself is the only place that knows the settled price at execution.

**Consequence:**
- The router contract needs a price check per trigger swap → `routing.md`, `security.md` when `contracts/` is built.
- Relative triggers (% from entry, trailing) pass the absolute level computed at firing.
- Base replaced no canonical block in an hour of measurement (1,801 blocks), so reorg phantoms are rare, not impossible → [verification.md](verification.md).

---

## D79 — Kafka client: rdkafka

**Date:** 2026-09-30 · **Status:** Decided (from building the input log, #20)

**Decision:** Rust services talk to Kafka through `rdkafka`, the Rust wrapper over librdkafka, built from its bundled source. The input-log producer is idempotent (`enable.idempotence`), so retries can't reorder or duplicate records.

**Rejected:**
- *rskafka* (pure Rust, no C build). Lighter to compile, but a much smaller user base and no idempotent producer, which the input log's ordering relies on.
- *kafka* (the `kafka` crate). Unmaintained.

**Why:** librdkafka is the client most Kafka deployments run, with idempotence, transactions and consumer groups we'll need past M0. Boring Rust (D45) favours the well-known crate.

**Consequence:** Building `det` compiles librdkafka (C), about a minute and a half on a clean build; CI caches it. A C toolchain is needed locally, which macOS and the CI image already have.

---

## D80 — On free RPC, Base is followed by polling, and pools are discovered as they appear or trade

*(Amended by D82: `eth_call`s go to PublicNode's free endpoint. Amended by D88: a block endpoint that can't answer stops the run with an error naming `--rpc`.)*

**Date:** 2026-09-30 · **Status:** Decided (from building M1, #37; dev and staging only, D17)

**Decision:**
- **Head following:** in dev and staging, the Base head follower polls the free public RPC over HTTP (`eth_blockNumber` every 500ms). For each new block it reads the header by number and the followed logs by block hash, and fills any skipped block by number, in order. Production swaps in `newHeads` from our own node or the paid provider (D16, D44) behind the same `ChainReader`.
- **Pool discovery:** a pool becomes known from its creation event, or from its first followed event (`Sync`, `Swap`, `Mint`, `Burn`) if it was created before the engine started. Each is proven genuine by recomputing its CREATE2 address from the factory, its tokens (and fee) and the init code hash. A contract answering `factory()` can lie; its address can't.

**Rejected:**
- *A factory scan from genesis at cold start.* The free endpoint caps `eth_getLogs` at 2,000 blocks, so covering Base's ~52M blocks takes about 26,000 calls and returns over a million v2 pairs, most never traded again. Cold start would take hours.
- *A free third-party WebSocket.* No signup-free Base WebSocket is documented by Base, and polling measured fine: blocks first seen ~473ms after their timestamp (verification.md).

**Why:** A pool that trades shows up in the logs we already follow, and a pool that never trades can't be priced anyway. Proving pools by address keeps discovery free of extra trust.

**Consequence:**
- A pool created before startup that hasn't traded since is unknown until it trades. The history job's 30-day backfill (D15) populates known pools for search when it lands.
- Discovery needs the factory addresses and init code hashes per venue in config, each checked against a live pool in a test.

---

## D81 — PRs merge only after a watchdog review

*(Amended by D90: the watchdog posts its verdict as a PR comment, and the `watchdog-status` workflow turns it into the `watchdog/review` status; the watchdog runs on the build account; builders merge once `verify`, `frontend` and `watchdog/review` pass.)*

**Date:** 2026-09-30 · **Status:** Decided (process; from auditing M0 and M1)

**Decision:** A separate watchdog agent session reviews every PR before it merges. It checks the change against its issue, the D-entries and CLAUDE.md's update table, runs the tests, and posts a `watchdog/review` commit status on the PR's head commit (`pending`, then `success` or `failure`) with its findings as a PR comment. Branch protection on `main` requires `verify` and `watchdog/review`, and applies to admins. Each push needs a fresh review. The protocol is in CLAUDE.md.

**Found while auditing:** PRs #32 to #44 had no reviews. The builder merged each one 1 to 8 minutes after opening it, so CI was the only gate. #44 landed without the D-entry and `pricing.md` update CLAUDE.md requires (#48), and a follower stall on reorged-out blocks went unnoticed (#46).

**Rejected:**
- *Required approving reviews.* Every agent acts as the one GitHub account, and GitHub doesn't let an account approve its own PR.
- *A soft gate (the builder waits a while, then merges).* Relies on the builder following a rule it already skipped: CLAUDE.md asked for `/meta-review` before merging.
- *Review after merge.* Defects reach `main` first.
- *A paid CI review bot.* Free resources only for now.

**Why:** The builder moves faster than anyone can read its PRs. A gate that blocks the merge is the only review that reliably happens.

**Consequence:**
- When the watchdog is down, nothing merges. Temi can lift the gate by turning off admin enforcement on `main`.
- GitHub can't tell who posted a status, so the builder's token could post `watchdog/review` itself; only CLAUDE.md forbids it. Binding the required check to a GitHub App that only the watchdog holds closes this gap. **(Follow-up: needs Temi to create the App.)**

---

## D82 — On free RPC, the engine's eth_calls go to PublicNode

*(Amended by D88: an outage such as PublicNode's `-32701` is retried rather than taken as the call's answer, and a call endpoint that can't answer stops `engine follow` with an error naming `--call-rpc`.)*

**Date:** 2026-09-30 · **Status:** Decided (from building v3 pools, #39; dev and staging only, D17; amends D80)

**Decision:** In dev and staging, the engine reads blocks and logs from Base's public endpoint (D80) but sends its `eth_call`s (pool proofs, bootstrap reads, shadow checks) to PublicNode's free Base endpoint, `base-rpc.publicnode.com`, which needs no signup. The call worker also starts at most 5 calls a second, keeps at most 8 in flight, and retries rate-limit errors instead of passing them to the core as answers.

**Found while building:** Base's endpoint allows about 20 `eth_call`s per 30 seconds per client, whatever the call's size: a 20-call and a 500-call multicall each got exactly 20 through per window. Plain requests aren't limited at 10 a second. A live run bootstrapping v3 pools spent more time waiting out rate limits than reading. It answers a rate limit with HTTP 429 and a JSON-RPC error body, which a client reads as the call's answer unless it checks.

**Rejected:**
- *Stay on Base's endpoint with bigger batches and a 0.6/s pace.* Bootstraps would lag minutes behind the chain on a busy stretch.
- *Read bootstrap state at whatever block a merged call runs at, to merge more.* Workable, but a bigger change to the core to work around one endpoint.
- *A keyed free tier.* Needs a signup (free-only rule: ask first); unnecessary while PublicNode works.

**Why:** PublicNode took 500-call and 2,000-call multicalls at 2 to 5 a second without one rate limit (measured 2026-09-30, verification.md). The engine reads state seconds after its block, well inside the ~90 blocks of state PublicNode keeps.

**Consequence:**
- PublicNode refuses reads more than ~90 blocks back ("archive requests require a personal token"). The core sees that as a failed call and rediscovers the pool on its next event, so a long stall costs re-reads, not correctness.
- Production uses our own node and the paid provider (D16, D44); `--rpc` and `--call-rpc` point anywhere.

---

## D83 — Every input log starts with the core's configuration

**Date:** 2026-09-30 · **Status:** Decided (amends D72; from building v2 pools, #38, recorded after review in #48)

**Decision:** The first record of every input log is the core's configuration, as its own source (`Source::Config`, `INPUT_SOURCE_CONFIG`), in the core's proto config schema (the engine's is `omnimarket.engine.v1.EngineConfig`). Recording wrappers never write it; the binary does, once, before the core runs (`Recorder::record_config`). Replay takes it first (`Replay::config`) and builds the core from it, so a recording replays from the log alone.

**Found while building:** a live run with shadow checks on (`--check-every 5`) replayed under the default config diverged at the first recorded check answer: a core with different config issues different calls. D54 already asks for the config version on every decision record; the input log needs the config itself, since replay rebuilds the core.

**Rejected:**
- *Config passed on the replay command line.* A replay then depends on someone remembering the flags of a run days old, and a wrong flag looks like a determinism bug.
- *A config version or hash only.* Enough to detect a mismatch, not to rebuild the core; it needs a config store keyed by version, which doesn't exist yet.
- *Config as an event from the `EventSource`.* It isn't something the core waits for, and it must be there before the core starts.

**Why:** Replay has to reproduce the core exactly, and config is one of its inputs.

**Consequence:**
- Every binary that records a core records its config first; `engine replay` refuses a recording that doesn't start with one.
- Changing a config schema needs the same compatibility care as any recorded payload (`buf breaking` covers it).

---

## D84 — No separate reconciler on chains followed by canonical blocks

**Date:** 2026-09-30 · **Status:** Decided by Temi (amends D10; answers #50)

**Decision:** Base, and BNB when it lands, have no reconciler loop. Their one follower delivers canonical blocks, and the reconciler's jobs are done where they already happen:
- **Confirming provisional events, catching dropped preconfirmations:** nothing is provisional (D77).
- **Filling gaps:** the head follower fetches every skipped block by number, in order, and retries failures without skipping (D80).
- **Detecting reorgs:** the core compares each block's parent hash with the head it holds; tiered undo walks back, undoes and emits corrections (D12).

MegaETH keeps D10's reconciler, since its fast loop (mini-blocks) is provisional.

**Rejected:**
- *A trailing `getLogs` re-read per block anyway.* It would catch a node that silently leaves logs out of an answer, but doubles the log requests for a failure not yet seen. The shadow state check (`--check-every`) already catches its effect on tracked pools; if it ever fires, a trailing re-read is the fix to add.
- *Keep the reconciler in the M1 scope as written.* It would be a loop with nothing to do.

**Why:** D10 split tip following into a fast provisional loop and a canonical reconciler. With D77 the fast loop is canonical on Base, so the split collapses into one loop.

**Consequence:** The build plan's M1 scope and `indexer.md` drop the Base reconciler. The history job (D15) reuses the head follower's per-block `getLogs` loop on these chains instead of a reconciler's. A shadow-check mismatch on a live run is the trigger to revisit.

---

## D85 — OmniMarket is a proof of concept, not a commercial launch

**Date:** 2026-09-30 · **Status:** Decided (amends D28, D51, D63, D64; supersedes D68)

**Decision:** OmniMarket is a proof of concept: it shows how the backend of a Solana-first terminal like Trojan could run on EVM chains. It uses Trojan's product (order types, fee model, wallet stack) as its reference spec. It has no users, no token, no fundraising and no launch.
- **M13 becomes production readiness:** GCP production layout, SLOs and alerts, runbooks. The audit contest, bug bounty, public launch and fee and referral tiers are dropped.
- **Fees:** the router still takes D28's per-trade fee, because the reference product charges one and the contract design has to carry it. Referral tiers and cashback (D68) are out of scope.
- **No grant applications** (D51). Anything beyond free tiers is Temi's call when it comes up.
- **Real-funds demos stay** (D5): about $50 per chain, recorded, to prove the path end to end.
- `docs/market.md` is kept as research into where EVM trading happens and what goes wrong there, not as positioning.

**Rejected:**
- *Keep the launch plan.* A launch needs distribution, legal and support work that proves nothing about the engineering, and it would compete with the product the project is modelled on.
- *Drop the fee from the router.* The fee is part of what a production router has to get right (taken in the same transaction, in the native or quote asset), so leaving it out would make the proof weaker.

**Why:** The project's job is to show how its author designs and builds trading infrastructure. Scope that only matters for a commercial product costs time and says nothing about that.

**Consequence:** build-plan.md's M13 and relative-effort table, highlights.md, frontend.md, infra.md, market.md, routing.md (no referral rate), security.md and D57 (no audit contest or bug bounty) are updated to match.

---

## D86 — The commit gate splits by path, and fails closed

**Date:** 2026-09-30 · **Status:** Decided (process; from the terminal foundation, #56)

**Decision:** The git pre-commit hook runs `scripts/verify-fast.sh`. A commit whose staged files are all under `web/terminal/` runs the frontend's fast checks (`npm run verify:fast`: typecheck, lint, unit tests). Every other commit, an empty one included, runs `scripts/verify.sh` as before. `verify.sh` stays backend-only (Rust, contracts, proto) and needs no Node. CI runs the frontend's full checks (`npm run verify:pr`) in its own `frontend` job, beside `verify`.

**Found while reviewing:** #56's first version added the frontend to `verify.sh`, so every backend commit and CI's `verify` job needed Node and `web/terminal/node_modules`. Its fast hook ran the backend checks only when a staged path matched a fixed list, so a commit touching only `clippy.toml` (D73) or `.github/` ran nothing and passed.

**Rejected:**
- *The frontend inside `verify.sh`.* Every backend clone needs `npm ci` before it can commit, and CI installs Node for `verify` as well as for `frontend`.
- *Backend checks only for a list of backend paths.* Fails open: a path nobody listed skips the gate.
- *Full `verify.sh` on every commit, frontend commits included.* Minutes of cargo on each frontend commit, for checks that can't see the frontend.

**Why:** Each side's gate needs only its own toolchain, and anything not provably frontend-only gets the full backend checks, so the local gate fails closed.

**Consequence:**
- A commit that mixes frontend and other files runs only `verify.sh` locally; CI's `frontend` job checks its frontend half.
- `frontend` isn't a required check on `main` yet, so a red `frontend` job doesn't block a merge. **(Follow-up: Temi adds it to branch protection.)**
- The Claude Code hook still runs the full `verify.sh` when the git hook isn't installed: it runs before the files are necessarily staged.

---

## D87 — Work order: critical work first, then the demo sprint, then M1

*(Amended by D90: `scripts/work next` applies this order over REST; the build account's frontend builder takes Jutin's frontend issues while he's away, claimed by the `wip` label instead of reassignment.)*

**Date:** 2026-09-30 · **Status:** Decided (process; Temi's priority call)

**Decision:** Agents take work in a fixed order, recorded in CLAUDE.md's "Current mode":
1. Stalled open PRs (red CI, a conflict or a failed `watchdog/review`), whoever opened them, oldest first. A claim on one lapses after 2 hours without a push.
2. Issues labelled `critical`, in any milestone.
3. Backend issues in the demo sprint (#62), lowest demo stage first. M1 issues in the blocking chain of the next demo issue count as demo work.
4. The rest of M1, only when no demo issue is left to take.

`critical` means red CI on `main`, a bug that stops or corrupts the live read path (following, pool state, recording or replay), or a security problem. An agent that applies the label says which part of the bar the issue meets. An issue is claimed by an assignee and a claim comment before the first commit, and it counts as taken once it has an assignee or an open PR that closes it. That lets several sessions pick work at the same time without a coordinator.

**Rejected:**
- *The demo sprint only, until it ships.* A stalled follower or a red `main` would wait behind features that depend on them. The demo shows the M0 and M1 engine, so a critical bug there breaks the demo too.
- *A fixed share of sessions per track (e.g. one in three on M1).* Sessions don't see what the others picked, so nothing could enforce the share.
- *Milestone order: finish M1, then M2.* It delays anything showable by weeks, and M1's open issues (#40, #41) don't affect the demo until replay mode (#88).

**Why:** The demo is the priority, and the engine it runs on must stay correct. A written order lets every session choose the same way. Rule 1 covers every stalled PR, not only a session's own: sessions restart and share one GitHub account, so none can know which PRs it opened. Scoping it to "your own" left #57, the fix for the one critical bug (#46), unattended while #46 counted as taken.

**Consequence:**
- Non-critical M1 work waits until the demo sprint has no backend issue left to take. When the sprint reaches replay mode (#88), its chain pulls #42 forward, and through #42, #40 and #41. #46 is `critical`, since the follower stalls forever.
- The sprint's scope and shortcuts are recorded separately (#74).
- When the sprint ends, this entry gets an amendment note and CLAUDE.md's current mode returns to milestone order.

---

## D88 — An RPC endpoint that can't answer stops the run with an error naming its flag

**Date:** 2026-09-30 · **Status:** Decided (from running the M1 demo; dev and staging, D17; amends D80 and D82)

**Decision:**
- **Check before the run.** Before anything else starts, `engine follow` asks its call endpoint for the latest block number, then makes one plain `eth_call` at that block (to the zero address, no data), which any node answers with empty bytes. A failure that isn't the node's answer is retried with the usual backoff for up to 5s. If nothing has answered by then, or the node answers the call with an error, the binary exits non-zero with an error that names `--call-rpc` and gives the endpoint's last error.
- **An outage is not an answer.** PublicNode's `-32701 no available nodes found for platform base-rpc` joins rate limits and lagging nodes (#47) on the call worker's retry list, so it never reaches the core, or the input log, as a failed call.
- **A limit during the run.** A call still unanswered 60s after its first attempt makes the call worker give up on the endpoint: it stops calling and reports the error, and the binary drops the core and exits non-zero with the same message. Until the core is gone the worker holds every call it hasn't answered, because `ChannelRpc` takes a dropped reply for a dead worker and panics.
- **Each attempt is bounded.** An attempt gets the time left until its limit, at least 1s, and one that runs out counts as unanswered ("no reply within …s"). So an endpoint that takes the connection and never replies fails the check in 5s and stops a run 60s into its silence, like one that refuses.
- **The block endpoint too.** The head follower (D80) gets the same check, on what one poll reads (the latest block number, that block's header and its logs), and the same 60s limit on each read during a run. Reading a height again when the node doesn't know its block's hash (#57) is bounded by the same 60s. When it gives up it returns the error, which ends the core's blocks; the core finishes what's in flight, and the binary exits non-zero with an error naming `--rpc`.

**Found while running the demo:** On 2026-09-30 PublicNode answered every `eth_call` with `-32701`. The call worker handed that error to the core as the call's answer, as it does a revert, so `engine follow --minutes 1 --check-every 5` ran its full minute and printed 25 failed verification calls, 1,081 failed bootstraps and no pools tracked. The summary read as a broken engine. Pointing `--call-rpc` at Base's endpoint instead ran for more than 7 minutes on its rate limit (D82). A dead block endpoint was worse: the head follower retried every read forever and checked `--minutes` only between polls, so `engine follow --minutes 1 --rpc http://127.0.0.1:1` never ended and printed nothing.

**Found in review (#61):** the limits were checked only when an attempt returned an error, and the HTTP client has no request timeout. A local listener that accepted connections and never replied kept `engine follow` running, silent, past 100s.

**Rejected:**
- *Exit non-zero when every call in a run failed.* It reports only after the whole run. It still records the outage as the chain's answers. And it can't tell an outage from real failed answers, such as reverts, or PublicNode's archive refusals after a stall.
- *The check alone.* An endpoint that fails mid-run would still turn into failed calls, or, with its errors retried, hang the end of the run: the core finishes only once every call it made is answered.
- *Retrying outages without a limit.* The same hang, with no end.
- *A request timeout on the HTTP client instead of bounding attempts.* One fixed timeout can't fit both the 5s check and the 60s run limit, and it would leave the bound to one implementation, untested by the scripted endpoints the limits are tested on.
- *A short limit, about 10s, in place of the check.* It would give up mid-run on a single rate-limit window (Base's is 30s, D82) or a brief outage. The check stops a dead endpoint sooner, before anything starts.
- *Telling the core the endpoint is down (a third `CallResult`).* Which endpoint works is the I/O layer's business. The core can't act on it, and the input log would hold an outage as if the chain had said it.
- *Dropping the core at once when the follower gives up, as when the call worker does.* Ending its blocks lets the core finish cleanly: the end of its blocks is recorded and its calls are answered, so the recording replays as a complete run. The call worker can't offer that, since the core can't finish without the answers it's waiting on.

**Why:** Whoever runs the demo learns within seconds that the endpoint is at fault, not the engine, and the input log only ever holds what the chain answered.

**Consequence:**
- A dead endpoint, call or block, stops `engine follow` in about 8s; one that dies mid-run stops it about a minute after its first unanswered read. Either way the exit code is 1 and the error ends "Pass --call-rpc with another Base RPC URL." or "Pass --rpc with another Base RPC URL."
- Base's own endpoint passes the check but can't keep up with a run's calls: with `--call-rpc https://mainnet.base.org`, a call went 64s without getting past the rate limit (`-32016`) and the run stopped after 152s with that error, where before it ran for over 7 minutes (verification.md). So it now fails the same clear way as a dead endpoint.
- A run stopped mid-way prints the error, not a summary. When the call worker gave up, the recording ends with calls unanswered, like a crashed run's: replaying it reports the core waiting for an input. When the follower gave up, the recording is complete and replays.
- The limit applies wherever chain I/O runs. In production, failover to the second provider (D16) should replace giving up; until it's built, the engine stops.
- A long-running host gives up the same way. The always-on demo (#82) would exit on any upstream outage longer than 60s, and the free endpoints have them; a restart there means a new core instance, a cold bootstrap of every pool and a new recording. So #82 needs a supervisor with a restart policy, and probably a `--give-up-after` flag: 60s for the CLI, longer or off on the host.

---

## D89 — Mutation testing reuses its builds and results across runs

**Date:** 2026-10-01 · **Status:** Decided (from the testing review; #94)

**Decision:** Mutation testing (cargo-mutants 27.1.0) measures whether the tests notice the code doing the wrong thing, which line coverage can't. cargo-mutants changes the code one small mutation at a time and counts a mutant as caught when some workspace test fails. `scripts/mutants.sh` runs it so that work carries over from one run to the next:
- **Builds.** Each worker is a persistent git worktree under `target/mutants/workers/` with its own target dir under `target/mutants/targets/`. A run checks every worker out at a snapshot of the working tree (uncommitted changes included, through a temporary index), so cargo rebuilds only the crates that changed. cargo-mutants then mutates its worker in place, one shard per worker. A `mutants` Cargo profile drops debug info.
- **Results.** A ledger (`target/mutants/ledger.txt`) holds every mutant caught or unviable since the last fresh run, and later runs skip them (`--iterate`) until `--fresh`. Entries for code that no longer exists are dropped.
- **Diff mode.** `--diff BASE` tests only mutants in code changed since BASE, as a fresh verdict, and leaves the ledger alone. A diff with no Rust in it stops before building anything.
- **The unmutated workspace passes first.** cargo-mutants' baseline runs only the mutated packages' tests, so if another package's test already failed, every mutant would look caught. The script runs the whole workspace's tests on the snapshot before any shard starts.
- **CI:** the `mutants` workflow. On PRs that touch Rust it tests the changed code. Nightly on `main` it tests every mutant not in the ledger, and the ledger is cached per ISO week, so the week's first run is fresh. The workers' target dirs are cached from `main`'s runs. It isn't a required check.

**Found while measuring** (a 4-core container, 2026-09-30 and 2026-10-01):
- cargo-mutants' default copies the tree for each job and builds the copy cold. With 3 jobs, each copy's first build took 800–900s under contention (123s alone), and 18 of about 400 mutants were done after 25 minutes.
- With the harness, a worker's first build took 68.5s (`mutants` profile) and the next run's took about 1s. A second run over `venues/src/v2.rs` took 56s end to end, skipping the 19 of its 30 mutants already settled.
- A fresh run over the whole workspace at `e8a2fcd` tested 650 mutants in 28.7 minutes on 2 workers: 372 caught, 93 missed, 2 timed out, 183 unviable. The ledger then held 555, so the next run tests only the 95 left.

**Rejected:**
- *cargo-mutants' own tree copies (`--jobs N`).* Every run, and every job in it, starts from a cold build.
- *Copying a warm `target/` into each copy (`--copy-target`).* Measured: copying the 5.2G `target/` took 145s, and cargo still recompiled 209 crates (94s), slower than building cold.
- *sccache.* It shares compiled dependencies across directories, but not the workspace's own crates, which build incrementally. The persistent workers keep both.
- *A required check.* Some untested code is already known (the binaries' command wiring, error `Display` impls), and a hard gate would block every PR that touches it. The check reports to the author and the watchdog instead.
- *Every mutant every night.* Repeats the same verdicts; the weekly fresh run is what catches a test that got weaker.

**Why:** Line coverage was 82%, yet both real bugs found on 2026-09-30 (#46, #52) passed their author's tests and were caught by independent review. Mutation testing measures what the tests check, and it's only worth running on every PR if it's cheap.

**Consequence:**
- Before opening a PR, `scripts/mutants.sh --diff origin/main` shows whether the new tests catch mutations of the new code. CLAUDE.md says so.
- The ledger assumes tests don't get weaker: a test deleted after its mutants were caught goes unnoticed until the next fresh run (weekly in CI, `--fresh` locally).
- CI recreates the worktrees on each run, so it rebuilds the workspace's own crates every time; only dependencies come warm from the cache.
- The workers are git worktrees, so they appear in `git worktree list`. `scripts/mutants.sh --clean` removes them, their builds and the ledger. After a plain `cargo clean`, it (or `git worktree prune`) clears the entries left behind.

---

## D90 — Build work runs on a second account's cloud sessions, under an orchestrator

**Date:** 2026-10-01 · **Status:** Decided by Temi (process)

**Decision:** Most build work moves to Claude Code cloud sessions on a second Claude account (the **build account**), whose GitHub connection acts as `AndySakov`, like Temi's own sessions. Temi steers it through one long-lived **orchestrator** session on that account, which starts and tracks the other sessions. The protocol is in `docs/agents/handoff/`.
- **Roles.** One backend builder and one frontend builder at a time, one session per issue, and one watchdog session per PR (D81's separate reviewer). The orchestrator picks work with `scripts/work next`, which applies D87's order, starts sessions, dispatches reviews and reports to Temi. It writes no code.
- **The gate.** The watchdog posts its verdict as a PR comment whose first line names the head commit and passes or fails. The `watchdog-status` workflow turns that line into the `watchdog/review` commit status, linked to the comment, and only for the PR's current head. Branch protection is unchanged.
- **Merging.** The builder merges with a merge commit (`scripts/work merge`), which refuses unless `verify`, `frontend` and `watchdog/review` all pass on the head, there's no conflict and nothing is labelled `hold`. `frontend` gates merges this way, though branch protection doesn't require it yet (D86).
- **Claims.** `scripts/work claim` adds the `wip` label, assigns the claimer if nobody is assigned, and comments with the session link. While Jutin is away, the frontend builder takes his frontend issues and leaves him assigned, so he sees what's left when he's back; `wip` is the claim there. An acceptance criterion that names Jutin's review is met by Temi's review while he's away, and Jutin is tagged for a look.
- **Partial work.** An acceptance criterion that can't be met from the repo (a signup only Temi can do; a frontend slice waiting on its backend) moves to a new issue linked both ways, labelled `needs-temi` or blocked by the backend issue, and the original closes with the rest.
- **Temi's brakes.** The `hold` label on an issue or PR stops agents taking or merging it. `needs-temi` marks work only Temi can unblock.
- **Cloud sessions are readied by a SessionStart hook** (`.claude/hooks/session-start.sh`): dockerd, the git hooks, buf, npm deps, Playwright's Chromium (or the preinstalled one), and a cargo build warmed in the background.

**Found while setting up (2026-10-01):**
- In cloud sessions GitHub's GraphQL API is refused ("GraphQL is not available from Claude Code sessions"), so `gh issue list`, `gh pr view` and `gh pr checks` fail. REST through `gh api` works.
- Commit-status and check-run writes are refused by the session proxy ("Write access to this GitHub API path is not permitted"), whatever the network level, token or GitHub account connected (a PAT stored as an environment credential changed nothing). Comments, labels, assignees, opening and merging PRs, and workflow dispatch are allowed.
- A cold `scripts/verify.sh` takes 3.5 minutes on a cloud container, and Docker's daemon isn't running at start.
- A session started by another session receives its brief as an automated message. It followed an ordinary work brief, but refused one that read like a credential probe.

**Rejected:**
- *A PAT for `AndySakov` in the build account's environment.* The proxy ignored it, and still blocks status writes and GraphQL for every account.
- *A watchdog that dispatches a workflow with the verdict as inputs.* Two steps that can disagree; a status from a comment can't exist without the findings it links to.
- *Keeping the watchdog on Temi's account.* Temi's usage limits would cap how many PRs merge a day.
- *Builders picking their own issues.* Two builders could race for one issue; one dispatcher can't.
- *Reassigning Jutin's issues.* Temi wants Jutin to see what's left when he's back.

**Why:** The build account has the budget and Temi doesn't, so the token-heavy work (building, reviewing) runs there and Temi spends his limits only on steering and decisions. The gate and the work order stay as they were; only their mechanics change to fit what cloud sessions can do.

**Consequence:**
- The gate has the same trust gap D81 records: any session with write access could post a fake verdict comment, as it could post a status before. CLAUDE.md forbids it, and the status now links to the comment that set it.
- Both Claude accounts act on GitHub as `AndySakov`, so GitHub alone can't tell their work apart; the claim comment's session link can.
- When the build account's budget runs out, the orchestrator stops starting sessions, and Temi's watchdog (or Temi) reviews again.
- When Jutin is back, frontend issues return to him one by one, as he takes them.

---

## D91 — API contract v0: proto3 JSON, decimal strings, generated TypeScript

**Date:** 2026-10-01 · **Status:** Decided, pending Temi's review for Jutin (#75)

**Decision:** The API between the backend and both UIs (D62) is defined in `proto/omnimarket/api/v1` and checked by `buf lint` and `buf breaking` like every other schema (D70). v0 covers the demo's screens (#62). Details and the REST paths are in `frontend.md`; the WebSocket topics are in `data.md`.
- **Wire format:** proto3's JSON mapping, over REST and one WebSocket connection (one JSON object per text frame).
- **Amounts, prices, USD values and percentages are decimal strings**: base 10, no exponent, exact. Token amounts are in whole-token units (already divided by the token's decimals). Percentages are in percent ("-3" is −3%); slippage and fees are basis points (`uint32`).
- **Addresses and hashes are lowercase 0x hex strings**, not `bytes`. Lineage IDs stay `bytes` in the shared `omnimarket.lineage.v1.Lineage` (base64 in JSON).
- **Times are Unix milliseconds** (`uint64 *_ms`), from block time or the det clock. Like every 64-bit integer in proto3 JSON, they travel as strings; the generated TypeScript makes them `bigint`.
- **Lineage (D53, D71).** Every record the server sends carries `Lineage`. A message that presents one backend record (a trade, firing or receipt) carries that record's lineage; one the API assembles (a snapshot, list or feed) gets its own ID from its key and block, with `caused_by` naming what it was built from. Requests carry a client-chosen `client_request_id` instead: the server derives the request's lineage ID from it, and a repeat with the same ID is the same request, which makes quoting, trading and order edits idempotent (build rule 7). The WebSocket envelope (subscribe, snapshot, delta, heartbeat, error) and the parts that only appear inside a record (`TokenRef`, `RouteLeg`, `Fee` and the like) carry none.
- **WebSocket protocol.** The client subscribes to topics; for each, the server sends one snapshot, then deltas, numbered by a per-topic `seq`. A gap means the client resubscribes. Deltas replace the record with the same key; trades append. A heartbeat every second; three missed mark the data stale.
- **Generated code.** TypeScript is generated with `buf generate` and protobuf-es v2 into `web/terminal/src/api/generated` and committed; `npm run api:check` (part of the frontend's `verify`) fails when it's stale. Rust gets the same messages from the `proto` crate.
- **Fixtures.** One proto3 JSON file per message in `web/terminal/src/mocks/fixtures/api/v1`, for MSW, stories and tests. A test checks that every message has one and that each is canonical: it parses, and re-serialises to the same JSON.

**Rejected:**
- *Binary protobuf on the WebSocket.* Smaller, but unreadable in browser devtools, and the demo's rates (≤ 20 ticks a second per token, D43) don't need it. It can be added later as a negotiated encoding of the same messages.
- *Raw integer amounts (wei) as strings.* Equally exact, but every value on screen then needs the token's decimals, and the scaling bugs move into the UI. The server converts once.
- *JSON numbers for amounts.* A double can't hold a 256-bit amount or most prices exactly.
- *`bytes` for addresses and hashes.* proto3 JSON writes them as base64, which no EVM tool or explorer reads.
- *`google.protobuf.Timestamp`.* RFC 3339 strings read well, but block times and the det clock are integer milliseconds, so every conversion is a chance to drift.
- *Hand-written TypeScript types, or OpenAPI.* Two definitions of one contract drift; D62 says the types come from the proto schemas.
- *gRPC-web or Connect services.* A protocol runtime and a server framework the demo doesn't need; v0 documents plain REST paths.
- *Generating the TypeScript at build time instead of committing it.* Every frontend install would need buf, and contract changes wouldn't show in the PR's diff.

**Why:** proto3 JSON keeps one schema for Rust and TypeScript (D41, D62) while staying readable in a browser. Decimal strings are the only JSON form that's exact for both 18-decimal token amounts and USD values, and lowercase hex is what users paste into explorers.

**Consequence:**
- From merge on, `buf breaking` holds the contract: changes are additive, or go to `v2`.
- The UI parses amounts into `decimal.js` (or its fixed-decimal type) and never into `number` (frontend-plan.md).
- #78's server and #63's client follow this protocol; the fixtures are the shared example of each message.

---

## D92 — Every acceptance criterion names the test that proves it, and CI checks it passed

**Date:** 2026-10-01 · **Status:** Decided (process; #98)

**Decision:** A PR body carries an "Acceptance criteria" table (`.github/pull_request_template.md`): one row per acceptance criterion of the issue it closes, quoting the criterion, and the tests that prove it in backticks, or `manual: <evidence>` where no test can. CI's `criteria` job checks the table:
- **Every criterion has a row.** The criteria are the checkbox items under the closed issue's `## Acceptance criteria` heading. A row matches a criterion when its text is the same, ignoring case, runs of whitespace, surrounding quotes and a final full stop. A criterion with no row fails the job, and so does a row that quotes no criterion (usually a misquote).
- **Every named test passed in this CI run.** The job reads `cargo test`'s output from the `verify` job (the workspace tests and the Kafka test) and the frontend's Vitest and Playwright JSON reports from the `frontend` job (`scripts/frontend-passed-tests.sh` turns them into the same `test <name> ... ok` lines). A name matches a passed test's full name or its trailing segments (`::` for Rust paths, ` > ` for frontend titles), so a test that doesn't exist, failed or was ignored fails the job. A backticked file path says where a test is and isn't checked.
- **`manual:` rows are listed** in the job summary for the watchdog, which checks their evidence. A criterion moved to a follow-up issue (D90) is a `manual:` row naming that issue.
- The parsing and checking live in the `criteria` crate (a binary the job builds), so `cargo test` covers them and mutation testing (D89) reaches them. The job reads the PR body and the issues through the REST API when it runs, so after editing the body, re-running the `criteria` job alone picks the edit up. A PR that closes no issue only has its named tests checked.
- It joins `main`'s required checks once it has run green on two PRs (Temi changes branch protection). `scripts/work merge` doesn't require it until then.

**Rejected:**
- *Ticking the issue's checkboxes.* Nobody ticked them, and a tick proves nothing.
- *Test names in the issue instead of the PR.* The tests don't exist when the issue is written.
- *Checking that the named test exists by searching the source.* A test that exists but is ignored, or fails, would pass; the run's output shows what actually passed.
- *A step inside the `verify` job.* It couldn't become a separate required check, and an edited PR body would mean re-running every test.
- *Parsing JUnit XML from every runner.* `cargo test` has no stable JUnit output yet; libtest's plain lines and the frontend's JSON reports are already there.

**Why:** PR bodies claimed criteria in prose, and the watchdog mapped each criterion to a test by hand. A criterion with no test could be claimed and merged. A job that fails on a missing row or a test that didn't pass makes the claim checkable, and leaves the watchdog to judge whether the test proves the criterion and to check the manual evidence.

**Consequence:**
- A criterion only Storybook, a live run or a doc can show needs a `manual:` row, and the watchdog is the only check on it.
- Rewording a criterion in the issue after the PR is open fails the job until the row is updated.
- Contract tests (`forge test`) aren't read yet: `contracts/` doesn't exist. When it does, its output needs adding to the job.
- Whether `actions/download-artifact` finds the earlier attempt's test results when only the `criteria` job is re-run is **(verify)**. If it doesn't, re-run the whole workflow after editing the body.
