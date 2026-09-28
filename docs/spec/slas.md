# SLAs & Latency Budgets

**Status:** Draft. Decisions: D42–D46.

Targets measure **internal latency**: from our engine receiving the event (or the API receiving the click) to the transaction being broadcast. End-to-end latency from the chain's timestamp is reported alongside, not targeted.

## Targets (p99)

| Metric | Target |
|---|---|
| Price move → trigger broadcast | ≤ 50ms |
| Click → broadcast (any payment token) | ≤ 100ms |
| Quote latency | ≤ 10ms |
| Price tick → client | ≤ 100ms |
| Indexer lag | ≤ 1 block (Base/BNB), ≤ 250ms (MegaETH) |
| Engine recovery / failover | < 10s / ≤ 5s |
| Trigger firing | Exactly once |
| Quote accuracy (shadow check) | < 0.1% mismatches |
| Concurrent trigger orders | 100k per chain |

## Budget: price move → trigger broadcast

| Step | Budget |
|---|---|
| Decode event, update pool state | < 1ms |
| Reprice, check sorted trigger index | < 1ms |
| Firing → execution (in-cluster gRPC) | ~1ms |
| Fresh route and quote (in memory) | < 5ms |
| Build transaction (intent pre-signed, D42) | < 1ms |
| Executor signs locally | < 1ms |
| Submit over persistent WebSockets, first wins | ~5–10ms |
| **Total** | **~15–20ms** (Base adds < 5ms local simulation) |

## Budget: click → broadcast

| Step | Budget |
|---|---|
| API gateway + auth | ~5ms |
| Route and quote | < 5ms |
| User intent signature (Privy, nearest region) ∥ simulation | ~30–50ms |
| Executor signs locally | < 1ms |
| Submit | ~5–10ms |
| **Total** | **~50–70ms** |

## Budget: price tick → client

Leading-edge throttle at 20/s with delta encoding (D43): ≤ 50ms worst-case hold + delivery.

## Measurement

Per-step timestamps on every trade as a trace; Prometheus + Grafana chart p99 per step, per chain. Any step over budget is visible on its own.

## Still to measure

Privy signing latency per region; BNB builder inclusion latency; provider delivery delay per chain (end-to-end view).
