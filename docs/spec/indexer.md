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

| Chain | Fast loop (provisional) | Reconciler loop (canonical) |
|---|---|---|
| MegaETH | Realtime API filtered `logs` subscription, per ~10ms mini-block | `getLogs` per ~1s EVM block |
| Base | `newHeads` + one `getLogs` per 2s block (canonical, D77) | `getLogs` per block, trailing |
| BNB | `newHeads` + one `getLogs` per 0.45s block | `getLogs` per block, trailing |

Streams are one message per block wherever the chain allows, because providers bill every pushed event (D16). In production, Base's fast loop reads blocks from our own Base node (D44), with providers as fallback.

**Head follower (built, `chain-io`, D80).** On its own thread with its own runtime, it asks the node for the latest block number every 500ms. For each block after the last one delivered, it reads the header by number and the followed logs by block hash (`eth_getLogs` with `blockHash` and the event signatures), sorts the logs by index and sends the block down a channel to the core. A block the poll skipped is fetched by number, and a failed call is retried with backoff (250ms doubling to 8s), never skipped, so the core sees every block in order. A header the node doesn't have yet (load-balanced nodes can lag each other) waits for the next poll.

**Free Base RPC limits (dev, D17):** HTTP only; `eth_getLogs` is limited to a 2,000-block range; historical state reads work at least 1M blocks back. Measured 2026-09-30.

**Engine core (built, `engine`).** One task on a current-thread runtime (D74) reads blocks through a channel `EventSource` and the clock through `det`, both recorded to `inputs.base`. It keeps the last 128 block hashes. A block whose number isn't the next one is a follower bug and stops the core; a block whose parent isn't the held head is a reorg (undo arrives with D12's tiers). `engine follow` runs it live; `engine replay` reruns a recording from Kafka.

The reconciler:
- confirms fast-loop events (provisional → confirmed)
- detects reorgs (parent hash mismatch) and dropped preconfirmations (event seen in fast loop, absent from canonical block)
- fills gaps when a subscription drops (block-number continuity check)
- emits corrections to Kafka so downstream consumers can undo

## Bootstrap (D9)

1. Discover pools and bonding curves from factory / launchpad create events (D11, D36).
2. Pick a block N. Read each pool's state at N (`getReserves`, `slot0`, liquidity, tick data), batched via multicall or a lens contract (D13).
3. Buffer live events from subscription start; apply everything after N in order.
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

Custom, 30 days of backfill per chain at launch: the reconciler's `getLogs` loop pointed at past block ranges, writing idempotently into ClickHouse (keyed by chain, block hash, log index) so backfill can restart and overlap the live feed.

## Open questions

- **Tuning:** liquidity floor, grace window, demotion hysteresis, hot-undo window per chain
- ~~Uniswap v3 tick bootstrap cost~~ → **decided (D13): batched reads** (multicall / lens-style). Implementation details deferred.
- ~~Uniswap v4 hooks~~ → **decided (D14): full support** in phase 1. Hook pools whose math can't be replicated are quoted by simulation (opaque venue type, which is now a phase 1 requirement).
- ~~RPC providers & budget~~ → **decided (D16):** Chainstack Pro primary, QuickNode fallback, per-block streams.
- ~~History job: build vs managed~~ → **decided (D15): build**, backfilling **30 days** per chain at launch.
