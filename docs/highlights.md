# Design Highlights

The design choices worth talking about when describing OmniMarket: what we did, why it's non-obvious, and a one-line way to say it. Each links back to its decision in [spec/decisions.md](spec/decisions.md).

---

## Architecture

### Kafka is never between a price change and an order (D6)
Every chain has one in-memory engine that holds live pool state and does pricing, quoting and trigger checks in-process. Kafka receives everything, but only for the cold path (history, analytics, UI feeds).
- **Why it's non-obvious:** event-driven designs usually put the broker in the middle of everything. Here that would add broker hops and consumer lag exactly where latency matters.
- **Say it as:** "Kafka is the system of record and fan-out, not the hot path."

### Execution is a separate service, with one owner per nonce (D8)
Slow, failure-prone I/O (signing, RPC submission) lives outside the pricing loop, one sub-millisecond gRPC hop away. Each trigger firing carries a unique ID, so retries can't double-execute.
- **Say it as:** "The engine decides, execution acts, and a firing ID makes it exactly-once."

### Phase 1 is built so phase 2 is half done (D7)
The terminal (taker side) is designed so a proprietary AMM (maker side) can reuse its head follower, pool state, fair price and nonce management. The router treats venues as quote sources without assuming how quotes are computed, so our own AMM slots in as just another venue.

---

### Recovery replays values, not history (D40)
The engine snapshots its pool state every ~30s. On restart it loads the snapshot and replays the pool updates published to Kafka since then. Each update already carries its after-state (D12), so recovery is applying values: no RPC calls, no recomputation, back in under 10 seconds.

### A standby that can't double-fire (D40)
The standby stays warm by applying the primary's Kafka stream (no extra RPC cost, identical state). Only the holder of a short lease may fire, every firing carries the lease's epoch, and execution rejects stale epochs, so a primary that freezes and wakes up can't act. Deterministic firing IDs (D35) cover the moment of the switch.
- **Say it as:** "Leases decide who fires, fencing stops a zombie from firing, and firing IDs make the overlap harmless."

## Indexing

### A fast stream plus a reconciler, and one correction mechanism for both (D10)
Each chain follows its fastest feed (MegaETH 10ms mini-blocks, Base 200ms Flashblocks) for provisional state. A reconciler then confirms against canonical blocks.
- **The clever part:** a dropped preconfirmation is treated as a reorg of depth zero, so one undo path handles both.
- **Say it as:** "Act on the preconfirmation, reconcile on the block, and treat every mismatch as a reorg."

### Pool tracking works like a cache (D11)
Every pool is *known* (metadata only), but only *active* pools (liquid, new, or referenced by a user) get full in-memory state. Pools are promoted on activity and demoted when they go quiet.
- **Say it as:** "Memory follows real activity, not spam, without missing a new pair."

### The event log doubles as the undo log (D12)
Undo is tiered:
- **Memory:** the last ~10 seconds.
- **Kafka:** back to finality, because every published pool update carries its before and after state.
- **Rebuild:** a fresh bootstrap if all else fails.

No new storage system was needed for deep reorgs.

### Stream per block, not per event (D16)
RPC providers bill every WebSocket push. Subscribing to one message per block (Base Flashblocks payloads, BNB headers plus one `getLogs`) instead of one per swap turns a cost that grows with trading volume into a fixed one. That's an estimated 5–10× cheaper.
- **Say it as:** "We shaped the data feed around the provider's billing model."

### Free tiers are chaos engineering for free (D17)
Dev and staging run on free RPC tiers. Their rate limits and dropped connections exercise the failover and gap-fill paths constantly, long before production does.

---

## Pricing

### Three prices, not one (D18)
- **Display:** a liquidity-weighted mid across pools.
- **Trigger:** what orders fire on.
- **Execution:** the quote at the actual trade size.

It borrows perpetual exchanges' lesson (display last price, liquidate on mark price) that a single price can't serve every purpose.
- **Say it as:** "What you see, what fires your stop, and what you get are three different questions."

