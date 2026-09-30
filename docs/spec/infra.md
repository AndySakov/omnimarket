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

Unit tests · contract fork tests (Anvil) · Protobuf compatibility checks · nightly deterministic-simulation fuzz runs (D49).

CI runs on GitHub-hosted runners (`.github/workflows/ci.yml`, D76). CI and the local commit gate run the same `scripts/verify.sh`, so a commit that passes locally passes the same checks in CI. Locally, a tracked git pre-commit hook (`.githooks/`, installed by `scripts/setup.sh`) gates every commit, and a Claude Code `PreToolUse` hook refuses agent commits with `--no-verify`. CI is the gate nothing can skip.

Protobuf checks (`scripts/proto-check.sh`) run `buf lint` and `buf breaking` against `main`; CI fetches `main` for the comparison. `scripts/buf` pins buf's version and checksum and downloads it once into `.tools/`, so local runs and CI use the same binary with nothing installed globally.

CI also runs a Kafka service container (the same `apache/kafka` image as the local stack) for the input-log integration test, `cargo test -p sim --test kafka -- --ignored`. Locally it runs against `scripts/stack up`.

**Review gate (D81).** Branch protection on `main` requires two status checks on a PR's head commit, for admins too: `verify` (the CI job) and `watchdog/review`. A separate watchdog agent session posts `watchdog/review` as a commit status (`pending` while it reviews, then `success` or `failure`) with its findings as a PR comment. A new push needs a new review. The protocol agents follow is in `CLAUDE.md`.

## Production cloud (D51)

Google Cloud: GKE, Cloud KMS (executor keys), Cloud Storage (snapshots). Credits: Start tier ($2k) at MVP → blockchain foundation grant (BNB Chain Builder Grant first) → Web3 program Scale tier, up to $200k over 2 years.

Tentative regions **(verify)**: central + Base `us-east4`; BNB `asia-northeast1` or `asia-southeast1`; MegaETH `us-east4` until its rotating sequencer is live.

## Open questions

1. ~~Production cloud and credits~~ → **decided (D51): Google Cloud.**
2. **Regions:** Base and BNB sequencer/builder locations **(verify)**; MegaETH's planned rotating sequencer.
3. **Production cost** estimate once the cloud is chosen.
