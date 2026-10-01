# Infrastructure

**Status:** Draft. Decisions: D17, D43, D44, D50, D51.

## Environments (D50)

| Env | Where | Cost |
|---|---|---|
| Dev | k3d + docker compose, Anvil forks | Free |
| CI | GitHub Actions on GitHub-hosted runners (D76) | Free: public repository |
| Staging | Oracle Cloud Always Free ARM, k3s, free RPC tiers | Free |
| Prod | Per-chain regional clusters + central region | Paid, offset by credits (D51) |

## Local stack

`scripts/stack up` starts the dev stack from `deploy/local/compose.yaml` and waits until every service is healthy (about 10s once images are pulled); `scripts/stack down` stops it and deletes its data.

| Service | Port | Notes |
|---|---|---|
| Postgres 18 | 5432 | user, password and database `omnimarket` |
| Kafka 4.3 (KRaft, single node) | 9092 | advertised as `localhost:9092` for clients on the host |
| RustFS 1.0 (D75) | 9000 S3, 9001 console | access key `omnimarket`, secret `omnimarket-dev`; `scripts/stack up` creates the `omnimarket-inputs` bucket (input-log archive). The engine reads the keys from `OMNIMARKET_S3_ACCESS_KEY` / `OMNIMARKET_S3_SECRET_KEY`, defaulting to these |
| Tempo 3.0 | 3200 API, 4317 OTLP gRPC, 4318 OTLP HTTP | local storage |
| Grafana 13 | 3000 | anonymous admin, Tempo provisioned as the default data source |
| Anvil (Foundry 1.8) | 8545 | forks Base; set `BASE_RPC_URL` to use your own endpoint instead of the public one |

## Production layout

| Where | Runs |
|---|---|
| Per-chain region (nearest sequencer/builders) | Engine + standby, execution, local Kafka, Base node (Base only) |
| Central region | Postgres, ClickHouse, API/WebSocket gateway, candle service, history job, mirrored Kafka topics |

No hot-path call crosses regions.

## Data stores (self-hosted on Kubernetes)

CloudNativePG (Postgres) · Strimzi (Kafka) · Altinity operator (ClickHouse) · RustFS in dev and staging, Cloud Storage in production (snapshots, input-log archive; D75).

## Tooling

Terraform · Helm · Argo CD · Prometheus · Grafana · Loki · Tempo · Pyroscope · SOPS (staging) · Cloud KMS (executor keys encrypted at rest, D57) · cosign + SBOM for images.

**Independent watcher (D55)** runs outside the main clusters: its own small deployment, its own RPC provider, ideally its own region. **External dead-man's switch** pages if monitoring goes quiet.

## CI pipeline

Unit tests · mutation testing (D89) · fix PRs' regression tests checked on `main`'s code (D92) · contract fork tests (Anvil) · Protobuf compatibility checks · nightly deterministic-simulation fuzz runs (D49).

CI runs on GitHub-hosted runners (`.github/workflows/ci.yml`, D76). CI and the local commit gate run the same `scripts/verify.sh` on every commit that isn't frontend-only, so such a commit that passes locally passes the same checks in CI. Locally, a tracked git pre-commit hook (`.githooks/`, installed by `scripts/setup.sh`) gates every commit, and a Claude Code `PreToolUse` hook refuses agent commits with `--no-verify`. CI is the gate nothing can skip.

The hook runs `scripts/verify-fast.sh` (D86). A commit whose staged files are all under `web/terminal/` gets the frontend's fast checks (`npm run verify:fast`: typecheck, lint, unit tests). Every other commit runs `scripts/verify.sh`: any path outside `web/terminal/`, including `clippy.toml`, `.github/`, docs or a new top-level directory, gets the full checks. `verify.sh` doesn't touch the frontend, so a backend clone needs no Node or `node_modules`.

CI's `frontend` job runs the terminal UI's full checks, `npm run verify:pr` in `web/terminal/`: typecheck, lint, unit tests, a check that the TypeScript generated from `proto/omnimarket/api` isn't stale (`npm run api:check`, D91), the production and Storybook builds, and Playwright's end-to-end, visual and accessibility tests in Chromium. It isn't a required check on `main` yet.

Protobuf checks (`scripts/proto-check.sh`) run `buf lint` and `buf breaking` against `main`; CI fetches `main` for the comparison. `scripts/buf` pins buf's version and checksum and downloads it once into `.tools/`, retrying when GitHub's release CDN answers with an error, so local runs and CI use the same binary with nothing installed globally. CI's `verify` and `frontend` jobs (the latter runs `buf generate` to check the generated API types) cache `.tools/` keyed on `scripts/buf`, so CI downloads buf only when the pinned version changes.

CI also runs a Kafka service container (the same `apache/kafka` image as the local stack) for the input-log integration test, `cargo test -p sim --test kafka -- --ignored`. Locally it runs against `scripts/stack up`.

**Mutation testing (D89).** `scripts/mutants.sh` runs cargo-mutants, which changes the code one small mutation at a time (a condition flipped, a return value replaced) and checks that some workspace test fails. A mutant no test catches is code the tests run but don't check.
- **Builds carry over.** Each worker is a persistent git worktree under `target/mutants/workers/` with its own target dir. Each run checks the workers out at a snapshot of the working tree, uncommitted changes included, so only changed crates rebuild.
- **Results carry over.** A ledger skips mutants caught in earlier runs until `--fresh`.
- **Cleaning up.** The workers are git worktrees, so `--clean` removes them with `git worktree remove`, rather than leaving entries behind as `cargo clean` does.
- **Diff mode.** `--diff origin/main` tests only mutants in code changed since `main`.
- **The baseline covers the workspace.** The script first checks that the unmutated workspace's tests pass, since cargo-mutants' own baseline covers only the mutated packages.