### Stablecoins are pinned, until they aren't (D19)
Stablecoins count as $1 while the chain's reference stablecoins agree within ~0.5%. If they diverge, the engine prices each one from its pools and warns users. It's cheap in normal times and honest during a depeg like USDC in March 2023.

### Exact in-memory math, checked against the chain (D21)
Quotes reproduce each DEX contract's math down to integer rounding, in microseconds. A background job samples real quotes, simulates them on-chain, and demotes any pool type that drifts to simulated quoting until it's fixed.
- **Say it as:** "Fast by default, verified continuously, self-demoting when wrong."

### Trigger levels live in the pool's own currency (D22)
A stop at "$0.010" on an ETH-paired token is stored as a level in ETH. When ETH/USD moves, the engine converts the boundary and checks only the orders it crossed, instead of repricing thousands of tokens and scanning every order.

### One liquidity measure for everything (D24)
±2% depth (dollars tradable before the price moves 2%) weights pools, weights cross-chain prices, and sets the liquidity floor. It works the same across v2, v3, v4 and Aerodrome, so pools of different types compare fairly.

---

## Routing

### Route shape is chosen per order from situational cues (D25)
The router prices single-pool, multi-hop and split routes in memory and picks by *output − gas − risk penalty*. The penalty adapts to the situation:
- **Order origin:** a stop-loss in a crash wants reliability; a limit buy wants price.
- **Size vs depth:** small trades skip the split search entirely.
- **Liquidity concentration:** one dominant pool means splitting can't help.
- **Venue trust:** simulated, brand-new or fee-on-transfer venues are penalised, and excluded for urgent orders.
- **Later:** pool heat, gas price, chain MEV profile, state confidence, revert history.

- **Say it as:** "Small trades get Trojan's speed, big patient trades get aggregator prices, and the router decides which situation it's in."

### Own-flow awareness (D25)
When a copied whale buys and 500 copy trades follow within milliseconds, a normal router quotes all 500 against the same pool state and 499 quotes are wrong. Our router applies our own in-flight orders to its in-memory state before quoting the next one, so later orders see real prices and spread across pools.
- **Why it's non-obvious:** an outside aggregator can't do this. Only we see our own order flow.
- **Say it as:** "The router knows about the trades it hasn't landed yet."

### A router that can't be upgraded, can't hold funds, and only moves what users signed for (D26, D42)
Our router is immutable (no admin key to steal) and must end every call with a zero balance. It moves user funds only against a signed intent: exact amount, minimum output, deadline, output to the signer. Because users approve Permit2 rather than the router, a new router version needs no re-approvals.
- **Say it as:** "The contract that touches user funds has no owner, no balance, and no permission beyond the trade you signed."

### Slippage is chosen by situation, not one flat number (D27)
Slippage is both a fill guarantee and the amount a sandwich bot can take. Defaults follow what the router already knows: 15% for new pairs, 3% for established tokens, 0.5% for majors, doubled for stop-losses that must land. The displayed quote is never executed as-is: the route is re-quoted at send time.
- **Say it as:** "Your slippage depends on what you're trading and why, not on a global setting."

### Honeypot checks without deploying anything (D29)
To test whether a token can be sold, the engine runs one read-only `eth_call` in which a simulator contract is *injected* through a state override at a throwaway address, given native coin, and made to buy then sell against the live pool. A sell that reverts means a honeypot; value missing from the round trip is the tax. No deployment, no gas, no on-chain footprint.
- **Say it as:** "We test-sell every token in a simulation before anyone can buy it."

### The indexer is also a safety monitor (D29)
The swap and transfer stream we already index shows when sells stop succeeding while buys continue, when the realised tax drifts from the simulated one, or when liquidity is pulled. Those behavioural signals trigger an immediate re-check at no extra RPC cost.

### Heuristics are measured, not guessed (D25, D5)
Every routing decision and its outcome (quoted vs filled, reverts) is logged. Shadow mode replays the same order flow under different penalty settings to compare.

---

## Execution

