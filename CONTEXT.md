# OmniMarket Context

The shared vocabulary for OmniMarket. Terms here are the ones to use in code, docs and conversation. Each points to the decision that defines it in [docs/spec/decisions.md](docs/spec/decisions.md).

## Language

### Data and state

**Chain Engine**:
One stateful, in-memory process per chain that follows the chain head, holds live pool state, and runs pricing, routing and trigger evaluation in-process (D6).
_Avoid_: indexer (that's one of its jobs), node

**Hot path**:
Everything between a chain event and a trade: engine memory and direct calls to execution, with no broker hop (D6).
_Avoid_: fast path

**Cold path**:
Consumers fed from Kafka: ClickHouse history, Postgres, UI feeds, candles (D6, D41).

**Fast loop**:
The engine loop that applies events from the chain's fastest stream immediately: canonical blocks on Base and BNB (D77), mini-blocks on MegaETH, which are provisional (D10).

**Reconciler**:
The engine loop that reads canonical blocks, confirms or corrects fast-loop state, detects reorgs and fills gaps (D10).

**Provisional state**:
State applied from the fast loop before canonical confirmation. Shown in the display price; triggers never fire on it (D10, D77).

**Tiered undo**:
Automatic rollback all the way to L1 finality, stored in three tiers: hot (engine memory), then warmer and colder stores (D12).

**Known pool** / **Active pool**:
A known pool is recorded from its creation event, metadata only. An active pool has full in-memory state and is priced and routed. Pools qualify as active by base-asset pairing and a liquidity floor, or by being new (D11).

**Snapshot**:
A periodic dump of all active pool state, stamped with its block. Recovery loads a snapshot, then replays Kafka updates since that block (D40).

### Prices and routing

**Display price**:
The liquidity-weighted mid across a token's active pools. It drives the UI, PnL marks and triggers (D18, D20).
_Avoid_: "the price" when the context allows more than one

**Fair price**:
The display price, or for a curated cross-chain asset list, a depth-weighted average of per-chain display prices (D23).

**Depth**:
Liquidity measured as ±2% depth in USD (D24).

**Cue**:
A signal inferred from the order, market, pools or our own flow that sets the router's risk penalty per extra pool (D25).

**Shadow check**:
A background sample of live in-memory quotes, re-simulated on-chain, that alerts on any mismatch (D21).

### Orders and execution

**Trigger order**:
A limit, take-profit, stop-loss, trailing stop or other catalogue order that fires when the display price hits its level (D20, D37, D39).

**Firing** / **Firing ID**:
One activation of a trigger order. Its ID is hash(order ID, per-order firing count), so a standby engine computes the same ID and execution dedupes repeats (D35).

**Intent**:
A user-signed Permit2 message authorising a trade: input token and maximum amount, output token and minimum rate, and allowed submitters. Our executor wallets submit it (D42, D47, D59).
_Avoid_: order (an order can produce many intents), transaction

**Submitter**:
An address allowed to submit an intent: our executor set, and always the user (D58).

**Executor wallet**:
A wallet we control that submits intents on-chain and pays gas (D42).

**Exit guarantee**:
The stop-loss default: if a gap-down breaks the signed minimum, the server re-signs at the current quote within policy caps so the stop still fills. Turning it off gives a stop-limit (D60).

**Nonce ledger**:
The durable per-wallet record of nonces, with a sequencer and a gap watchdog (D32).

### Modes and safety

**Shadow mode** / **Shadow execution**:
The full pipeline (quote, build, simulate against live state, sign locally) that stops before broadcast. It's the default outside real-funds demos (D5).

**Real-funds demo**:
A recorded end-to-end run with a small funded wallet per chain (D5).

**det runtime**:
The `det` crate's traits (Clock, Rng, EventSource, Rpc, Signer, Broadcaster, Store). Each has a real, a simulated, a recording and a replay implementation, so core logic is deterministic and replays exactly (D49, D54, D74).

**Flight recorder**:
The log of every input to the engine and execution cores, so any moment can be replayed exactly (D54).

**Lineage**:
The chain of IDs every record carries back to its cause: chain event, pool update, price update, trigger check, firing ID, intent hash, route decision, simulation, executor tx, landing, position update, notification (D53).

**Wide event** / **Decision record**:
One typed Protobuf event per unit of work. Decision records also capture the alternatives considered and why one was chosen (D53).

**Independent watcher**:
A separate service with its own code and RPC that reconciles every router and executor transaction against our records. Any mismatch pauses all executors (D55).

**Brakes**:
Automatic breakers plus four manual levels that stop money paths at the smallest effective scope (D56).

## Relationships

- A **Trigger order** produces one or more **Firings**. Each **Firing** produces at most one trade, deduped by **Firing ID**.
- A trade is carried by an **Intent**, submitted by a **Submitter** (normally an **Executor wallet**).
- The **Chain Engine** runs a **Fast loop** and a **Reconciler**, and publishes every state change to Kafka for the **Cold path**.
- **Lineage** links every step from chain event to notification, and the **Flight recorder** makes each step replayable.

## Flagged ambiguities

- "Price" alone is ambiguous: say **display**, **fair** or execution (quote) price (D18).
- "Order" versus **Intent**: the order is the user's standing instruction; the intent is the signed authorisation for one trade.
- D31's per-position allowances are superseded by **Intents** (D42). Don't use "allowance" for the current model.
