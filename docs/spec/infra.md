# Infrastructure

**Status:** Draft. Decisions: D17, D43, D44, D50, D51.

## Environments (D50)

| Env | Where | Cost |
|---|---|---|
| Dev | k3d + docker compose, Anvil forks | Free |
| CI | GitHub Actions on Blacksmith runners | 3,000 free min/month, then ~$0.004/min |
| Staging | Oracle Cloud Always Free ARM, k3s, free RPC tiers | Free |
| Prod | Per-chain regional clusters + central region | Paid, offset by credits (D51) |

## Production layout

| Where | Runs |
|---|---|
| Per-chain region (nearest sequencer/builders) | Engine + standby, execution, local Kafka, Base node (Base only) |
| Central region | Postgres, ClickHouse, API/WebSocket gateway, candle service, history job, mirrored Kafka topics |

No hot-path call crosses regions.

## Data stores (self-hosted on Kubernetes)

CloudNativePG (Postgres) · Strimzi (Kafka) · Altinity operator (ClickHouse) · MinIO or cloud object storage (snapshots).

## Tooling

Terraform · Helm · Argo CD · Prometheus · Grafana · Loki · Tempo · SOPS (staging) · cloud KMS for executor keys (prod).

## CI pipeline

Unit tests · contract fork tests (Anvil) · Protobuf compatibility checks · nightly deterministic-simulation fuzz runs (D49).

## Production cloud (D51)

Google Cloud: GKE, Cloud KMS (executor keys), Cloud Storage (snapshots). Credits: Start tier ($2k) at MVP → blockchain foundation grant (BNB Chain Builder Grant first) → Web3 program Scale tier, up to $200k over 2 years.

Tentative regions **(verify)**: central + Base `us-east4`; BNB `asia-northeast1` or `asia-southeast1`; MegaETH `us-east4` until its rotating sequencer is live.

## Open questions

1. ~~Production cloud and credits~~ → **decided (D51): Google Cloud.**
2. **Regions:** Base and BNB sequencer/builder locations **(verify)**; MegaETH's planned rotating sequencer.
3. **Production cost** estimate once the cloud is chosen.
