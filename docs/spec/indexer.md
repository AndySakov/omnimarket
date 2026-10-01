# Indexer

**Status:** Draft. Background: [primer](../primers/indexing.md), [iTRY case study](../primers/case-study-itry.md).

## Two jobs (D9)

| | Live state | History |
|---|---|---|
| Lives in | Chain Engine (hot path) | Separate cold-path job |
| Output | In-memory pool state + Kafka events | ClickHouse (swaps, pool events, candles) |
| Needs history? | No: bootstraps from contract reads | Yes: backfill depth is a product choice |
| Latency | ms | Can lag, can restart |
| Build/buy | Build | Build (D15) |

## Tip following (D10, streams amended by D16)

| Chain | Fast loop | Reconciler loop (canonical) |
|---|---|---|
| MegaETH | Realtime API filtered `logs` subscription, per ~10ms mini-block (provisional) | `getLogs` per ~1s EVM block |
| Base | Canonical blocks: `newHeads` (production) or polling (D80) + one `getLogs` by block hash per 2s block (D77) | None: the fast loop is already canonical (D84) |
| BNB | `newHeads` + one `getLogs` per 0.45s block (canonical) | None, as for Base (D84) |

Streams are one message per block wherever the chain allows, because providers bill every pushed event (D16). In production, Base's fast loop reads blocks from our own Base node (D44), with providers as fallback.

**Head follower (built, `chain-io`, D80, D88).** On its own thread with its own runtime, it asks the node for the latest block number every 500ms. For each block after the last one delivered, it reads the header by number and the followed logs by block hash (`eth_getLogs` with `blockHash` and the event signatures), sorts the logs by index and sends the block down a channel to the core. A block the poll skipped is fetched by number, and a failed call is retried with backoff (250ms doubling to 8s), never skipped, so the core sees every block in order. A header the node doesn't have yet (load-balanced nodes can lag each other) waits for the next poll. If the node doesn't know a block's hash when its logs are read (`-32001 block not found`), the block was either replaced after its header was read (a reorg) or a lagging node hasn't got it yet; either way the follower backs off (250ms doubling to 8s), reads the height's header again and continues with whatever is canonical now, instead of retrying a hash that may never come back. The check before a run reads what one poll reads, retried for up to 5s each, so a dead endpoint stops `engine follow` within about 8s, before Kafka or the core start. During a run, a read still unanswered after 60s, or a height still unreadable after 60s of reading it again, makes the follower give up: it returns the error, which ends the core's blocks, and `engine follow` exits with it once the core has finished what's in flight. Either way the error names the endpoint and says to pass `--rpc` with another Base RPC URL.

**Free Base RPC limits (dev, D17):** HTTP only; `eth_getLogs` is limited to a 2,000-block range; historical state reads work at least 1M blocks back. Measured 2026-09-30.

