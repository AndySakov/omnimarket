# Observability, Replay & Safeguards

**Status:** Draft. Decisions: D52–D56. Principles: [D52](decisions.md) (cypherpunk ground rules).

## Ground rules (D52)

1. Brakes stop only our automation, never the user; funds are never frozen or moved.
2. Users can always leave without us: export keys, or self-submit intents to the router.
3. Every brake is public (tamper-evident log, status page) and auto-expires.
4. Verifiable, not just trusted: open source, reproducible builds, verifiable trade receipts.
5. Minimal data about people: pseudonymous telemetry, per-user encryption with crypto-shredding.

## See everything (D53)

**Stack:** Prometheus · Loki · Tempo · Pyroscope · Grafana (self-hosted).

**Lineage:** every record carries the IDs of what caused it.

`chain event → pool update → price update → trigger check → firing ID → intent hash → route decision → simulation → executor tx → landing → position update → notification`

Stored as edges in ClickHouse; walk backwards for root cause, forwards for blast radius.

**Wide, typed events** (Protobuf), one per unit of work. **Decision records** keep the alternatives considered.

**Sampling:** money paths 100%; everything else keeps slow/failed traces + 1%.

**Skeleton (built, D70):** the `telemetry` crate sends `tracing` spans over OTLP/HTTP to Tempo, read in Grafana. Export runs on the batch processor's own thread and uses the wall clock only for span timing, so a traced core makes the same decisions (a test checks the replay digest and recording are unchanged with tracing on). Each decision record is a span carrying `lineage.id`, `lineage.caused_by` and every field, under its run's span. Exporter warnings go to stderr. `cargo run -p sim --bin toy-run -- <seed>` traces one toy core run to the local stack (`scripts/stack up`, Grafana on :3000). The local stack also runs Prometheus, Loki and Pyroscope with provisioned alert rules (infra.md), but they watch only the stack itself: the engine and API don't yet export metrics, ship logs or push profiles. Span export is best effort: the input log, not the trace, is the record replay relies on.

