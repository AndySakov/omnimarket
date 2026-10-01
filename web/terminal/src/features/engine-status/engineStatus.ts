// What the status bar says about the engine (#71). Pure: the bar and the "How it works" view both
// render from `presentEngineStatus`, so the two never disagree.

import { EngineStatus_Mode, type EngineStatus } from '../../api/generated/omnimarket/api/v1/market_pb'
import type { DataSourceKind } from '../../api/source'
import type { DataState } from '../../api/stream/dataState'

/**
 * How long the head may stand still before the bar calls itself stale. Base makes a block every
 * 2 seconds, so 10 seconds is five missed blocks: the engine has stopped, even if the connection
 * (and its heartbeats) hasn't.
 */
export const HEAD_STALE_AFTER_MS = 10_000

const CHAIN_NAMES: Record<string, string> = { '8453': 'Base' }

export type EngineModeLabel = 'Live' | 'Replay' | 'Fixtures'

export type PresentedVenue = { venue: string; known: string; active: string }

export type PresentedEngineStatus = {
  chain: string
  block: string
  /** "1 block · 2.3s" */
  lag: string
  /** Pools the engine follows, over every venue. */
  pools: string
  venues: PresentedVenue[]
  /** "5,117 / 5,120" */
  shadowChecks: string
  shadowChecksDetail: string
  mode: EngineModeLabel
  coreInstance: string
  recording: string
  uptime: string
  headHash: string
}

const integer = new Intl.NumberFormat('en-US')

function count(value: bigint): string {
  return integer.format(value)
}

/** Lag in ms to seconds with one decimal place. Display only; never used in arithmetic. */
function seconds(ms: bigint): string {
  return `${(Number(ms) / 1000).toFixed(1)}s`
}

export function formatUptime(ms: bigint): string {
  const totalMinutes = ms / 60_000n
  const days = totalMinutes / 1440n
  const hours = (totalMinutes % 1440n) / 60n
  const minutes = totalMinutes % 60n
  if (days > 0n) return `${days}d ${hours}h`
  if (hours > 0n) return `${hours}h ${minutes}m`
  if (minutes > 0n) return `${minutes}m`
  return `${ms / 1000n}s`
}

/** Fixture builds always say Fixtures, whatever the fixture's own mode, as the header does. */
export function engineModeLabel(source: DataSourceKind, mode: EngineStatus_Mode): EngineModeLabel {
  if (source === 'fixtures') return 'Fixtures'
  return mode === EngineStatus_Mode.REPLAY ? 'Replay' : 'Live'
}

export function presentEngineStatus(status: EngineStatus, source: DataSourceKind): PresentedEngineStatus {
  const known = status.pools.reduce((sum, venue) => sum + venue.known, 0n)
  const passed = status.shadowChecks - status.shadowCheckMismatches
  const blocks = status.lagBlocks === 1n ? 'block' : 'blocks'
  const hash = status.headBlockHash
  return {
    chain: CHAIN_NAMES[status.chainId.toString()] ?? `Chain ${status.chainId}`,
    block: count(status.headBlockNumber),
    lag: `${count(status.lagBlocks)} ${blocks} · ${seconds(status.lagMs)}`,
    pools: count(known),
    venues: status.pools.map((venue) => ({ venue: venue.venue, known: count(venue.known), active: count(venue.active) })),
    shadowChecks: `${count(passed)} / ${count(status.shadowChecks)}`,
    shadowChecksDetail:
      status.shadowChecks === 0n
        ? 'No shadow checks run yet.'
        : `${count(passed)} of ${count(status.shadowChecks)} agreed with the chain; ${count(status.shadowCheckMismatches)} disagreed.`,
    mode: engineModeLabel(source, status.mode),
    coreInstance: status.coreInstance || 'unknown',
    recording: status.recording ? 'on' : 'off',
    uptime: formatUptime(status.uptimeMs),
    headHash: hash.length > 14 ? `${hash.slice(0, 8)}…${hash.slice(-4)}` : hash,
  }
}

/**
 * The bar's own data state: the stream's, except that a head that hasn't moved for
 * `HEAD_STALE_AFTER_MS` is stale too. `headSeenAtMs` is when this client last saw the head change.
 */
export function engineStatusState(
  streamState: DataState,
  headSeenAtMs: number | undefined,
  nowMs: number,
  staleAfterMs: number = HEAD_STALE_AFTER_MS,
): DataState {
  if (streamState !== 'live' || headSeenAtMs === undefined) return streamState
  return nowMs - headSeenAtMs > staleAfterMs ? 'stale' : 'live'
}