**Engine core (built, `engine`).** One task on a current-thread runtime (D74) reads blocks through a channel `EventSource` and the clock through `det`, both recorded to `inputs.base`. It keeps the last 128 block hashes. A block whose number isn't the next one is a follower bug and stops the core; a block whose parent isn't the held head is a reorg (undo arrives with D12's tiers). `engine follow` runs it live; `engine replay` reruns a recording from Kafka.

**Uniswap v2 (built, `venues::v2`, engine).** `Sync` carries a pair's full reserves after every change, so it alone sets the state: no reserve read is needed at discovery. A pair is known from its factory's `PairCreated`, or from its first `Sync` (D80). A pair first seen trading is held unproven, with its `Sync`s buffered in order. Its `token0()` and `token1()` are read in one Multicall3 `aggregate3` call per 100 new pairs, at the block it was seen. It is tracked only if its address is the factory's CREATE2 address for those tokens; otherwise it is rejected for good (a fork or a fake). A verification call that fails as a whole forgets its pairs, and each is proven again the next time it trades. Every change publishes a `PoolUpdate` with before and after to `pool-updates.base`. The Base deployment's factory and init code hash are checked against the live WETH/USDC pair in a test.

**Uniswap v3 (built, `venues::v3`, engine).** A pool is known from its factory's `PoolCreated` (uninitialized and empty, then `Initialize` and `Mint` build it), or from its first `Initialize`, `Swap`, `Mint` or `Burn` if it was created before the engine started (D80). `Swap` carries the full price, tick and active liquidity, but `Mint` and `Burn` are deltas to the tick table, so a pool first seen mid-life needs its state read. That read happens at the block it was first seen in, which already includes that block's events; events from later blocks are buffered and applied after the read. The read takes three rounds, all at that block and all through Multicall3:
1. `token0`, `token1`, `fee`, `tickSpacing`, `slot0` and `liquidity`, for up to 50 pools per call. The pool is tracked only if its address is the factory's CREATE2 address for its tokens and fee; otherwise it is rejected for good (a fork, such as Slipstream, or a fake).
2. Every bitmap word that can hold its ticks, 500 words per call.
3. Uniswap's TickLens `getPopulatedTicksInWord` for each non-empty word, 50 words per call.

The pool is then published as discovered (every initialized tick in the update), and the buffered events apply on top. A failed call abandons the read; the pool starts again the next time it's seen. Each read is numbered, and every call carries its read's number, so an answer or failure from an abandoned read that arrives during the next one is ignored.

**v3 bootstrap cost** (calls per pool, beyond its 1/50 share of an identity call): tick spacing 200 (1% fee) has 36 words, so 1 word call; spacing 60 (0.3%): 116 words, 1 call; spacing 10 (0.05%): 694 words, 2 calls; spacing 1 (0.01%): 6,932 words, 14 calls, the worst case. Then about one TickLens call per 50 non-empty words, usually 1. Measured live on 2026-09-30 over 15 minutes: 3,296 bootstrap calls identified 807 addresses emitting v3 events, of which 467 were Uniswap v3 pools (the rest forks or fakes), so about 7 calls per tracked pool including the identity share. Every call went to PublicNode (D82) with no rate limit.

**Trade records (built, engine, D97).** Every Swap on a tracked pool publishes one `Trade` to `trades.base`, keyed by pool: v2's `Swap` (followed for this alone; `Sync` sets the state) and v3's (which also sets the state). The trade is the pool's net change in each token, read as the token against its quote asset: the pool's token that comes first in the config's quote-asset list (Base: USDC, USDT, WETH), or token1 when neither is listed. Paying out the token is a buy, taking it in a sell; the price is quote base units per token base unit × 10^36, since the engine doesn't know decimals. A pool not yet tracked buffers its swaps with its other events, the v3 read's own block included, and publishes them once its proof or read completes; if that fails they're dropped and counted (`trades_dropped`). A run's summary carries a digest over every trade published, so a replay can be checked against the live run. Trades share pool updates' block-hash key, so tiered undo (#40) will retract a reorged-out block's trades with its pool updates. The trader, `tx.from`, isn't read: it costs a full-transaction block read per block, or a read per transaction **(verify)**.

**Which node errors reach the core (#47, D88).** The call worker retries, with backoff and below the core, anything that isn't the node's answer to the call: transport failures, rate limits (HTTP 429, `-32016`, `-32005`, "rate limit"), a block the node doesn't have yet ("block not found", "header not found", from a lagging load-balanced node), and no node at all (PublicNode's `-32701 no available nodes found for platform base-rpc`). Everything else is recorded as the call's result and handed to the core as `CallResult::Failed`: a revert, a bad argument, or state older than the node keeps (PublicNode's "archive requests require a personal token").

**A call endpoint that can't answer (D88).** `engine follow` first checks its call endpoint: the latest block number, then a plain `eth_call` at that block (to the zero address, no data). A failure that isn't an answer is retried for up to 5s, so a dead endpoint stops `engine follow` within about 8s, before anything else starts. The node answering the check's call with an error fails the check at once. During a run, a call still unanswered after 60s makes the worker give up: it stops calling and reports the error, and `engine follow` stops the core and exits with it. Until the core is gone the worker holds every call it hasn't answered, so the core never sees one dropped. The 60s rides out a rate-limit window (Base's is 30s, D82) or a brief outage. Each attempt is bounded too, so an endpoint that takes the connection and never replies counts as unanswered. An attempt gets the time left until the limit, at least 1s, and this holds for the head follower's reads as well. Base's own endpoint can't keep up with a run's calls, though: with `--call-rpc https://mainnet.base.org`, a call went 64s without getting past the rate limit and the run stopped after 152s, where it used to run for over 7 minutes (verification.md). Either way the run exits with code 1 and an error that names the endpoint, gives its last error and says to pass `--call-rpc` with another Base RPC URL.

**Shadow state check.** With `--check-every N`, every N blocks the engine takes the next 20 tracked v2 pairs and the next 20 initialized v3 pools, in address order, wrapping round. It reads, at the block it just applied, each pair's `getReserves()` and each pool's `slot0()`, `liquidity()` and one initialized tick's `ticks()`, rotating through the pool's tick table from one check to the next so every tick is compared in time, in one Multicall3 call per venue. The engine compares the answer with the reserves it held after that block and counts matches and mismatches, logging each mismatch as an error.

The reconciler, on chains whose fast loop is provisional (MegaETH):
- confirms fast-loop events (provisional → confirmed)
- detects reorgs (parent hash mismatch) and dropped preconfirmations (event seen in fast loop, absent from canonical block)
- fills gaps when a subscription drops (block-number continuity check)
- emits corrections to Kafka so downstream consumers can undo

On chains followed by canonical blocks (Base, BNB), there is no separate reconciler (D84). Its jobs are already done: nothing is provisional to confirm or drop; the head follower fills gaps by number; the core detects reorgs by parent hash, and tiered undo walks back and emits corrections (D12). A node silently leaving logs out of a `getLogs` answer is the one failure a trailing re-read would catch, and the shadow state check catches its effect on tracked pools.

## Bootstrap (D9)

1. Discover pools and bonding curves from factory / launchpad create events, or from their first followed event (D11, D36, D80).
2. Read state at the block N a pool is first seen in, batched via Multicall3 or a lens contract (D13), only where events can't rebuild it:
   - **v2:** no state read. `Sync` carries the full reserves, so the first `Sync` is the state; the batched read is `token0()`/`token1()` for the CREATE2 proof.
   - **v3:** `slot0`, liquidity and the tick table, because `Mint` and `Burn` are deltas (see the v3 paragraph above).
3. Buffer the pool's events from blocks after N; apply them in order once the read completes. Events in block N itself are already in the read.
4. Mark the engine ready. Only then serve quotes and evaluate triggers.

## Pool coverage (D11)

| Tier | Holds | Enters when | Leaves when |
|---|---|---|---|
| Known | Metadata (tokens, fee tier, DEX, creation block) | Pool creation event | Never |
| Active | Full state, priced, routable | Base-asset pair above liquidity floor (±2% depth, D24) · inside new-pool grace window · referenced by a position, trigger, or followed wallet (D38) · bonding curve not yet graduated (D36) | None of those hold (after hysteresis) |

Promotion reuses the bootstrap procedure for a single pool: read state at block N, apply buffered events after N. Tuning parameters (TBD): liquidity floor per chain, grace window, demotion hysteresis.

## Certainty levels & undo (D12)

| Status | MegaETH | Base | BNB |
|---|---|---|---|
| Provisional | Mini-block | n/a (D77) | n/a |
| Confirmed | EVM block | Block | Block |
| Final | L1-final batch (EigenDA data + L1 commitment; lag **to measure**) | L1-final batch (~15–20 min) | Fast finality (~1.1s) |

Undo tiers: **hot** (memory, provisional + ~10s) → **warm** (Kafka before/after events, up to final) → **rebuild** (bootstrap). The cold path advances a per-chain finality watermark.

## RPC providers & budget (D16)

| Environment | Provider |
|---|---|
| Dev, CI, staging (D17) | Free tiers + public endpoints (Base public RPC over HTTP, MegaETH public endpoint) |
| Production (and load tests that exceed free quotas) | Chainstack Pro (~$199/mo) primary · QuickNode Build (~$49/mo) failover |

Estimated load after per-block streams, per month **(verify)**:

| Source | Requests / events |
|---|---|
| Fast-loop pushes | Base ~26M (tick + getLogs) · BNB ~11.5M · MegaETH scales with volume |
| Reconciler `getLogs` | ~10M |
| Simulations (shadow mode, opaque v4 venues) | 3–25M |
| Bootstrap + history backfill (batched) | <1M each |

Before paying: measure real event rates per chain, and confirm per-event WebSocket billing and MegaETH mini-block `logs` support on the chosen provider.

## History job (D15)

Custom, 30 days of backfill per chain at launch: the head follower's per-block `getLogs` loop (the reconciler's, on MegaETH) pointed at past block ranges, writing idempotently into ClickHouse (keyed by chain, block hash, log index) so backfill can restart and overlap the live feed.

## Open questions

- **Tuning:** liquidity floor, grace window, demotion hysteresis, hot-undo window per chain
- ~~Uniswap v3 tick bootstrap cost~~ → **decided (D13): batched reads** (multicall / lens-style). Implementation details deferred.
- ~~Uniswap v4 hooks~~ → **decided (D14): full support** in phase 1. Hook pools whose math can't be replicated are quoted by simulation (opaque venue type, which is now a phase 1 requirement).
- ~~RPC providers & budget~~ → **decided (D16):** Chainstack Pro primary, QuickNode fallback, per-block streams.
- ~~History job: build vs managed~~ → **decided (D15): build**, backfilling **30 days** per chain at launch.