### Private fan-out: privacy without slow inclusion (D30)
On BNB, the same signed transaction goes to several private block builders in parallel. It never touches the public mempool (so it can't be sandwiched), yet it reaches most of the block-building market. Because every copy shares one nonce, it can only land once.
- **Say it as:** "Send one transaction to every private door at once; only one can open."

### Intents: the user signs once, we do the rest (D42)
Users never send transactions. Each trade is a signed intent ("sell exactly N for at least X before T, output to me"), and our executor wallets submit it and pay the gas, recovered from the trade. Trigger intents are signed when the order is created, so when a stop fires there is no signing round trip to the wallet vendor: the executor signs locally in under a millisecond. Executor keys hold only gas money; they can only carry out intents users already signed, on the signed terms. Same model as UniswapX, CoW Swap and 1inch Fusion, applied to a trading terminal.
- **Say it as:** "When your stop fires, nothing needs your signature: you signed it when you set it."

### From ~130ms to ~15–30ms by questioning every "fixed" cost (D42–D44)
We re-examined each large latency item instead of accepting it: intents removed the wallet-vendor signature from triggers, persistent WebSocket submission and per-chain co-location cut network hops, and a local Base node turns simulation into a < 5ms local call.

### A nonce ledger that survives crashes and unblocks itself (D32)
Only our executor wallets have sequential nonces (user intents use unordered ones). Nonces come from an in-memory counter (no RPC call per trade), but every assigned nonce is also recorded in a durable ledger with compare-and-set status writes. If a transaction is dropped, a watchdog re-sends it with a higher fee or burns the nonce with a 0-value self-transfer so the wallet isn't stuck.
- **Say it as:** "Fast like a counter, recoverable like a ledger, and a dropped transaction never freezes a wallet."

### Simulate while signing (D34)
On manual trades, the pre-send simulation doesn't need the user's signature, so it runs in parallel with the Privy signing call. The hot path pays for the slower of the two, not both.

### Our indexer doubles as our receipt service (D34)
Our own swaps show up in the pool events we already stream, so matching by transaction hash tells us a trade landed within 10–200ms, without polling for receipts.

### Exactly-once by making retries safe (D35)
Every trigger firing has a deterministic ID (order ID + firing count), so the standby engine computes the same ID after failover. Execution remembers IDs it has handled, so the engine can retry freely and a duplicate is always recognised.
- **Say it as:** "Retries are always safe, duplicates are always recognised, so a stop-loss fires exactly once."

## Triggers

### Copy trading rides the indexer (D38)
Leader swaps are spotted in the pool events we already stream, so on Base and MegaETH a copy can land one flashblock or mini-block after the leader, with no extra RPC calls. Followed wallets pre-activate the pools they trade, and fan-out to many followers is routed with own-flow awareness. We never copy from the mempool.
- **Say it as:** "Copy trades one block behind the leader, never in front of them."

### Launchpads are first-class venues (D36)
On BNB most new memecoins start on a four.meme bonding curve, not a DEX pool. We price and trade the curve directly, and the moment it graduates, the new PancakeSwap pool is already active, so there's no gap in pricing and the "buy on migration" trigger has something to fire on.

## Data

### Reorgs are new versions, not deletes (D41)
ClickHouse rows are keyed by (chain, block hash, log index) and carry a status and a version. A correction inserts a newer version (including "removed" for reorged-out events) and the table keeps the latest. Backfills can restart and overlap the live feed without duplicates, and nothing is ever mutated in place.

## Testing & operations

### Shadow execution: load tests at mainnet realism with zero spend (D5)
The full pipeline (quote → build → simulate against live state → sign) runs against real mainnet data and stops just before broadcast. Load tests (flash crowds, stop-loss cascades, copy-trade fan-out) hit real conditions without spending money.

### A signing boundary keeps the wallet vendor swappable (D4)
All signing goes through an internal `Signer` interface: Privy in production, a local encrypted keystore for load tests and forks. This contains vendor lock-in, and load tests don't depend on a third party's rate limits.
