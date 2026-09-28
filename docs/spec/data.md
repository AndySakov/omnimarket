# Data

**Status:** Draft. Decision: D41. Columns are written in build mode; this doc fixes where data lives, its keys, and retention.

## Stores

| Store | Holds | Hot path waits on it? |
|---|---|---|
| Postgres | Users, wallets, follows, orders + firings, nonce ledger, positions, token metadata, trailing highs | No: engine/execution cache in memory, write in background |
| ClickHouse | Swaps, pool events, candles, backfill, routing logs, execution outcomes | No |
| Kafka | All engine and execution output | No (publish is async) |
| Object storage | Engine snapshots | No |

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

Protobuf; schemas versioned in the repo and checked for compatibility in CI.

## Snapshots

Versioned, compressed binary per chain every ~30s (D40); last 10 kept. MinIO in dev/staging.

## Retention

Candles forever. Raw swaps from the 30-day backfill onward; downsample past 90 days if needed.
