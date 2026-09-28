# Case Study: How iTRY (Brix) Ingests Chain Data

Background for D9. Sources: the iTRY monorepo (`packages/serverless`, `packages/docs`) and [docs.brix.money](https://docs.brix.money).

## What iTRY is

A tokenised fund: institutions pay USDC and receive DLF/iTRY tokens backed by Turkish money-market funds. Ethereum is the hub, MegaETH a spoke (LayerZero OFT). Mints are instant; redemptions settle in daily batches.

## How it gets chain data: it doesn't run an indexer

| Mechanism | What it does |
|---|---|
| **Alchemy Custom Webhooks** | A GraphQL filter per webhook: 2 known contract addresses × 4 event topics (`ITRYIssued`, `ITRYRedeemed`, `ItryMinted`, `YieldDistributed`). Alchemy pushes matching logs per block to a Lambda Function URL. |
| **Webhook Lambda** | Verifies HMAC signature, rate-limits, decodes, groups logs by transaction. **Always returns 200**; failures go to SQS → retry Lambda → DLQ after 3 attempts. |
| **Confirmation gate** | Records the event as `SUBMITTED` immediately, then re-queues itself (SQS delay ≈ remaining blocks × 12s) until **3 confirmations**. Then checks the transaction is **still canonical**. If it was reorged out, marks it `FAILED`. |
| **Idempotency** | Order IDs are hashed into the event topic, so the webhook maps an event straight to its order. Terminal states are never overwritten. Status writes are compare-and-set. |
| **Reconcilers instead of gap-free ingestion** | `stuck-order-detection` (orders stuck in `SUBMITTED` > 1h → alert); `mempool-evicted-reconciler` (a submitted tx vanished → `FAILED_SUBMISSION`); nonce-ledger rescue with same-nonce filler txs. |
| **State reads instead of event history** | Partner balances for yield come from `balanceOf()` snapshots once per hour at a pseudo-random minute, not from indexing `Transfer` events. |

## Why those choices were right for iTRY

1. **It watches its own contracts, not the world.** Two known addresses, four event types. No discovery problem, so a filtered managed webhook covers 100% of what matters.
2. **Tiny volume.** Institutional orders, a handful per day. An always-on indexer would idle 99.9% of the time; Lambda + webhooks cost almost nothing and need no operations.
3. **The backend is the writer.** iTRY submits the transactions itself, so events are *evidence confirming its own actions* (the internal spec is literally "TX Evidence Ingestion"), not the primary data. The source of truth is its own order record; the chain confirms it.
4. **Money correctness beats latency.** Waiting 3 blocks (~36s) plus a canonicality check is fine when settlement is daily. Nothing downstream acts on unconfirmed data.
5. **Missing an event is recoverable.** A missed webhook leaves an order stuck in `SUBMITTED` → an alert fires → a human reconciles. Slow but safe, and rare.
6. **Snapshots can't drift.** Yield uses time-weighted average balances. Reading `balanceOf()` directly is self-correcting: a missed `Transfer` can't corrupt it. The random minute stops partners gaming snapshot times.

## Why OmniMarket can't copy it

| | iTRY | OmniMarket |
|---|---|---|
| Contracts watched | 2 of our own, known in advance | Thousands of third-party pools, most created after we start |
| Event volume | A few per day | Thousands per second (MegaETH mini-blocks) |
| Latency need | Minutes | Milliseconds |
| Finality posture | Wait 3 confirmations, then act | Act on the tip, correct on reorg |
| Role of events | Evidence of our own transactions | The primary data (prices, liquidity) |
| Cost of a missed event | Alert, manual fix | Wrong price or trigger → user loses money, silently |
| Runtime | Lambda per delivery | Always-on stateful process |
| Webhook delivery | Seconds of added latency, fine | Would miss the ~10ms mini-block stream entirely |

## What does carry over, and where

- **Execution tracking (D8):** iTRY's transaction lifecycle is exactly what OmniMarket's Execution service needs for its own transactions: submitted → confirmed / failed / evicted, compare-and-set status writes, the eviction reconciler, and the nonce ledger with same-nonce filler rescue. This is first-hand, production-proven experience.
- **Snapshot over history (D9 live state):** "read current state from the contract, don't reconstruct it from events" is the same idea as bootstrapping pool state with `getReserves()` / `slot0()`.
- **Confirmation depth as a status, not a gate:** OmniMarket acts on the tip, but positions and PnL can still carry "provisional → confirmed" status, the way iTRY does with `SUBMITTED` → `COMPLETED`.
- **Buy, don't build, when stakes and volume are low:** a candidate policy for OmniMarket's cold-path history job.
