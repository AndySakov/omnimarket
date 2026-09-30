# EVM Landscape

**Status:** Research snapshot, 2026-09-29. Figures come largely from press releases, review sites and DefiLlama readings and sometimes conflict; treat them as directional. Decisions it led to: D64–D69. OmniMarket is a proof of concept (D85): this research picks the chains to build on and the problems worth engineering time. It isn't a go-to-market plan.

## Why EVM

The leading trading terminals are Solana-first, and several have announced EVM support. Taking a terminal to EVM isn't a port: each chain has different block timing, finality, DEX math, launchpads and MEV exposure. That makes it a good test of whether a backend design holds up across chains.

## Where EVM trading happens

| Chain | Signal | What it means for the build |
|---|---|---|
| BNB Chain | 30-day DEX volume ~$22.6B, level with Base and ahead on shorter windows; most EVM memecoin trading fees (GMGN: ~$30M on BSC vs ~$0.4M on Base) | Where EVM memecoin trading actually happens, so the second chain (D64) |
| Base | 30-day DEX volume ~$24.5B; no public mempool; public RPC is free to follow | Lowest-risk chain to prove the architecture on, so the first chain |
| MegaETH | ~$1.6M/day DEX volume; ~10ms mini-blocks | Hardest ingest problem, least trading today, so last (D69) |
| New EVM chains | One every few months (e.g. Robinhood Chain, July 2026); early volume goes to terminals that support them on day one | Adding a chain should be a packaged process (D67) |

## What goes wrong for traders

### Execution quality is invisible

Traders lose 15–30% to slippage, taxes and MEV without seeing it, and sandwiching on BNB's public mempool is endemic (four.meme itself was hit). The design answers with private builder fan-out (D30), situation-based slippage (D27), safety checks with evidence (D29), "why did this fire" (D53), an exit guarantee for stops (D60), gasless trading (D42), and a public execution-quality report (D65).

### Keys and privacy are the attack surface

Public incidents show where terminals get hurt:
- Telegram-bot exploits: Banana Gun (~$3M, refunded) and Maestro (~$500k).
- Feb 2026: allegations that staff at a major terminal used internal tools to look up private user wallets.

The design answers with no custody and an exit that works without us (D52), policy-limited keys (D57), pseudonymous telemetry, a public on-chain-anchored audit log (D55), intents with user-signed minimums (D42, D59), and the anti-snooping rule (D66).

## Sources

- [State of memecoin trading bots 2026](https://www.crypto-reporter.com/press-releases/the-state-of-memecoin-trading-bots-in-2026-volume-fee-capture-and-market-share-across-the-top-8-126770/)
- [GMGN fees (DefiLlama)](https://defillama.com/protocol/gmgn)
- [Trojan EVM roadmap](https://memecointradingterminals.com/trojan-trading-terminal/)
- [BNB DEX volume nears Base](https://cryptobriefing.com/bnb-chain-dex-volume-nears-base/)
- [Insider-tracking allegations (CoinDesk)](https://www.coindesk.com/markets/2026/02/26/zachxbt-alleges-axiom-employee-conducted-insider-trading) · [Banana Gun refund](https://www.blocmates.com/news-posts/telegram-trading-bot-banana-gun-to-refund-3-million-to-users)
- [Four.meme sandwiching (BlockSec)](https://blocksec.com/blog/how-to-play-four-meme-without-getting-sandwiched)
- [Robinhood Chain terminals](https://medium.com/coinmonks/best-robinhood-chain-meme-coin-trading-platforms-in-2026-gmgn-vs-axiom-vs-fomo-vs-terminal-43e130d87181)
- [MegaETH DEX volume (DefiLlama)](https://defillama.com/dex-aggregators/chain/megaeth)
