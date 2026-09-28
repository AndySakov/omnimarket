# Execution

**Status:** Draft. Decisions: D8, D30, D32–D35, D42–D47, D57–D58. **Complete** (tuning and measurements aside).

One Execution service per chain (D8): route → build → simulate → sign → submit → track. Called by the Chain Engine and API over gRPC; publishes outcomes to Kafka.

**Execution model (D42):** users sign **intents** (Permit2 witness transfers: exact amount in, minimum out, deadline, output to themselves). Our **executor wallets** submit the transactions and pay gas, recovered from the trade. The execution service owns the executor wallets and their nonces.

## Submission (D30)

| Chain | Path |
|---|---|
| BNB | Parallel fan-out to 3–4 private builder RPCs; never the public mempool |
| Base | Primary + fallback provider in parallel; priority fee by situation |
| MegaETH | `realtime_sendRawTransaction` (receipt in one call), primary + fallback |

Identical signed transactions share a nonce, so fan-out can't double-execute.

## Intents and executors (D42)

| | Manual trade | Trigger order | Copy trade |
|---|---|---|---|
| User signature | 1 signature at click (user session) | 1 signature **when the order is created** (user session) | 1 signature at copy time (server, under policy caps) |
| On the hot path | Privy sign ∥ simulate → executor signs locally → submit | Fresh route → executor signs locally (< 1ms) → submit | Privy sign ∥ simulate → executor signs → submit |

- Executor pool per chain; keys in our own `Signer` (keystore / KMS). Firings spread across executors, so cascades don't queue behind one nonce sequence.
- Executor keys hold only gas money: they can execute only intents users signed, on the signed terms.
- Balances kept as WETH/WBNB (auto-wrap on deposit). Permit2 approvals sent from the user's wallet in the background: base assets at setup, each new token right after its buy lands.
- Armed triggers are fire-ready: intent signed, route candidates and gas estimates cached.
- **Who signs (D57):** the user's own Privy session when they're present (manual trades, creating/editing orders); server signing only for absent flows (copy trades, auto-armed TP/SL, background approve/wrap), under Privy policy: router EIP-712 domain only, per-intent and per-user daily caps, minimum-output floor.
- **Submitter field (D58):** each intent lists who may submit it: our executor set or the user.
- Carrier reviewed against Privy's smart wallets and EIP-7702 (D47): Permit2 in phase 1; a 7702 delegate is a phase 2 candidate; no 4337 smart wallets on the trade path.

## Nonces (D32)

- Applies to **executor wallets** only; user intents use Permit2's unordered nonces (no gaps possible).
- Per-executor sequencer with an in-memory counter (no RPC call per trade).
- Durable nonce ledger: `assigned → signed → submitted → landed | replaced | filled`, compare-and-set writes, terminal states final. Recovery source for restarts and the standby.
- Gap watchdog: not landed within a few blocks → same nonce, higher fee; stale → 0-value self-transfer filler.
- Re-sync from chain on startup, failover, or nonce errors. Cap 5–10 in flight per executor.

## Priority fees (D33)

| Situation | Tip |
|---|---|
| New-pair buy | Aggressive |
| Stop-loss / trailing | High |
| Manual | Median of recently landed + margin |
| Take-profit / limit | Low |
| Watchdog re-send | Previous + 25% |

Levels follow live landed tips per chain; per-trade fee cap; user override. Gas is paid by the executor and recovered from the trade (D42).

## Latency levers (D43, D44)

- Engine + execution per chain in the region nearest that chain's sequencer/builders; execution close to Privy's nearest region.
- Submission over persistent WebSockets to all endpoints at once (first wins); Base also direct to the sequencer **(verify)**.
- Production Base node next to the Base engine: direct Flashblocks feed, local simulation and state reads (< 5ms).

## Tracking & failures (D34)

- `signed → submitted → preconfirmed → confirmed → final` | `reverted | dropped | replaced`.
- Simulation always blocks the send. Manual trades run it in parallel with the Privy signature; triggers run it before the executor signs (local on Base, co-located provider elsewhere). A failing simulation cancels the send.
- Landing detected from our own indexer by transaction hash (MegaETH: receipt from the send call).
- Revert handling: stop-loss auto-retry ×3 · TP/limit re-arm · manual: tell the user · copy: one retry.

## Exactly-once firing (D35)

- Firing ID = hash(order ID, firing count): deterministic, so the standby matches.
- Execution dedupes by firing ID; engine retries until acknowledged.
- One firing per order at a time. Orders durable in Postgres; engine index is a cache.

## Open questions

Biggest first:

1. ~~Signing latency~~ → **decided (D42):** intents; Privy off the trigger hot path.
2. ~~Nonces~~ → **decided (D32).**
3. ~~Gas and priority fees~~ → **decided (D33).**
4. ~~Tracking and failure handling~~ → **decided (D34).**
5. ~~Exactly-once trigger firing~~ → **decided (D35).**

Still to measure: Privy signing latency per region; builder inclusion latency on BNB.
