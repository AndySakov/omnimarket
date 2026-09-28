# Execution

**Status:** Draft. Decisions: D8, D30.

One Execution service per chain (D8): build → simulate → sign → submit → track. Sole owner of every wallet's nonce on its chain. Called by the Chain Engine and API over gRPC; publishes outcomes to Kafka.

## Submission (D30)

| Chain | Path |
|---|---|
| BNB | Parallel fan-out to 3–4 private builder RPCs; never the public mempool |
| Base | Primary + fallback provider in parallel; priority fee by situation |
| MegaETH | `realtime_sendRawTransaction` (receipt in one call), primary + fallback |

Identical signed transactions share a nonce, so fan-out can't double-execute.

## Open questions

Biggest first:

1. **Signing latency.** Permit2 per-trade permits (D26) mean two sequential Privy signatures on sells and token-funded buys (Privy quotes ~20–100ms each).
2. **Nonces.** Several in-flight transactions per wallet (trigger bursts, copy-trade fan-out); gaps and reordering.
3. **Gas and priority fees.** Per chain and per situation (reuse D25 order-origin cues).
4. **Tracking and failure handling.** Stuck, dropped, reverted; replace and cancel.
5. **Exactly-once trigger firing** across engine → execution (D8).