The `mutants` workflow (`.github/workflows/mutants.yml`) runs it on PRs that touch Rust, for the changed code only, and nightly on `main`. The nightly run tests every mutant not already caught. Its ledger is cached per ISO week, so the week's first run tests everything again. The workers' target dirs are cached from `main`'s runs. Missed mutants appear in the job summary and the `mutants` artifact, and turn a PR's run red. It isn't a required check: it reports to the author and the watchdog.

**Regression check (D92).** On a PR whose body closes an issue labelled `bug` with a closing keyword (`Fixes #n`, `Closes #n`, ...), the `regression-check` workflow (`.github/workflows/regression-check.yml`) runs `scripts/regression-check`. It finds the Rust tests the PR adds or changes since its merge base with `main`, and runs them on the PR's head, where all must pass, and on `main`'s code with the PR's tests put in, where at least one must fail.
- **The PR's tests on `main`'s code.** A worktree of `main` gets the PR's version of every changed file under a `tests/` directory (or named `tests.rs`). In every other changed source file it keeps `main`'s code and takes the PR's `#[cfg(test)]` items in place of `main`'s. A source file `main` doesn't have can't take its tests, so they're reported as new.
- **What counts.** A test that fails on `main`'s code proves the bug. One that doesn't build there (it calls something the fix adds) is reported but doesn't count. When one file's test module doesn't build on `main`'s code, each file's tests run with only its own module put in, so it can't hide the others. `#[ignore]`d tests don't run.
- **The report** goes to the job summary: each test's result on both sides, the panics of the tests failing on `main`, and the build errors.
- **Scope.** Rust tests run by `cargo test`. Contract tests (`forge`) and the terminal UI's tests aren't covered.
- **Builds.** Each tree has its own target dir under `target/regression-check/` (cargo's fingerprints name sources relative to the workspace root, so a shared one would reuse the head's binaries on `main`'s code), cached by `rust-cache`.
- **Replays.** `workflow_dispatch` takes any base and head commit; locally, `scripts/regression-check --base <ref> --head <ref>`. `scripts/test_regression_check.py`, run by `verify.sh`, checks the script on a throwaway repository.

It isn't a required check yet. Once it has passed on two fix PRs, it joins `verify` and `watchdog/review` in branch protection.

**Review gate (D81, D90).** Branch protection on `main` requires two status checks on a PR's head commit, for admins too: `verify` (the CI job) and `watchdog/review`. A separate watchdog agent session posts its verdict as a PR comment whose first line is ``## Watchdog reviewing `<sha>` `` (pending) or ``## Watchdog review: `<sha>` passes`` / ``fails …``. The `watchdog-status` workflow (`.github/workflows/watchdog-status.yml`), triggered by the comment, posts the matching `watchdog/review` status on that commit, linked to the comment, but only if it is still the PR's head and the comment comes from `AndySakov`. It never checks out PR code. A new push needs a new review. Builders merge with `scripts/work merge`, which also requires `frontend`. The protocol agents follow is in `CLAUDE.md`.

**Acceptance criteria (D95).** A PR body's "Acceptance criteria" table (`.github/pull_request_template.md`) maps each acceptance criterion of the issue it closes to the tests that prove it, or to `manual: <evidence>`. The `criteria` job in `ci.yml` runs on PRs after `verify` and `frontend`. It fetches the PR body and each closed issue over REST, and runs the `criteria` crate's binary over them and the tests that passed in this run: `verify` uploads `cargo test`'s output (the workspace and the Kafka test), and `frontend` uploads the Vitest and Playwright JSON reports as the same `test <name> ... ok` lines (`scripts/frontend-passed-tests.sh`). The job fails when a criterion has no row, a row names neither a test nor `manual:` evidence, or a named test didn't pass in this run (it doesn't exist, failed or was ignored). Its summary lists the `manual:` rows for the watchdog, and the rows that quote no criterion (extra claims, or misquotes, whose criterion then also has no row). After editing a PR body, re-run the `criteria` job. It isn't a required check yet: it joins `verify` and `watchdog/review` once it has run green on two PRs.

**Cloud agent sessions (D90).** Claude Code cloud sessions can't reach GitHub's GraphQL API or write commit statuses, so `scripts/work` does the agents' GitHub reads and writes over REST (its claim, ledger and PR-body rules are tested offline by `scripts/test_work.py`, which `verify.sh` runs), and the gate's status comes from the workflow above. A `SessionStart` hook (`.claude/hooks/session-start.sh`, cloud sessions only) starts dockerd, installs the git hooks, fetches buf, npm deps and Playwright's Chromium (falling back to the container's preinstalled one through `PLAYWRIGHT_CHROMIUM_PATH`), and warms the cargo build in the background; a cold `verify.sh` takes about 3.5 minutes there. The build account's environment needs network access to Base's RPC endpoints (`mainnet.base.org`, `base-rpc.publicnode.com`) for live runs.

## Production cloud (D51)

Google Cloud: GKE, Cloud KMS (executor keys), Cloud Storage (snapshots). Credits: the Web3 program's Start tier ($2k) once there's a working MVP. No grant applications (D85).

Tentative regions **(verify)**: central + Base `us-east4`; BNB `asia-northeast1` or `asia-southeast1`; MegaETH `us-east4` until its rotating sequencer is live.

## Open questions

1. ~~Production cloud and credits~~ → **decided (D51): Google Cloud.**
2. **Regions:** Base and BNB sequencer/builder locations **(verify)**; MegaETH's planned rotating sequencer.
3. **Production cost** estimate once the cloud is chosen.
