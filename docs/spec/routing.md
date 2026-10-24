# Routing

**Status:** Draft. Decisions: D25–D29, D42, D58–D59.

## Quoting (built, #83)

`POST /v1/quotes` answers from the API's own mirror of every tracked pool, kept from `pool-updates.base`. A pool's discovery update carries its key (venue, tokens, fee, tick spacing) and full state, and later updates carry the state and the ticks they touched. Quotes run in the API process rather than the engine because the API already consumes the topic. That way a quote costs no hop to the engine, and the engine's core stays a single-threaded state machine (rule 6). The mirror lags the engine by the topic's delivery time.

- **Math (`crates/routing`).** v2 is `getAmountOut` with the pair's 0.3%. v3 is `UniswapV3Pool.swap`'s exact-input loop: `SwapMath.computeSwapStep`, `SqrtPriceMath` and `TickMath`, with 512-bit `mulDiv`. It steps at most one bitmap word at a time, as the pool does, because each step rounds on its own. A pool that can't take the whole amount gives no quote. verification.md has the check against the on-chain quoter.
- **Routes.** Every direct pool, plus two hops through WETH or USDC. No splits or penalty cues yet (D25).
- **Selection.** The route with the most output net of gas. Gas is a rough per-hop estimate: 60k for the transaction, plus 90k per v2 hop and 130k per v3 hop, at 0.01 gwei. It's converted to the output token at the display prices, or ignored when they're unknown. #84 replaces the estimate with a simulation.
- **Fee (D28).** 1%, from the input when it's WETH, USDC or USDT (a buy), otherwise from the output (a sell). A pair with neither is refused.
- **Slippage (D27).** Defaults when the request sets none: 0.5% between two quote assets, 3% for an established token, 15% for a thin one or one with no price yet. The reason goes in the quote.
- **Price impact.** The route's own price before our fee, against the display prices of both sides. Stablecoins count at $1.
- **Expiry and repeats.** A quote expires 4s (2 blocks) after the latest price update's block time. Until then, the same request with the same client request ID gets the same quote. A different request reusing the ID gets its own, so one client can't read or pin another's. IDs are at most 64 bytes, and at most 10,000 unexpired quotes are kept. Scoping repeats to a session comes with accounts (#85).
- **Not yet.** The on-chain shadow check of sampled quotes, counted in the engine status, and re-quotes on the `account` topic (#85) are left for later.

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
- Executes signed intents (D42): verifies signature, terms, deadline and **submitter** (our executor set or the user, D58), pulls at most the signed maximum via Permit2, enforces the stricter of the signed minimum rate and the submitter's tighter minimum (D59), caps fee and gas refund at the signed limits, sends output to the signer. New router versions need no re-approvals.

## Quotes & slippage (D27)

- Re-quote at send time; `minOut = fresh quote × (1 − slippage)`. If the fresh quote is worse than the displayed one beyond slippage, don't send: show the new quote.
- Defaults (user-adjustable): new/thin 15% · established 3% · major/stable 0.5% · stop-loss/trailing ×2 · fee-on-transfer + tax.

## Fees (D28)

1% per successful trade (no referral discount: D85), taken by the router in the native/quote asset: from the input on buys, from the proceeds on sells. Quotes and `minOut` are net of fee.

## Token safety (D29)

| Layer | Catches | Method | Runs |
|---|---|---|---|
| Round-trip simulation | Honeypot, buy/sell tax, tx limits | `eth_call` + state-override simulator contract | Discovery, promotion, periodic, on alarm |
| Contract inspection | Owner powers: mint, blacklist, pause, set-tax, proxy | Bytecode selector scan | Once, and on ownership change |
| Liquidity safety | Unlocked LP, deployer-held liquidity, pool age | Engine state | Discovery, liquidity events |
| Behavioural | Sells failing, tax drift, liquidity pulls | Our own swap/transfer stream | Continuously |

Policy: block buys on confirmed honeypots; warn on everything else; never block sells. GoPlus as an async second opinion on Base/BNB.

**Round trip (built, #89).** `contracts/src/SafetyProbe.sol` is never deployed. One `eth_call` from the API puts its runtime code, with 1 ETH, at a throwaway address as a state override, and runs it:

- It wraps 0.01 ETH into WETH and buys the token through a pool trading it against WETH: Uniswap v2 (a transfer, then `swap`) or v3 (`swap` with its callback). It then sells everything it received back through the same pool.
- It reports the pool's own output for the buy against what arrived, the tokens it sent against what reached the pool, the WETH back, and the failing leg's revert reason.
- **Taxes:** the buy tax is the share of the pool's output that didn't arrive, and the sell tax the share of the tokens sent that didn't reach the pool.
- **Verdict:**
  - *honeypot* when the sell reverts, or returns under half of what was spent (dust);
  - *warning* for a tax of 1% or more either way, or a buy that fails;
  - otherwise *passed*.
- **The pool:** the token's main pool when that's a WETH pool, else any WETH pool the API mirrors. Tokens quoted only in a stablecoin aren't checked yet.
- **Coverage gap:** a fee-on-transfer token can't be sold through a v3 pool at all, since the pool's balance check reverts. So a v3 check of such a token reads honeypot, which is true of that pool.

The API's worker (`api --call-rpc URL`) checks a token once when a pool trading it against WETH is discovered, and on its page's first read. It checks again every 5 minutes while the page has been read in the last 30. The report is `TokenSnapshot.safety` and its summary is a discovery row's `safety`. The probe's committed runtime code (`crates/execution/src/safety_probe.hex`) is checked against its source in `scripts/verify.sh`.

## Open questions

1. ~~Own router contract~~ → **decided (D26).**
2. ~~Quote lifetime & slippage defaults~~ → **decided (D27).**
3. ~~Fees~~ → **decided (D28).**
4. ~~Token safety checks~~ → **decided (D29).**
5. **Penalty values** per cue: tuning, set from measurements via the logging loop, not decided up front.

Still to verify: Permit2 on each chain (D26); state overrides on BNB's and MegaETH's providers, and GoPlus MegaETH coverage (D29). On Base, mainnet.base.org, PublicNode, dRPC and 1RPC all apply them (verification.md).
