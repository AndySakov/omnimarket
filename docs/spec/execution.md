# Execution

**Status:** Draft. Decisions: D8, D30–D32.

One Execution service per chain (D8): build → simulate → sign → submit → track. Sole owner of every wallet's nonce on its chain. Called by the Chain Engine and API over gRPC; publishes outcomes to Kafka.

## Submission (D30)

| Chain | Path |
|---|---|
| BNB | Parallel fan-out to 3–4 private builder RPCs; never the public mempool |
| Base | Primary + fallback provider in parallel; priority fee by situation |
| MegaETH | `realtime_sendRawTransaction` (receipt in one call), primary + fallback |

Identical signed transactions share a nonce, so fan-out can't double-execute.

## Hot-path signing (D31)

- Sells spend a per-position Permit2 allowance, signed when the buy lands (capped at position size, 7 days, renewed; router checks `owner == msg.sender`).
- Armed triggers are fire-ready: calldata layout, gas estimate and allowance prepared. At fire time: fresh quote → next nonce from memory → one signature → submit.
- Nonces are never reserved per order (an unused reserved nonce blocks the wallet).

## Nonces (D32)

- Per-wallet sequencer with an in-memory counter (no RPC call per trade).
- Durable nonce ledger: `assigned → signed → submitted → landed | replaced | filled`, compare-and-set writes, terminal states final. Recovery source for restarts and the standby.
- Gap watchdog: not landed within a few blocks → same nonce, higher fee; stale → 0-value self-transfer filler.
- Re-sync from chain on startup, failover, or nonce errors. Cap 5–10 in flight per wallet.

## Open questions

Biggest first:

1. ~~Signing latency~~ → **decided (D31).** Still to measure: Privy latency per region.
2. ~~Nonces~~ → **decided (D32).**
3. **Gas and priority fees.** Per chain and per situation (reuse D25 order-origin cues).
4. **Tracking and failure handling.** Stuck, dropped, reverted; replace and cancel.
5. **Exactly-once trigger firing** across engine → execution (D8).
