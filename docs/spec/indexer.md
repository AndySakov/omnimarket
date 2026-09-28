# Indexer

**Status:** Draft. Background: [primer](../primers/indexing.md), [iTRY case study](../primers/case-study-itry.md).

## Two jobs (D9)

| | Live state | History |
|---|---|---|
| Lives in | Chain Engine (hot path) | Separate cold-path job |
| Output | In-memory pool state + Kafka events | ClickHouse (swaps, pool events, candles) |
| Needs history? | No: bootstraps from contract reads | Yes: backfill depth is a product choice |
| Latency | ms | Can lag, can restart |
| Build/buy | Build | Either |

## Tip following (D10)

| Chain | Fast loop (provisional) | Reconciler loop (canonical) |
|---|---|---|
| MegaETH | Realtime API filtered log subscription, per ~10ms mini-block | `getLogs` per ~1s EVM block |
| Base | Flashblocks `pendingLogs`, ~200ms | `getLogs` per 2s block |
| BNB | Log subscription per 0.45s block | `getLogs` per block, trailing |

The reconciler:
- confirms fast-loop events (provisional → confirmed)
- detects reorgs (parent hash mismatch) and dropped preconfirmations (event seen in fast loop, absent from canonical block)
- fills gaps when a subscription drops (block-number continuity check)
- emits corrections to Kafka so downstream consumers can undo

## Bootstrap (D9)

1. Discover pools (see open question: coverage).
2. Pick a block N. Read each pool's state at N (`getReserves`, `slot0`, liquidity, tick data).
3. Buffer live events from subscription start; apply everything after N in order.
4. Mark the engine ready. Only then serve quotes and evaluate triggers.

## Pool coverage (D11)

| Tier | Holds | Enters when | Leaves when |
|---|---|---|---|
| Known | Metadata (tokens, fee tier, DEX, creation block) | Pool creation event | Never |
| Active | Full state, priced, routable | Base-asset pair above liquidity floor · inside new-pool grace window · referenced by a position, trigger, or copied wallet | None of those hold (after hysteresis) |

Promotion reuses the bootstrap procedure for a single pool: read state at block N, apply buffered events after N. Tuning parameters (TBD): liquidity floor per chain, grace window, demotion hysteresis.

## Certainty levels & undo (D12)

| Status | MegaETH | Base | BNB |
|---|---|---|---|
| Provisional | Mini-block | Flashblock | n/a |
| Confirmed | EVM block | Block | Block |
| Final | L1-final batch **(verify timing)** | L1-final batch (~15–20 min) | Fast finality (~1.1s) |

Undo tiers: **hot** (memory, provisional + ~10s) → **warm** (Kafka before/after events, up to final) → **rebuild** (bootstrap). The cold path advances a per-chain finality watermark.

## Open questions

- **Tuning:** liquidity floor, grace window, demotion hysteresis, hot-undo window per chain
- ~~Uniswap v3 tick bootstrap cost~~ → **decided: batched reads** (multicall / lens-style). Implementation details deferred.
- ~~Uniswap v4 hooks~~ → **decided: full support** in phase 1. Hook pools whose math can't be replicated are quoted by simulation (opaque venue type, which is now a phase 1 requirement).
- **RPC providers & budget:** must support MegaETH Realtime API and Base Flashblocks
- **History job:** build vs managed, and backfill depth
