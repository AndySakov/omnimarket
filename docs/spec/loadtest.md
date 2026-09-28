# Load Testing & Chaos Fuzzing

**Status:** Draft. Decisions: D5, D17, D46, D48, D49.

## Setup (D48)

- **Event source interface:** live · replay (1×/5×/10×) · synthetic (scripted events over real pool state).
- **Execution:** full pipeline in shadow mode, stops before broadcast (D5). Simulation against a local forked node (Anvil / reth fork), free (D17).
- **Tools:** k6, Toxiproxy, Anvil/reth fork, Prometheus + Grafana. Every run publishes its dashboard and a results note.

## Named scenarios

| # | Scenario | How | Pass if |
|---|---|---|---|
| 1 | Steady state | k6: 2k users, 100k armed orders per chain, live feed | All D46 targets at p99 |
| 2 | Flash crowd on a new pair | k6: 5k buys on one new pool within 1s | Click ≤ 100ms p99; fills spread by own-flow awareness; executors keep up |
| 3 | Stop-loss cascade | Synthetic 40% crash, 10k stops on one token | Each fires once; stops before TPs; trigger ≤ 75ms p99 |
| 4 | Copy-trade fan-out | Injected leader swap, 2k followers | Copies ready for the next flashblock / mini-block |
| 5 | MegaETH firehose | Peak mini-block replay at 1×, 5×, 10× | Ceiling recorded; lag ≤ 250ms at 1× |
| 6 | Failover under load | Kill primary during #3 | ≤ 5s; zero duplicate or missed firings |
| 7 | Reorgs & dropped preconfs | Synthetic | Undo exact; corrections reach ClickHouse and UI |
| 8 | Provider trouble | Toxiproxy latency/drops | Fallback takes over; gaps filled; no missed triggers |
| 9 | Soak | #1 for 24h | No memory growth; no shadow-check drift |

## Chaos fuzzing (D49)

Pushes every lever at once, in random combinations, to find what named scenarios miss.

**Levers:** user traffic (rate, bursts, order mix, concentration, edit/cancel races) · market (crash, pump, whipsaw across levels, wicks, liquidity pulls) · chain (reorgs, dropped preconfs, delayed/empty blocks, gas spikes, non-inclusion) · tokens (tax changes, honeypot flips, supply changes, graduation mid-order) · infrastructure (RPC latency/drops/stale data, Kafka loss/lag, Postgres slowness/failover, Privy errors, engine kills, partitions, clock skew).

**Invariants:** exactly-once firing, no stale-epoch firing · nothing fills below its signed minimum, router zero balance · undo equals recompute · legal state transitions only, cost basis matches fills · executor nonce ledger matches chain · prices finite and in range · bounded memory. SLO breaches are recorded, not fatal.

**Search:** seeded runs → weighted random levers → escalate until something breaks → shrink to a minimal reproducer → add to the scenario library. Coverage feedback biases toward unseen state transitions and lever combinations.

**Tiers:**

| Tier | Runs | Strength |
|---|---|---|
| System | Real deployment under chaos | Real network, real processes |
| Deterministic simulation | Engine + execution cores in one process, simulated clock/network/RPC | Thousands of simulated hours per hour; exact replay by seed |

**Build-mode constraint:** engine and execution cores are deterministic given inputs; time, randomness, network and RPC sit behind injectable interfaces.
