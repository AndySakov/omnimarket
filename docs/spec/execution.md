# Execution

**Status:** Draft. Decisions: D8, D30–D35. **Complete** (tuning and measurements aside).

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

## Priority fees (D33)

| Situation | Tip |
|---|---|
| New-pair buy | Aggressive |
| Stop-loss / trailing | High |
| Manual | Median of recently landed + margin |
| Take-profit / limit | Low |
| Watchdog re-send | Previous + 25% |

Levels follow live landed tips per chain; per-trade fee cap; user override. Gas is always paid by the user's wallet.

## Tracking & failures (D34)

- `signed → submitted → preconfirmed → confirmed → final` | `reverted | dropped | replaced`.
- Simulate in parallel with signing; a failing simulation cancels the send.
- Landing detected from our own indexer by transaction hash (MegaETH: receipt from the send call).
- Revert handling: stop-loss auto-retry ×3 · TP/limit re-arm · manual: tell the user · copy: one retry.

## Exactly-once firing (D35)

- Firing ID = hash(order ID, firing count): deterministic, so the standby matches.
- Execution dedupes by firing ID; engine retries until acknowledged.
- One firing per order at a time. Orders durable in Postgres; engine index is a cache.

## Open questions

Biggest first:

1. ~~Signing latency~~ → **decided (D31).** Still to measure: Privy latency per region.
2. ~~Nonces~~ → **decided (D32).**
3. ~~Gas and priority fees~~ → **decided (D33).**
4. ~~Tracking and failure handling~~ → **decided (D34).**
5. ~~Exactly-once trigger firing~~ → **decided (D35).**

Still to measure: Privy signing latency per region; builder inclusion latency on BNB.
