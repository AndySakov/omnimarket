# Routing

**Status:** Draft. Decisions: D25–D29.

## Route shapes (D25)

| Shape | Example | Used when |
|---|---|---|
| Single pool | ETH → PEPE via its main pool | Default; small trades; urgent orders |
| Multi-hop (≤2) | USDC → WETH → PEPE | The pair has no direct pool; intermediates only from the quote-asset set (D19) |
| Split (≤3 pools) | 60% pool A, 40% pool B | The price gain beats extra gas + risk |

## Choosing a route

`score = output − extra gas − Σ risk penalty(extra pool)`

1. **Fast path:** trade < ~1% of best pool's ±2% depth, or one pool holds >90% of depth → single pool.
2. **Otherwise** quote single, 2-hop, and split candidates in memory (splits: greedy 5% chunks).
3. **Apply our in-flight orders** to the pool state before quoting.
4. Pick the highest score.

### Risk penalty cues

| # | Cue | Effect | Phase |
|---|---|---|---|
| 1 | Size vs depth | Small → skip split search | 1 |
| 2 | Order origin | Stop-loss: reliability · limit/TP: price · new pair: speed · copy: leader's pool | 1 |
| 3 | User slippage setting | High slippage = urgency → higher penalty | Later |
| 4 | Pool heat (update rate) | Hot pools penalised: stale by landing time | Later |
| 5 | Liquidity concentration | One dominant pool → skip split search | 1 |
| 6 | Live gas price | Raises the bar for splits | Later |
| 7 | Chain MEV profile | BNB: splits also cut sandwich profit | Later |
| 8 | Venue trust | Opaque, very new, fee-on-transfer: penalised / excluded when urgent | 1 |
| 9 | State confidence | Provisional or just-reorged pools penalised | Later |
| 10 | Revert history | Recently failing pools penalised | Later |
| 11 | Own-flow awareness | In-flight orders applied to state before quoting | 1 |

### Tuning loop

Log every decision + outcome (quoted vs filled, revert) to ClickHouse; replay order flow in shadow mode (D5) under different penalties.

## Router contract (D26)

- Our own router on every chain, same CREATE2 address.
- One call per route: any shape from D25, `minOut` + deadline enforced, fee taken in-transaction.
- Immutable, holds no funds between transactions (zero-balance invariant).
- Approvals via Permit2: one-time approval to Permit2, then a signed exact-amount, short-lived permit per trade. New router versions need no re-approvals.

## Quotes & slippage (D27)

- Re-quote at send time; `minOut = fresh quote × (1 − slippage)`. If the fresh quote is worse than the displayed one beyond slippage, don't send: show the new quote.
- Defaults (user-adjustable): new/thin 15% · established 3% · major/stable 0.5% · stop-loss/trailing ×2 · fee-on-transfer + tax.

## Fees (D28)

1% per successful trade (0.9% with referral), taken by the router in the native/quote asset: from the input on buys, from the proceeds on sells. Quotes and `minOut` are net of fee.

## Token safety (D29)

| Layer | Catches | Method | Runs |
|---|---|---|---|
| Round-trip simulation | Honeypot, buy/sell tax, tx limits | `eth_call` + state-override simulator contract | Discovery, promotion, periodic, on alarm |
| Contract inspection | Owner powers: mint, blacklist, pause, set-tax, proxy | Bytecode selector scan | Once, and on ownership change |
| Liquidity safety | Unlocked LP, deployer-held liquidity, pool age | Engine state | Discovery, liquidity events |
| Behavioural | Sells failing, tax drift, liquidity pulls | Our own swap/transfer stream | Continuously |

Policy: block buys on confirmed honeypots; warn on everything else; never block sells. GoPlus as an async second opinion on Base/BNB.

## Open questions

1. ~~Own router contract~~ → **decided (D26).**
2. ~~Quote lifetime & slippage defaults~~ → **decided (D27).**
3. ~~Fees~~ → **decided (D28).**
4. ~~Token safety checks~~ → **decided (D29).**
5. **Penalty values** per cue: tuning, set from measurements via the logging loop, not decided up front.

Still to verify: Permit2 on each chain (D26); `eth_call` state overrides on each provider and GoPlus MegaETH coverage (D29).
