# Triggers

**Status:** Draft. Decisions: D20, D22, D35–D38.

The order engine lives inside each Chain Engine (D6): orders are durable in Postgres, and the engine holds an in-memory index rebuilt from it (D35).

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

All firings go through the exactly-once path (D35) and fire instantly on the display price (D20).

## Supporting data

- **Cost basis** per position from its own fills.
- **Dev wallet** per token, recorded at discovery.
- **Supply** per token for market-cap triggers; tracked from mint/burn events when the token can change it.

## Venues added (D36)

Bonding curves, starting with four.meme on BNB. Migration promotes the new PancakeSwap pool immediately.

## Copy trading (D38)

- Detect leader swaps in the event stream we already ingest; copy lands one flashblock / mini-block later on Base and MegaETH.
- Per-follow settings: fixed or proportional size, max per trade, filters, buy-only or mirror sells, auto TP/SL.
- Fan-out routed with own-flow awareness (D25). Never copy from the mempool.
- Pools traded by followed wallets are activated immediately (D11).

## Open questions

1. **Multi-level TP bookkeeping:** how sibling orders resize after partial sells and manual trades.
2. **Trailing-stop state on failover:** is the running high persisted, or rebuilt from price history?
3. **Limits:** max orders per user, per token; behaviour at the 100k concurrent-orders target (product.md).
4. **Kumbaya launchpad** mechanics on MegaETH **(verify)**.
