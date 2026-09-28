# SLAs & Latency Budgets

**Status:** Draft. Decisions: D42–D47.

Targets measure **internal latency**: from our engine receiving the event (or the API receiving the click) to the transaction being broadcast. End-to-end latency from the chain's timestamp is reported alongside, not targeted.

## Targets (p99)

| Metric | Target |
|---|---|
| Price move → trigger broadcast | ≤ 75ms (Base ~25ms; BNB/MegaETH ~35–50ms) |
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
| Simulation, blocking (Base: local node; BNB/MegaETH: co-located provider) | < 5ms / ~10–30ms |
| Executor signs locally | < 1ms |
| Submit over persistent WebSockets, first wins | ~5–10ms |
| **Total** | **~20–25ms Base; ~35–50ms BNB/MegaETH** |

## Budget: click → broadcast

| Step | Budget |
|---|---|
| API gateway + auth | ~5ms |
| Route and quote | < 5ms |
| User intent signature (user's Privy session in the browser, D57; server fallback under policy) ∥ simulation | ~30–50ms **(measure)** |
| Executor signs locally | < 1ms |
| Submit | ~5–10ms |
| **Total** | **~50–70ms** |

## Budget: price tick → client

Leading-edge throttle at 20/s with delta encoding (D43): ≤ 50ms worst-case hold + delivery.

## Measurement

Per-step timestamps on every trade as a trace; Prometheus + Grafana chart p99 per step, per chain. Any step over budget is visible on its own.

## Levers not yet taken

Own BNB/MegaETH nodes or in-process simulation would bring the remote-simulation chains down to Base's number.

## Still to measure

Privy signing latency per region; BNB builder inclusion latency; provider delivery delay per chain (end-to-end view).
