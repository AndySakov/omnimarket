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

### Heuristics are measured, not guessed (D25, D5)
Every routing decision and its outcome (quoted vs filled, reverts) is logged. Shadow mode replays the same order flow under different penalty settings to compare.

---

## Testing & operations

### Shadow execution: load tests at mainnet realism with zero spend (D5)
The full pipeline (quote → build → simulate against live state → sign) runs against real mainnet data and stops just before broadcast. Load tests (flash crowds, stop-loss cascades, copy-trade fan-out) hit real conditions without spending money.

### A signing boundary keeps the wallet vendor swappable (D4)
All signing goes through an internal `Signer` interface: Privy in production, a local encrypted keystore for load tests and forks. This contains vendor lock-in, and load tests don't depend on a third party's rate limits.
