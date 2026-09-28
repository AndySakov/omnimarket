# Primer: Blockchain Indexing

Background reading before the indexer decisions. Not part of the spec.

## 1. What an indexer is and why you need one

A blockchain is an append-only log optimised for *writing* and *verifying*, not for *asking questions*. A node can tell you:

- what's in block N
- the current value of a contract's storage (e.g. a pool's reserves)
- the events (logs) matching a filter within a block range

It **can't** tell you "every swap of token X in the last hour", "this token's price", or "this wallet's PnL". Those answers are derived by reading the chain in order and building your own query-friendly view.

**That's all an indexer is:** read the chain in order → decode the parts you care about → build a read model. It's CQRS where the chain is the write side and you own the read side.

## 2. What you actually read: events

Contracts emit **events** (logs) when something happens. For a DEX terminal the important ones are:

| Event | Emitted by | Tells you |
|---|---|---|
| `PairCreated` / `PoolCreated` | DEX factory | A new pool exists (token pair, fee tier, pool address) |
| `Swap` | Pool | Someone traded: amounts in/out; on v3 also the new price and tick |
| `Sync` (v2) | Pool | The pool's new reserves after any change |
| `Mint` / `Burn` | Pool | Liquidity added or removed |
| `Transfer` | Token | Tokens moved (holders, copy-trade detection) |

Each log has: the emitting **address**, **topics** (topic 0 is the hash of the event signature, e.g. `Swap(address,uint256,...)`), and **data**. You decode it with the contract's ABI.

**Key trick:** you can filter logs by topic 0 alone, across *all* addresses. "Give me every Uniswap-v2-style `Swap` on Base" catches every pool, including ones created a second ago. New-pool discovery is just listening to factory events.

Every log has a total order: **(block number, transaction index, log index)**. Processing in that order is non-negotiable; pool state is a fold over that sequence.

## 3. The core loop

The indexer keeps a **cursor**: the last block it processed, plus that block's hash. Then it repeats:

1. Fetch the next block's header and logs.
2. Check the new block's parent hash equals the cursor's hash. If not, the chain reorganised (see §5).
3. Decode the relevant logs, apply them to state, emit outputs.
4. Save the outputs and the new cursor **together, atomically**, so a crash can never leave them out of sync.

Two modes run the same logic with different goals:

| | Backfill | Head-following |
|---|---|---|
| What | Historical blocks | New blocks as they're produced |
| Optimise for | Throughput (big ranges, parallel fetches) | Latency (react within ms) |
| Reorgs | None (history is final) | Happen at the tip |

The handoff from backfill to live ("catch up, then switch without missing or duplicating a block") is a classic bug source.

## 4. How you get the data

| Source | How | Notes |
|---|---|---|
| `eth_getLogs` polling | Ask for logs in block range [a, b] | Simple, reliable, batchable. Latency = poll interval. Providers cap range and result size. |
| `eth_subscribe` (WebSocket) | Push of new heads / logs | Low latency. **Subscriptions drop silently**, so you must detect gaps and backfill them. |
| Chain-specific fast streams | See §6 | Where the real latency wins are. |
| Managed streams | QuickNode Streams, Alchemy webhooks, Goldsky, Substreams | Less to build and operate; less control; cost scales with volume. |
| Frameworks | Ponder, Envio, Subsquid, The Graph | Great for "events → database". Weak fit for "events → in-memory state at ms latency", which is what we need. |

The standard production pattern is **push for speed, pull for truth**: subscribe for low latency, and continuously reconcile with polling to catch anything the stream missed.

## 5. Reorgs and finality

At the tip, the chain can change its mind: a block you processed gets replaced by a different one. Anything you derived from it is now wrong.

- **Detect:** the new block's `parentHash` doesn't match the hash you stored for the previous height.
- **Handle:** walk back to the common ancestor, **undo** derived state for the orphaned blocks, then re-apply the new ones, and emit corrections downstream ("swap X was reverted").
- **Implication:** your state must be undoable for the last N blocks. Either keep a per-block change log, or snapshot state per block and roll back to a snapshot.

**Finality** is the depth past which a reorg can't happen. Anything above it is provisional. Every system has to choose: **act on the tip** (fast, occasionally wrong, needs corrections) or **wait for finality** (safe, slow). A trading terminal *must* act on the tip. Prices a second stale are useless. So corrections are a first-class feature, not an edge case.

## 6. Our three chains are all "sub-second" now, differently

| Chain | Blocks | Faster-than-block stream | Finality |
|---|---|---|---|
| **MegaETH** | EVM block ~1s | **Mini-blocks every ~10ms**, via the Realtime API's `miniBlocks` subscription (transactions, receipts, state changes) | Sequencer-ordered L2; L1-derived finality is much later **(verify details)** |
| **Base** | ~2s | **Flashblocks**: ~200ms preconfirmations **(verify)** | Sequencer-ordered L2; "safe" once posted to L1, "finalized" when that L1 block finalises |
| **BNB Chain** | ~0.45s (Fermi fork, Jan 2026) | None needed; blocks are already fast | Fast finality ~1.1s |

So "follow the head" means something different per chain. On MegaETH it's ~100 updates/second from one stream. Keeping up is a real throughput problem, not a trivial loop.

## 7. The failure modes that bite

- **Missed data:** dropped WebSocket, silent gaps. Needs gap detection by block number plus reconciliation.
- **Duplicates:** at-least-once delivery everywhere. Processing must be idempotent, keyed on (block hash, log index).
- **Inconsistent RPC nodes:** load-balanced providers route consecutive calls to different nodes. You see block N's header from one node, then get *empty* logs for block N from a node that hasn't seen it yet. Classic silent data loss. Mitigate by pinning to one node, checking the returned block hash, and cross-checking log counts.
- **Provider limits:** rate limits, `getLogs` range and size caps, compute-unit billing.
- **Spam:** thousands of junk tokens and pools. Filter by liquidity or activity, or memory and compute get wasted on noise.

## 8. The insight that shapes our design

For a terminal, the thing that matters most is **current pool state** (reserves, price, liquidity), and you **don't need history to know it**. You can read current state directly from the contracts at a given block (`getReserves()`, `slot0()`, liquidity, ticks), then apply events from that block onward.

That's **snapshot + replay** — the same pattern as the engine's crash recovery (D6).

So our "indexer" is really two separate jobs:

1. **Live state** (hot path): bootstrap pools from on-chain reads, then follow the head and apply events in memory. Needs no history. Lives in the Chain Engine.
2. **History** (cold path): backfill past swaps into ClickHouse for charts, volume, and PnL. Throughput-bound, can lag, can be restarted, can even use a framework or managed stream.

Separating these means backfill depth is a product decision (how much chart history do we want?), not a correctness requirement.

## Sources

- [MegaETH Realtime API](https://docs.megaeth.com/realtime-api)
- [MegaETH mini-blocks](https://docs.megaeth.com/miniblocks)
- [BNB Chain Fermi hard fork: 0.45s blocks](https://www.bnbchain.org/en/blog/fermi-hard-fork-accelerates-bsc-to-0-45-second-block-times)
- [BNB Chain Maxwell hard fork](https://www.bnbchain.org/en/blog/bnb-chain-announces-maxwell-hardfork-bsc-moves-to-0-75-second-block-times)
