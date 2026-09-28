# Architecture

**Status:** Draft. Grows as decisions land.

## Shape (D6)

```
                MegaETH / Base / BNB  (RPC + WebSocket, paid providers)
                        │ blocks, logs, pending state
                        ▼
   ┌──────────────── Chain Engine (one per chain, + warm standby) ───────────────┐
   │  head follower → pool state (in memory) → pricing → router/quoter           │
   │                                       └──→ trigger evaluator                │
   └───────┬───────────────────────────────────────────┬─────────────────────────┘
           │ HOT (gRPC): fired triggers, quotes         │ publishes every state change
           ▼                                            ▼
     Execution service (per chain, D8)            Kafka (system of record)
     build → simulate → sign → submit  ── outcomes ──►  │
     sole owner of wallet nonces                        │
                                       ┌────────────────┼─────────────────┐
                                       ▼                ▼                 ▼
                                  ClickHouse       PostgreSQL        WS Gateway
                                  candles, trades  users, wallets,   feeds to
                                  analytics        orders, positions terminal
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

## Open for spec

- Exactly-once trigger firing across engine → execution (D8)
- Engine recovery: snapshot format, replay window, time to recover
- Standby and failover: how the standby stays warm, how double-firing of triggers is prevented
- Where trigger orders live durably (engine memory is a cache; the source of truth is TBD)
