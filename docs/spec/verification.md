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
| Clippy config lookup (D72, D73) | Clippy uses the nearest `clippy.toml`, walking up from each crate's directory; a crate's own file replaces the root one rather than merging with it. Checked 2026-09-29 on clippy 1.98: the root bans reject a fixture two directories down, and removing `crates/det/clippy.toml` makes `det` fail on `SystemTime::now` | Local run |
| ChaCha8 stream is portable (D72) | `ChaCha8Rng::seed_from_u64(0)` gives the same first three `u64`s on rand_chacha 0.3.1 and 0.10.0; pinned in `det`'s tests | Local run, [rand reproducibility](https://rust-random.github.io/book/crate-reprod.html) |
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

---

# Final Consistency Review (2026-09-28)

Every spec doc read against all 58 decisions, looking for contradictions, stale references and gaps.

## Documentation drift fixed

| Doc | Fix |
|---|---|
| product.md | MVP list updated: copy trading, trailing stops, split routing, private BNB submission and bonding curves are phase 1 (D25, D30, D36–D38); stretch list rewritten; DEX table adds launchpad hook pools, PancakeSwap Infinity bin pools, four.meme |
| architecture.md | Diagram redrawn for intents, executors, router, input log, Kafka-fed standby, independent watcher; execution responsibilities and "around the hot path" added |
| wallets.md | Signing latency, nonce ownership and policy points marked resolved by D42, D32, D57; router question closed; smart-account note points to D47 |
| indexer.md | Active-pool reasons add followed wallets (D38) and bonding curves (D36); discovery covers launchpad events; Base fast loop from own node in prod (D44) |
| data.md | Input log topic (D54), lineage edges, decision records, audit log (D55), crypto-shredding (D52) |
| triggers.md | Arming signs in the user's session (D57), submitter field (D58), brake behaviour (D56) |
| infra.md | Pyroscope, cosign/SBOM, independent watcher placement, dead-man's switch |
| slas.md | Click-path signing moved to the user's session, marked to measure (D57) |
| highlights.md | D8 entry (execution owns executor nonces), D16 entry (Base tick + pending `getLogs`) |
| decisions.md | Amendment notes on D8 and D10 |

## Design gaps found (all approved)

**G1. Pre-signed trigger intents fix the amount and minimum at arming time.** Three consequences:
- A multi-level take-profit capped at current holdings (D39) can't sell less than the signed exact amount.
- A trailing stop's minimum was signed at the starting level, so after the trail rises its on-chain protection is loose.
- The fresh-quote protection of D27 doesn't apply to pre-signed intents at all.

*Decided (D59):* intents carry a **maximum** amount and a minimum output **rate**; the submitter may use less (minimum scaled pro-rata) and may supply a **tighter** minimum, never a looser one. Execution always supplies fresh quote × (1 − slippage). The signed fee rate and a signed gas-refund cap bound what the router may deduct.

**G2. A stop-loss can't get out of a gap-down.** If the price gaps below the signed floor, the simulation fails and the order can't fill; D34's "retry with a fresh quote" needs a new signature.

*Decided (D60):* stop-loss and trailing orders default to **exit guarantee**: if the signed floor can't be met, the server re-signs a fresh intent at the current quote under policy caps (D57), matching Trojan's behaviour. Users can switch it off ("never below X", a stop-limit). Limit orders and take-profits default off.

**G3. The execution service has no high-availability design.** D40 covers the engine's standby, but execution holds executor nonce counters and the firing-ID dedupe, and D41 put their durable home in the central Postgres, a cross-region hop on the hot path that D50 forbids.

*Decided (D61):* execution runs leader/standby with the same lease + fencing epochs as D40. Its durable state (firing-ID dedupe, executor nonce ledger) lives in a small **regional** Postgres per chain (synchronous replica, same region, ~1ms writes). The central Postgres keeps users, orders and positions.

**Accepted as-is (noted):**
- Executors pay gas on transactions that revert and can't recover it. Blocking simulation (D46) prevents most; the fleet breaker (D56) and per-user revert-rate limits contain griefing.
- Rotating the executor set means re-signing armed orders (D58). Mitigation: a large, stable executor set per chain (e.g. 32), rotated rarely.
- Bonding-curve venues on BNB take native BNB: the router unwraps WBNB for those hops.

## Still open

~~Frontend depth~~ → both: thin prototyping UI and a full terminal UI owned by Jutin (D62, frontend.md).

**To verify:** Base sequencer accepting direct submission (D43) · Base and BNB sequencer/builder locations (D50, D51) · Kumbaya launchpad mechanics (triggers.md) · Oracle Always Free limits (D50) · Chainstack state overrides (D29).

**To measure (needs live network access):** event rates per chain (D16) · Chainstack MegaETH mini-block `logs` (D16) · quote-asset coverage per chain (D19) · MegaETH L1 finality lag (D12) · Privy signing latency, server and browser (D46, D57) · BNB builder inclusion latency (D30) · provider delivery delay per chain (D46).

**Tuning values (set from measurement, not decided up front):**

| Value | From |
|---|---|
| Liquidity floor per chain (±2% depth), new-pool grace window, demotion hysteresis | D11, D24 |
| Hot-undo window per chain | D12 |
| Routing penalty weights per cue | D25 |
| Slippage defaults (starting values set) | D27 |
| Safety re-check interval | D29 |
| Tip levels (relative to landed tips) | D33 |
| Order limits (starting values set), trailing-save threshold | D39 |
| Breaker thresholds, per-user caps, executor gas float and top-up caps | D56, D57 |
| Executor set size per chain | D58 |
