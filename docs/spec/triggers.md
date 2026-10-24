# Triggers

**Status:** Draft. Decisions: D20, D22, D35–D39.

The order engine lives inside each Chain Engine (D6): orders are durable in Postgres, and the engine holds an in-memory index rebuilt from it (D35).

## Built for the demo (#86)

A thin M6 in shadow mode: `crates/triggers`, run by the API rather than the engine for now, since the API holds the accounts, quotes and trades a firing needs.

- **Orders.** A limit buy fires at or below its level, a take-profit at or above, and a stop-loss at or below. The level is a USD display price, or a percent from the position's average entry, resolved when it's set. A take-profit or stop-loss sells a share of the position into USDC; a limit buy spends a WETH or USDC amount. At most 50 open orders per account. REST places, edits and cancels them (frontend.md).
- **Evaluation.** A sorted index per token holds the levels that fire above and those that fire below. Each `prices.base` record checks the token's index, and the engine publishes those per canonical block only (D77), so nothing fires on provisional state. Pool and trade records never fire an order. The orders crossed fire in level order, the one nearest the price first.
- **Exactly once (D35).** A firing's ID is keccak-256 of the order ID and its per-order firing count. The order goes to `firing` with its count raised, and is kept in Postgres before its trade runs. A restart rebuilds the index from the store. An order a restart caught mid-firing fails rather than risk a second trade, and its user sees why.
- **A firing's trade.** It's quoted (#83), then run in shadow (#84) signed by the server's local signer (D57's user-absent path). The fill goes to the account (#85), and the order ends `filled` or `failed` with the reason. The trade carries the firing ID.
- **Exit guarantee (D60).** On by default for stops. If the trade fails, it's quoted again at the current price, within twice its slippage (at most 50%), and run once more. Off makes it a stop-limit.
- **Not yet:** auto exits attached to a buy, statuses on the `account` topic (REST serves them), and the rest of D37's catalogue.

## Order catalogue (D37)

| Type | Fires when | Index |
|---|---|---|
| Limit buy / sell | Price, market cap, or % change crosses the level | Sorted levels per token, quote-asset units (D22) |
| Stop-loss / take-profit | % from entry crossed | Same |
| Multi-level TP | Each level crossed; sells its share | Same, one entry per level |
| Trailing stop | Price falls X% below the running high | Per-order running high, updated on price moves |
| Auto-sell on buy | Arms TP/SL on each buy's fill | — |
| Dev sell | Token's dev wallet sells | (token, event) |
| Migration | Bonding curve graduates (D36) | (token, event) |
| Scheduled | Time reached | Timer wheel |
| Expiry | Time reached → cancel | Timer wheel |

All firings go through the exactly-once path (D35) and fire on canonical blocks, never on provisional state (D20, D77). Each trigger swap reverts on chain if the price is back on the wrong side of the level (D78).

**Arming signs the intent (D42, D57):** creating or editing an order signs its intent in the user's own session (exact amount, minimum output, expiry, allowed submitters per D58). Auto-armed TP/SL after a buy lands, and copy trades, are signed server-side under policy caps. Firing needs no user signature.

**Exit guarantee (D60):** stop-loss and trailing orders default to getting out even through a gap-down (server re-sign at the current quote); users can switch to a stop-limit. Multi-level TP sizes and trailing protection use D59's maximum amount and submitter-tightened minimum.

**Brakes (D56):** a chain-level breaker or manual level 2+ pauses firing; orders stay armed. On release, orders crossed during the pause fire with a fresh quote and the user is notified.

## Supporting data

- **Cost basis** per position from its own fills.
- **Dev wallet** per token, recorded at discovery.
- **Supply** per token for market-cap triggers; tracked from mint/burn events when the token can change it.

## Venues added (D36)

Bonding curves, starting with four.meme on BNB. Migration promotes the new PancakeSwap pool immediately.

## Copy trading (D38)

- Detect leader swaps in the event stream we already ingest; copy lands one block later on Base, one mini-block later on MegaETH.
- Per-follow settings: fixed or proportional size, max per trade, filters, buy-only or mirror sells, auto TP/SL.
- Fan-out routed with own-flow awareness (D25). Never copy from the mempool.
- Pools traded by followed wallets are activated immediately (D11).

## Mechanics (D39)

- **Multi-level TP:** % of the original position, capped at holdings; empty position cancels siblings; stop-loss sells all that remains.
- **Trailing high-water mark:** saved write-behind on >0.5% rises; recovered as max(saved, ClickHouse 1s-candle max since the save).
- **Limits:** 200 active orders per user, 20 per token per user.
- **Cascades:** everything fires; priority queue (stop-loss/trailing → TP/limit → scheduled).

## Open questions

1. ~~Multi-level TP bookkeeping~~, ~~trailing-stop failover~~, ~~limits~~ → **decided (D39).**
2. **Kumbaya launchpad** on MegaETH: it has a "Memes" launch section; whether it uses a bonding curve and how graduation works **(verify)**.
