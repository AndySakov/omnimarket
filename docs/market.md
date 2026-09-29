# Market

**Status:** Research snapshot, 2026-09-29. Figures come largely from press releases, review sites and DefiLlama readings and sometimes conflict; treat them as directional. Decisions it led to: D64–D69.

## Positioning

**The EVM-native terminal you can trust, with provably better execution.** Not "a faster Trojan on EVM": speed is table stakes. The gaps are EVM as an afterthought at every leader, trust, and execution quality. The biggest risk is distribution, not technology.

## The prize

| Signal | Figure |
|---|---|
| Axiom revenue | ~$300M in its first 263 days, fewer than 10 employees; ~39% of trading-terminal fees (DefiLlama, Aug 2026) |
| GMGN | ~$52M fees in 30 days; ~36% share |
| Trojan | ~$80M daily volume |
| Trader outcomes | 6% of Solana memecoin traders profitable over 90 days; platforms capture the profit (Galaxy Research) |

## Gaps

### 1. Every leader treats EVM as an afterthought

| Player | EVM stance |
|---|---|
| Axiom | Solana-first; ETH and BNB "newer and thinner" |
| Trojan | Solana only; BNB and Base on the roadmap |
| GMGN, Photon | Solana first, EVM secondary |
| Banana Gun, Maestro, Sigma | EVM-native but Telegram-first, Ethereum-mainnet sniping heritage; Banana Gun weekly fees ~$92k, 63% from Ethereum |
| BullX | Shut down trading June 2026 |

Demand is real: GMGN's fees on BSC (~$30M) dwarf Base (~$0.4M). BNB's 30-day DEX volume (~$22.6B) is roughly level with Base's (~$24.5B) and ahead on shorter windows.

### 2. Trust is broken

- ZachXBT (Feb 2026) alleged Axiom staff used internal tools to look up private user wallets and trade ahead of them.
- Banana Gun (~$3M) and Maestro (~$500k) exploits.

Our answer: no custody and exit without us (D52), pseudonymous telemetry and crypto-shredding, a public on-chain-anchored audit log (D55), verifiable receipts, intents with user-signed minimums (D42, D59), and the anti-snooping rule (D66).

### 3. Execution quality is invisible

Traders lose 15–30% to slippage, taxes and MEV without seeing it; BNB sandwiching is endemic (four.meme itself was hit). Our answer: private builder fan-out (D30), situation-based slippage (D27), safety with evidence (D29), "why did this fire" (D53), exit guarantee (D60), gasless trading (D42), and a public execution-quality report (D65).

### 4. New EVM chains reward whoever shows up first

Robinhood Chain (July 2026): GMGN took 18.5% of volume in three days, about 6× Axiom, which was the first major terminal there. Banana Gun led with day-zero MegaETH support. New EVM chains launch every few months. Our answer: a chain onboarding kit (D67).

## Risks

1. **Distribution beats technology.** Referral trees, cashback, KOL networks, points. On BNB, Binance Wallet's Meme Rush (built into Binance, KYC, Alpha points, partnered with four.meme) can't be out-distributed; we target traders who want better execution and privacy than a custodial exchange wallet.
2. **Fee compression.** Solana terminals net ~0.45–0.75% after cashback; EVM still ~1% (Banana Gun, GMGN). Plan cashback and referrals from launch (D68).
3. **Smaller pond.** Solana has taken up to 85% of memecoin volume at times and is a non-goal. EVM-only is the differentiation, but it caps the market; revisit after launch.
4. **MegaETH has little trading today** (~$1.6M/day DEX volume). Option value, not a market yet (D69).
5. **Base is great to build on, weak to earn from.** Revenue beachhead is BNB (D64).

## Sources

- [State of memecoin trading bots 2026](https://www.crypto-reporter.com/press-releases/the-state-of-memecoin-trading-bots-in-2026-volume-fee-capture-and-market-share-across-the-top-8-126770/)
- [Memecoins attract users, platforms reap profits (Galaxy Research via Cointelegraph)](https://cointelegraph.com/news/memecoins-attract-users-platforms-reap-profits-report)
- [Axiom fees compared](https://axiompedia.com/compare) · [Axiom review](https://coinbureau.com/review/axiom-trade-review)
- [GMGN fees (DefiLlama)](https://defillama.com/protocol/gmgn)
- [Terminals ranked 2026 (MEXC)](https://www.mexc.com/news/1031729)
- [Banana Gun volume](https://markets.financialcontent.com/wral/article/globeprwire-2026-3-17-banana-gun-clears-8b-annualized-volume-as-trading-patterns-mirror-retail-brokerages) · [Banana Gun infra overhaul](https://northpennnow.com/news/2026/mar/02/16b-trading-platform-banana-gun-ships-largest-infrastructure-overhaul-of-2026-across-six-blockchains/)
- [Trojan EVM roadmap](https://memecointradingterminals.com/trojan-trading-terminal/)
- [BNB DEX volume nears Base](https://cryptobriefing.com/bnb-chain-dex-volume-nears-base/) · [Solana memecoin dominance](https://solanafloor.com/news/solana-reclaims-memecoin-volume-dominance-from-robinhood-and-bnb-chain-capturing-85-market-share)
- [Axiom insider allegations (CoinDesk)](https://www.coindesk.com/markets/2026/02/26/zachxbt-alleges-axiom-employee-conducted-insider-trading) · [Banana Gun refund](https://www.blocmates.com/news-posts/telegram-trading-bot-banana-gun-to-refund-3-million-to-users)
- [Four.meme sandwiching (BlockSec)](https://blocksec.com/blog/how-to-play-four-meme-without-getting-sandwiched)
- [Binance Wallet Meme Rush](https://coincentral.com/binance-wallet-launches-meme-rush-to-reshape-meme-coin-trading/)
- [Robinhood Chain terminals](https://medium.com/coinmonks/best-robinhood-chain-meme-coin-trading-platforms-in-2026-gmgn-vs-axiom-vs-fomo-vs-terminal-43e130d87181)
- [MegaETH DEX volume (DefiLlama)](https://defillama.com/dex-aggregators/chain/megaeth)
- [6% of Solana meme traders profitable (HTX)](https://www.htx.com/news/what-are-the-odds-only-6-of-solana-meme-traders-made-a-profi-5gTkabXy/)
