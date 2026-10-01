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
| Base `newFlashblocks` logs (D16) | Receipts were removed from the Flashblocks WebSocket payload in Base's v1 upgrade; an open issue asks to restore them | `newFlashblocks` becomes a tick; each tick triggers one filtered `getLogs` at `pending`. Base fast-loop cost ~13M → ~26M requests/month, still fixed. [Issue #2265](https://github.com/base/base/issues/2265), [issue #613](https://github.com/base/base/issues/613) Superseded by D77: no Flashblocks feed. |
| Kumbaya (D21) | MegaETH's dominant DEX (~80% of chain TVL early 2026). v3-like concentrated liquidity, but non-standard pool bytecode and unverified source | Quoted by simulation until our v3 math passes the shadow check against it. [Report](https://github.com/Stengarl/DeFi_Bullshit_Detector/blob/main/kumbaya-report.md), [DefiLlama](https://defillama.com/protocol/kumbaya) |
| GoPlus on MegaETH (D29) | Not listed | Use the Etherscan API (chain ID 4326) for verified-source checks on MegaETH. [MegaETH Etherscan API](https://mega.etherscan.io/api) |
| MinIO open-source images (D41, D70) | Repository archived April 2026; `minio/minio` returns 404 on Docker Hub and has no active tags on quay.io. Checked 2026-09-29 | RustFS replaces it in dev and staging (D75). [minio/minio](https://github.com/minio/minio) |
| Blacksmith runners (D50) | Organizations only, not personal repositories; `AndySakov/omnimarket` is personal. Checked 2026-09-29 | CI stays on GitHub-hosted runners, free for public repositories (D76). [Blacksmith quickstart](https://docs.blacksmith.sh/introduction/quickstart) |
| Base Denim hard fork (D10, D12, D44) | Base plans to remove Flashblocks. Its upcoming Denim hard fork "replaces Flashblocks with canonical 200ms blocks, so Flashblocks subscriptions and pending state are unavailable after activation". Not active on Sepolia or mainnet; activation time undecided. Live for testing on Vibenet. Checked 2026-09-29. [Migrate from Flashblocks](https://docs.base.org/upgrades/denim/migrate-from-flashblocks) | Base follows canonical blocks only and triggers fire on canonical blocks (D77), so neither Flashblocks nor Denim's timing affects the design. Prototype: branch `prototype/base-tip-following` |
| Base Flashblocks WebSocket (D16, D17) | The raw Flashblocks WebSocket is for node operators. "Applications should not connect to it directly", and "Base does not provide a free public WebSocket RPC endpoint". The free option is HTTP polling of `pending` on `mainnet.base.org`. [Flashblocks FAQ](https://docs.base.org/specifications/flashblocks) | Base follows canonical blocks only and triggers fire on canonical blocks (D77), so neither Flashblocks nor Denim's timing affects the design. Prototype: branch `prototype/base-tip-following` |
| PancakeSwap Infinity (found while checking) | BNB's PancakeSwap Infinity has concentrated-liquidity and **bin** pools, with hooks | Added to D21 scope; bin-pool math still to spec |

## Partly confirmed

| Item | Status |
|---|---|
| State overrides on Base/BNB providers (D29) | Supported by the node software (reth, BSC's geth fork); QuickNode documents `eth_simulateV1` on Base. Chainstack: confirm with the first test call. |
| MegaETH finality (D12) | Settles through the OP Stack with data on EigenDA; "final" = batch finalised on L1. Exact lag not published; measure. [L2BEAT](https://l2beat.com/scaling/projects/megaeth) |

## Measured: Base (M0)

Measured 2026-09-29 from Nairobi with `scripts/measure-base.py`, against the free public RPC (`mainnet.base.org`) only. No HTTP 429s. Local clock within 1ms of `time.apple.com` (±166ms uncertainty).

**Event rates**, 300 blocks sampled evenly over the previous 24h (head 51,929,153):

| Per 2s block | p50 | p90 | p99 | max | mean |
|---|---|---|---|---|---|
| All logs | 834 | 1,393 | 2,565 | 2,902 | 921 |
| M1 logs: v2 Sync + Swap, v3 Swap + Mint + Burn | 73 | 146 | 247 | 299 | 86 |
| v3 Swap | 23 | 75 | 163 | 185 | 34 |
| v4 Swap (M9) | 5 | 14 | 29 | 47 | 7 |
| All logs, KB of JSON | 575 | 940 | 1,636 | 1,858 | 629 |
| M1 logs, KB of JSON | 63 | 126 | 214 | 259 | 74 |

Per second: M1 logs 43 mean, 124 at p99; all logs 461 mean, 1,282 at p99. v2 is a small share of Base's DEX traffic; v3 carries most of it.

**One Kafka partition per chain holds the input rate (D72).** The worst case, every log on the chain recorded twice (fast loop and reconciler) at p99, is about 1.6 MB/s of JSON. Common sizing guidance for one partition is around 10 MB/s, so the headroom is over 6x, and following only M1 pools needs about a tenth of that.

**Delivery delay**, `latest` polled every 100ms for 5 minutes (149 blocks):

| | p50 | p90 | p99 | max |
|---|---|---|---|---|
| Block first seen, minus its timestamp | 473ms | 690ms | 1,941ms | 2,151ms |
| Request round trip to the public RPC | 314ms | 410ms | 1,300ms | 3,757ms |
| Interval between consecutive blocks | 2,052ms | 2,292ms | 3,155ms | 3,450ms |

The delay includes the poll interval and the round trip, so it is an upper bound on the provider's delay, and it is from East Africa: production co-locates with the sequencer (D43). Roughly half a round trip (~160ms) of the p50 is the reply's return trip.

**Pending state over HTTP.** `eth_getBlockByNumber("pending")` changed on 174 of 176 polls over 2 minutes, 3.6 changes per block on average: it updates faster than one client can poll from here (~440ms between polls), consistent with ~200ms Flashblocks. The free HTTP endpoint serves pending state; resolving the 200ms cadence needs a WebSocket or a closer client.

**Reorgs of canonical blocks (D78)**, `scripts/measure-base.py reorgs`, measured 2026-09-30 00:03–01:03 EAT: `latest` polled every 500ms for 60 minutes, 1,801 consecutive blocks. Each block's parent was checked against the hash already held, and every height was re-read 10 blocks (20s) and 300 blocks (10 min) later.

| | Count |
|---|---|
| Reorgs seen when the next block arrived | 0 |
| Heights whose hash changed on a re-read | 0 |

No canonical block was replaced in an hour. Zero in 1,801 bounds the rate at about 3 per 1,801 blocks (1 per ~20 minutes) at 95% confidence, so this shows reorgs are rare, not that they never happen: a longer run or a sequencer incident would tighten it. The public endpoint is load-balanced, so a single lagging backend could in principle report a stale hash; none did. D78's guard stays, as insurance against the rare case rather than a frequent one.

**`eth_call` limits on free endpoints (D82)**, measured 2026-09-30 from Nairobi, Multicall3 `aggregate3` of `tickBitmap` reads at fixed rates for 30s:

| Endpoint | Calls per multicall | Rate | Rate-limited |
|---|---|---|---|
| `mainnet.base.org` | 500 | 1/s | 11 of 30 (20 through) |
| `mainnet.base.org` | 500 | 2/s | 40 of 60 (20 through) |
| `mainnet.base.org` | 20 | 5/s | 130 of 150 (20 through) |
| `base-rpc.publicnode.com` | 500 | 5/s | 0 of 150 |
| `base-rpc.publicnode.com` | 2,000 | 2/s | 0 of 60 |

`eth_blockNumber` on `mainnet.base.org` at 10/s for 40s: none limited. PublicNode serves `eth_call` state 90 blocks back and refuses 100 back (HTTP 403, "archive requests require a personal token").

**A run's calls on each endpoint (D88)**, `engine follow --minutes 1 --check-every 5`, 2026-09-30:

| `--call-rpc` | Outcome |
|---|---|
| `base-rpc.publicnode.com` (default) | Exit 0 after 116s: 18 verification and 539 bootstrap calls, none failed; 26 v2 pairs and 91 v3 pools tracked; 133 shadow checks passed, none failed |
| `mainnet.base.org` | Exit 1 after 152s: one call went 64s without getting past the rate limit (`-32016 over rate limit`), so the call worker gave up |
| A local stand-in answering every `eth_call` with PublicNode's `-32701` | Exit 1 after 8s, at the check before the run |
| The stand-in forwarding to PublicNode, then answering `-32701` from about 15s into the run | Exit 1 after 78s: 63s after the first refused call |

The same for the block endpoint, `--rpc`, with the default `--call-rpc`:

| `--rpc` | Outcome |
|---|---|
| `http://127.0.0.1:1` (nothing listening), `--minutes 1` | Before D88: still running when killed at 90s, having printed nothing. After: exit 1 after 8s, at the check before the run |
| The stand-in forwarding to PublicNode, then answering every request `-32701` from about 17s into the run, `--minutes 3` | Exit 1 after 80s: the follower's `eth_blockNumber` went unanswered for 63s |

## Still to measure (needs live network access)

- Real event rates on BNB and MegaETH (Base measured above), to size the RPC plan (D16).
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