**Engine status (built, #79).** After every canonical block the engine publishes an `omnimarket.status.v1.EngineStatus` to `status.base`, keyed by core instance; the API serves the latest as `GET /v1/status` and the `status` topic (frontend.md). Its fields:

| Field | What it is | From |
|---|---|---|
| `block_number`, `block_hash`, `block_timestamp` | The block just applied: the engine's head | The block |
| `lag_blocks` | The chain's latest block number when the follower read this block, minus this block's number. Unset in recordings from before the follower recorded the head | The block (the follower's head, recorded with it) |
| `lag_ms` | From the block's timestamp to the block's arrival at the core. An upper bound, as block timestamps have seconds resolution | The core's recorded clock reading for the block |
| `v2_pools`, `v3_pools` | Pools tracked with state, per venue. All are active until #41 tiers known and active pools | Core state |
| `shadow_checks`, `shadow_check_mismatches` | Shadow state checks (D21) answered since the core started, and those that disagreed with the chain | Core state |
| `uptime_ms` | From the core's first block to this one | The core's recorded clock |
| `mode`, `core_instance`, `recording` | Live or replay, the core instance whose session it is, and whether its inputs are recorded (always, for a replay) | Set by the publisher, not the core |

No field reads a wall clock: time comes from the core's clock reads, which are recorded inputs, so a replay of a recording publishes the same statuses apart from the publisher's fields, and only `mode` differs there (`a_recording_replays_to_the_same_statuses`). The status adds no clock read, so recordings made before it replay as they did.

| Stage | Recorded | Replayable via |
|---|---|---|
| Ingestion | Raw provider messages + arrival time | Input log (D54) |
| Pool state | Snapshots + every update | Point-in-time rebuild |
| Pricing | Price, source pools, weights | Recompute |
| Triggers | Checks that fired, level crossed | Replay |
| Routing | Candidates, scores, penalties, cues | Replay |
| Signing | Request, intent hash, policy result, latency (never keys) | Audit log |
| Simulation | Request, result, block | Re-run on a fork at that block |
| Submission | Every send, every endpoint, timestamps, responses | Audit |
| Landing | Receipt, position in block, fill vs signed minimum | Chain |
| Configuration | Config version on every decision | Git |

**Alerts:** D46 SLOs with burn-rate alerts. Page (money at risk) · ticket (budget burning) · info. Pages via Grafana → Telegram.

**Invariant monitors in production:** executor nonces vs chain · positions vs on-chain balances · router zero balance · fee and refund reconciliation · undo vs recompute.

**Canaries:** shadow canary every minute per chain; daily real-funds round trip per chain.

**Incident console:** search by user, order, tx, token or time → lineage graph, decisions, traces, replay.

## Replay anything (D54)

- **Input log:** every core input (provider messages, RPC responses, API requests, clock reads, seeds) → Kafka → Cloud Storage.
- **Archive (built):** `engine archive` copies one core instance's recording from Kafka to object storage (RustFS in dev and staging, D75; Cloud Storage in production) through `det::archive`. Layout: `<bucket>/inputs/<chain>/<core instance>/<first seq, 20 digits>.pb`, each segment 10,000 length-delimited `omnimarket.det.v1.InputRecord`s in log order. Writing the same recording again writes the same objects, so an interrupted archive can rerun. `engine replay --from-archive` reads the segments back in order and rejects a gap in `seq`, giving the same run as a replay from Kafka. A `manifest` object with the record count is written last, so an archive without one reads as unfinished and a missing trailing segment reads as truncation, not as a replay divergence. For now archiving is a command run after a recording, holding the whole recording in memory: fine for hours (tens of MB), not for a 30-day log. A continuous copy (a Kafka sink to Cloud Storage) comes with production deploys.
- **Exact replay:** snapshot + recorded inputs reproduce every decision (deterministic cores, D49).
- **det runtime (built):** four traits, each with a real, a simulated, a recording and a replay implementation (D72, D74).
  - `Clock` (wall clock, or tokio's paused clock in simulation, which jumps to the next timer whenever the core waits) and `Rng` (ChaCha8 from a 64-bit seed; production draws the seed from the OS once). Neither waits, so both are plain methods.
  - `EventSource` (cancel-safe; a channel receiver in production) and `Rpc` (the returned future owns what it needs, so calls run concurrently) are async. The core runs as one task on a current-thread runtime and waits with a `biased` `select!` (D74). `Signer`, `Broadcaster` and `Store` follow the same shape later.
  - Workspace clippy bans keep every other crate off the wall clock, OS randomness, threads and std hash maps (D73).
- **Recording path (built):** each recording wrapper writes an `InputRecord` (sequence number, source, arrival time, payload) to a recording sink when the input reaches the core: the core's configuration first (D83: so a replay rebuilds the same core from the log alone: a replay under a different config issues different calls and diverges), then every clock read, every Rng draw, every event (and the end of the stream), every RPC response tagged with its call number. All of one core's wrappers share one recorder, so the sequence number is the exact order the core saw inputs across sources. A cancelled event wait records nothing. Tests use an in-memory sink. The real one is `det::kafka::KafkaSink` (rdkafka, D79): it writes each record as a proto `InputRecord` to `inputs.<chain>`, keyed by core instance, with an idempotent producer. A write can't fail at the call site, so delivery errors are kept and reported by `flush`; a full local queue slows the core rather than dropping an input. `read_input_log` reads one core instance's records back in order and rejects a gap or reordering. An integration test records a seed to `inputs.sim` and replays it from Kafka to the same digest (D72).
- **Replay path (built):** `Replay` hands the recording back as det sources that share one log. An input is ready only when its record is next, so replay enforces the recorded order. RPC responses match their call by number, whatever order they arrive in. Replay panics with "replay diverged" when the core reads out of order, waits for an input that isn't next, or finishes with records unread. The M0 demo (`sim`) runs a toy core over 100 seeds: the same seed gives the same decision digest, a recording replays to that digest, and altering one recorded input changes it.
- **Pinned real recording (built, D99):** `crates/engine/tests/fixtures/base-replay/` holds 2 minutes of real Base (61 blocks from 52037493 to head 52037553, 5,125 logs, 1,077 inputs, 1,261 trades, 203 shadow checks), recorded with `--check-every 10`: `inputs.pb.zst` is the input log as one file (`det::file`: the archive's length-delimited `InputRecord`s in log order, zstd level 19: 541 KB from 52.5 MB), and `summary.txt` is what `engine follow` printed at the end of it (head, block digest, update digest and counts). `pinned_replay.rs` replays the file through today's engine, offline in about a second, and asserts the summary text. A change to the engine's decisions moves a digest or a count and fails it. The recording has v3 pools with a second event in the block they were first seen in, so changing the bootstrap guard's `>` to `>=` (applying a same-block event twice) fails it: 1,276 pool updates instead of 1,244. A replay that diverges (the engine asking for inputs in a different order) panics with "replay diverged" instead.
  - **Re-recording.** When a PR changes the engine's decisions on purpose, or changes what it records (its config, its calls), re-record and say in the PR why the digests moved:
    ```
    cargo build -p engine --release
    target/release/engine follow --rpc https://mainnet.base.org --call-rpc https://base-rpc.publicnode.com \
      --minutes 2 --check-every 10 --record-to crates/engine/tests/fixtures/base-replay
    cargo test -p engine --test pinned_replay
    ```
    Check that the new recording still catches the `>=` guard (edit `v3.rs`, run the test, see it fail, revert), and update the numbers above. A recording that starts at the chain head sees every busy v3 pool for the first time in its first blocks, so the same-block case comes up in any stretch so far; a stretch without it would show as the guard's mutant passing.
- **Point-in-time everywhere:** engine, Postgres (PITR), ClickHouse (versions), config (Git), binaries (signed digests).
- **Replay-gated deploys:** replay recorded production inputs through the release candidate and review every differing decision → shadow → one chain first → auto-rollback on SLO burn.
- **Retention:** input log 30 days hot / 1 year cold; money-path traces and decisions 1 year; audit log forever.

## Trust nothing (D55)

- **Independent watcher:** separate code, separate provider, separate region. Reconciles every on-chain router/executor transaction against our records. Any mismatch → all executors paused.
- **Tamper-evident audit log:** hash-chained, append-only; root anchored on Base daily.
- **Monitor the monitors:** absent-data alerts, external dead-man's switch, clock-drift alerts.

## Brakes (D56)

**Automatic breakers**

| Scope | Trips when | Effect |
|---|---|---|
| Trade | Quote deviates from display price; fee/gas over cap; simulation fails | Trade blocked |
| User (automated flows) | Server-signed volume over cap; anomaly | Automation held; manual + self-submit still work |
| Pool type | Shadow mismatch over threshold | Simulated quotes |
| Chain | Lag; provider head disagreement; deep reorg; revert/sim-failure spike | Firing paused on that chain |
| Executor fleet | Gas spend > recovered; nonce-gap storm; top-up cap | Executors paused |
| Global | Independent watcher mismatch | All executors paused |

**Manual levels** (1h auto-expiry, public log)

| Level | Effect | Users can still |
|---|---|---|
| 1. Caution | Wider margins, no splits, no new venues | Everything |
| 2. Stop automation | No triggers or copies; orders stay armed | Trade manually, self-submit |
| 3. Stop submissions | Executors off | Self-submit, export keys |
| 4. Freeze server signing | Privy policy deny | Sign in own session, export keys |

On release, crossed orders fire with a fresh quote and the user is notified.
