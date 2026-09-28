# Data

**Status:** Draft. Decisions: D41, D52–D55. Columns are written in build mode; this doc fixes where data lives, its keys, and retention.

## Stores

| Store | Holds | Hot path waits on it? |
|---|---|---|
| Postgres | Users, wallets, follows, orders + firings, nonce ledger, positions, token metadata, trailing highs | No: engine/execution cache in memory, write in background |
| ClickHouse | Swaps, pool events, candles, backfill, routing logs, execution outcomes | No |
| Kafka | All engine and execution output | No (publish is async) |
| Object storage | Engine snapshots; archived input log (D54); write-once audit log copy (D55) | No |

## ClickHouse and reorgs

- Key: (chain, block hash, log index). Re-inserts are harmless → restartable backfills (D15).
- Status per row: provisional → confirmed → final, or removed. Versioned inserts; `ReplacingMergeTree` keeps the latest.
- Candles: built live from `swaps.<chain>` by the candle service; rebuilt for windows touched by corrections. 1s candles are also a recovery input (D39).

## Kafka topics

| Topic | Key | Retention |
|---|---|---|
| `pool-updates.<chain>` | pool | ≥ 24h |
| `swaps.<chain>` | pool | 7 days |
| `prices.<chain>` | token | 24h |
| `corrections.<chain>` | block | 7 days |
| `executions.<chain>` | wallet | 30 days |
| `inputs.<chain>` (flight recorder, D54) | source | 30 days hot, then Cloud Storage for 1 year |

Protobuf; schemas versioned in the repo and checked for compatibility in CI.

## Observability data (D53–D55)

- **Lineage edges** (cause ID → effect ID, with type and timestamp) in ClickHouse; money paths 100%.
- **Decision records** (alternatives, scores, reasons, config version, brake state) in ClickHouse, 1 year.
- **Audit log:** hash-chained, append-only, ClickHouse + write-once Cloud Storage; daily root anchored on Base. Kept forever.

## Privacy (D52)

Telemetry uses pseudonymous user IDs. Per-user records are encrypted with a per-user key; deleting the key makes them unreadable (crypto-shredding) while the audit log's hash chain stays verifiable.

## Snapshots

Versioned, compressed binary per chain every ~30s (D40); last 10 kept. MinIO in dev/staging.

## Retention

Candles forever. Raw swaps from the 30-day backfill onward; downsample past 90 days if needed.
