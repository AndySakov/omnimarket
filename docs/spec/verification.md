# Verification Log

Results of checking every **(verify)** marker in the spec. Checked 2026-09-28 from documentation and public sources. Direct RPC calls from the planning environment were blocked by its network policy, so anything that needs a live call is listed under **Still to measure**.

## Confirmed

| Item | Result | Source |
|---|---|---|
| MegaETH timing (D2) | ~10ms mini-blocks, ~1s EVM blocks | [MegaETH mini-blocks](https://docs.megaeth.com/mini-block) |
| Base Flashblocks (D10) | ~200ms preconfirmations, 10 per 2s block | [Base Flashblocks API](https://docs.base.org/base-chain/api-reference/flashblocks-api/flashblocks-api-overview) |
| Permit2 on all three chains (D26) | Canonical address `0x000000000022D473030F116dDEE9F6B43aC78BA3` on MegaETH, Base, BNB | [MegaETH contracts](https://docs.megaeth.com/developer-docs/overview-1/contracts), [BaseScan](https://basescan.org/address/0x000000000022D473030F116dDEE9F6B43aC78BA3), [BscScan](https://bscscan.com/address/0x000000000022d473030f116ddee9f6b43ac78ba3) |
| Privy server-side signing (D4) | Any EVM chain; MegaETH listed explicitly | [Privy user wallets](https://www.privy.io/user-wallets), [Privy server wallets](https://privy.io/blog/introducing-server-wallets) |
| Trojan uses Privy (wallets.md) | Confirmed | [trojan.com](https://trojan.com/) |
| Prop AMMs on BNB (product.md) | Present, e.g. LunarBase (BNB/USDT, BTCB/USDT) | [BNB Chain market-making landscape](https://www.bnbchain.org/en/blog/bnb-chain-market-making-landscape-liquidity-venues-how-to-integrate) |
| Prop AMMs on MegaETH | None found | — |
| State overrides on MegaETH (D29) | `eth_call` overrides and `eth_simulateV1` documented | [QuickNode MegaETH eth_call](https://www.quicknode.com/docs/megaeth/eth_call), [eth_simulateV1](https://www.quicknode.com/docs/megaeth/eth_simulateV1) |

## Changed the design

| Item | Finding | Change |
|---|---|---|
| Base `newFlashblocks` logs (D16) | Receipts were removed from the Flashblocks WebSocket payload in Base's v1 upgrade; an open issue asks to restore them | `newFlashblocks` becomes a tick; each tick triggers one filtered `getLogs` at `pending`. Base fast-loop cost ~13M → ~26M requests/month, still fixed. [Issue #2265](https://github.com/base/base/issues/2265), [issue #613](https://github.com/base/base/issues/613) |
| Kumbaya (D21) | MegaETH's dominant DEX (~80% of chain TVL early 2026). v3-like concentrated liquidity, but non-standard pool bytecode and unverified source | Quoted by simulation until our v3 math passes the shadow check against it. [Report](https://github.com/Stengarl/DeFi_Bullshit_Detector/blob/main/kumbaya-report.md), [DefiLlama](https://defillama.com/protocol/kumbaya) |
| GoPlus on MegaETH (D29) | Not listed | Use the Etherscan API (chain ID 4326) for verified-source checks on MegaETH. [MegaETH Etherscan API](https://mega.etherscan.io/api) |
| PancakeSwap Infinity (found while checking) | BNB's PancakeSwap Infinity has concentrated-liquidity and **bin** pools, with hooks | Added to D21 scope; bin-pool math still to spec |

## Partly confirmed

| Item | Status |
|---|---|
| State overrides on Base/BNB providers (D29) | Supported by the node software (reth, BSC's geth fork); QuickNode documents `eth_simulateV1` on Base. Chainstack: confirm with the first test call. |
| MegaETH finality (D12) | Settles through the OP Stack with data on EigenDA; "final" = batch finalised on L1. Exact lag not published; measure. [L2BEAT](https://l2beat.com/scaling/projects/megaeth) |

## Still to measure (needs live network access)

- Real event rates per chain, to size the RPC plan (D16).
- Chainstack serving MegaETH mini-block `logs` subscriptions, and per-event WebSocket billing (D16).
- Quote-asset coverage per chain: share of active tokens paired with native or reference stablecoins (D19).
- MegaETH L1 finality lag (D12).
