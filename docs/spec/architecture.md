# Architecture

**Status:** Draft. Grows as decisions land.

## Shape (D6)

```
      MegaETH / Base / BNB  (providers; own Base node in prod, D44)
                        │ logs, mini-blocks, blocks
                        ▼   (every input also recorded → input log, D54)
   ┌──────────────── Chain Engine (one per chain, + Kafka-fed standby, D40) ───────┐
   │  head follower → pool state (in memory) → pricing → router/quoter             │
   │                                       └──→ trigger evaluator                  │
   └───────┬─────────────────────────────────────────────┬─────────────────────────┘
           │ HOT (gRPC): firings (epoch-fenced), quotes   │ publishes every state change
           ▼                                              ▼
     Execution service (per chain, D8, D42)         Kafka (system of record)
     route → simulate → executor signs → submit ─outcomes─► │
     owns executor wallets + their nonces                   │
           │ signed intents (Permit2) via our router        │
           ▼                                   ┌────────────┼──────────────┐
     Router contract (immutable, D26)          ▼            ▼              ▼
                                          ClickHouse    PostgreSQL     WS Gateway
     Independent watcher (D55) ──────►    candles,      users, orders, feeds to
     reconciles on-chain vs our records   lineage       positions,     terminal
                                                        nonce ledger
```

## Hot path vs cold path

| | Hot | Cold |
|---|---|---|
| Work | Pool state, pricing, quotes, trigger evaluation, execution | Candles, history, PnL, analytics, UI feeds, audit |
| Latency budget | Milliseconds | Seconds |
| Transport | In-process, direct calls | Kafka |
| State | In memory, per chain | ClickHouse (analytics), PostgreSQL (transactional) |

## Chain Engine responsibilities

- Follow chain head; handle reorgs (roll pool state back, emit corrections)
- Keep reserves / ticks / liquidity for every tracked pool
- Price every tracked token (USD via stable/native reference pools)
- Answer quote requests (routes across pools)
- Evaluate trigger orders on each relevant price change
- Publish pool updates, swaps, prices, and reorg corrections to Kafka
- Emit lineage IDs on every record (D53); read inputs only through recorded, deterministic interfaces (D49, D54)

## Execution service responsibilities

- Receive firings and click requests; route, simulate (blocking), have an executor sign locally, submit (D30, D42, D46)
- Own the executor wallets and their nonces (D32); never send from user wallets on the trade path
- Enforce per-trade breakers (D56); publish outcomes to Kafka

## Around the hot path

- **Router contract:** executes signed intents from listed submitters only (D26, D58)
- **Independent watcher:** separate code, provider and region; reconciles every on-chain fill (D55)
- **Brakes:** automatic breakers and manual levels that stop our automation, never users (D52, D56)

## Open for spec

- ~~Exactly-once trigger firing across engine → execution (D8)~~ → decided (D35)
- ~~Engine recovery~~ → snapshot every ~30s + Kafka replay + reconciler catch-up, < 10s (D40)
- ~~Standby and failover~~ → Kafka-fed warm standby, leader lease with fencing epochs, ~3–5s failover (D40)
- ~~Where trigger orders live durably~~ → Postgres; engine memory is a cache (D35)
