// What the Discover table shows, and in what order (#65). Pure functions of the discovery feed,
// so ordering is unit-tested apart from React.
//
// Two rules keep the table readable while it updates:
// - Sorting is total: ties fall back to the newer pool, then the token address, so two updates
//   with the same values always give the same order.
// - While the trader's pointer or focus is in the table, the order holds still (`holdOrder`): rows
//   update in place, new rows wait, and rows that leave stay dimmed until the hold ends.

import type Decimal from 'decimal.js'
import { DiscoveryList, SafetyVerdict, type DiscoveryRow } from '../../api/generated/omnimarket/api/v1/market_pb'
import { parseOptionalDecimal } from '../../shared/decimal'

export type DiscoveryView = 'New' | 'Trending'

/** "feed" is the list's own order: New by pool creation, newest first; Trending by its rank. */
export type SortKey = 'feed' | 'age' | 'volume' | 'txns' | 'liquidity' | 'marketCap'
export type SortDirection = 'asc' | 'desc'
export type DiscoverySort = { key: SortKey; direction: SortDirection }

export type DiscoveryFilters = {
  /** Only rows on this chain. */
  chainId?: bigint
  /** Minimum ±2% depth in USD (the table's "Liquidity"). */
  minLiquidityUsd?: Decimal
  /** Maximum pool age. Needs the server clock; ignored until the first heartbeat. */
  maxAgeMs?: bigint
  /** Only rows whose safety check passed. */
  safetyPassedOnly?: boolean
}

export const sortLabels: Record<SortKey, string> = {
  feed: 'Feed order',
  age: 'Age',
  volume: 'Volume',
  txns: 'Txns',
  liquidity: 'Liquidity',
  marketCap: 'Market cap',
}

const listOf: Record<DiscoveryView, DiscoveryList> = {
  New: DiscoveryList.NEW,
  Trending: DiscoveryList.TRENDING,
}

/** The row's key: its token address, lowercase (as the stream store keys it). */
export function rowKey(row: DiscoveryRow): string {
  return (row.token?.address ?? '').toLowerCase()
}

/** The keys of the rows in `view` that pass `filters`, sorted. */
export function selectRows(
  rows: Record<string, DiscoveryRow>,
  view: DiscoveryView,
  filters: DiscoveryFilters,
  sort: DiscoverySort,
  nowMs: bigint | undefined,
): string[] {
  const list = listOf[view]
  const kept = Object.values(rows).filter((row) => row.lists.includes(list) && passesFilters(row, filters, nowMs))
  return kept.sort((a, b) => compareRows(a, b, view, sort)).map(rowKey)
}

function passesFilters(row: DiscoveryRow, filters: DiscoveryFilters, nowMs: bigint | undefined): boolean {
  if (filters.chainId !== undefined && row.token?.chainId !== filters.chainId) return false
  if (filters.minLiquidityUsd) {
    const depth = parseOptionalDecimal(row.depthUsd)
    if (!depth || depth.lt(filters.minLiquidityUsd)) return false
  }
  if (filters.maxAgeMs !== undefined && nowMs !== undefined && nowMs - row.poolCreatedAtMs > filters.maxAgeMs) {
    return false
  }
  if (filters.safetyPassedOnly && row.safety?.verdict !== SafetyVerdict.PASSED) return false
  return true
}

export function compareRows(a: DiscoveryRow, b: DiscoveryRow, view: DiscoveryView, sort: DiscoverySort): number {
  const primary = comparePrimary(a, b, view, sort)
  if (primary !== 0) return primary
  // Ties: the newer pool first, then by address, so the order never depends on arrival.
  const byCreation = compareBigint(b.poolCreatedBlock, a.poolCreatedBlock)
  if (byCreation !== 0) return byCreation
  const ka = rowKey(a)
  const kb = rowKey(b)
  return ka < kb ? -1 : ka > kb ? 1 : 0
}

function comparePrimary(a: DiscoveryRow, b: DiscoveryRow, view: DiscoveryView, sort: DiscoverySort): number {
  const flip = sort.direction === 'asc' ? 1 : -1
  switch (sort.key) {
    case 'feed':
      if (view === 'Trending') return flip * compareRank(a.rank, b.rank)
      // New: newest first when ascending, as age is.
      return flip * compareBigint(b.poolCreatedAtMs, a.poolCreatedAtMs)
    case 'age':
      // Ascending age is youngest first: the later creation time.
      return flip * compareBigint(b.poolCreatedAtMs, a.poolCreatedAtMs)
    case 'volume':
      return compareDecimals(a.stats5m?.volumeUsd, b.stats5m?.volumeUsd, flip)
    case 'txns':
      return flip * compareBigint(txns(a), txns(b))
    case 'liquidity':
      return compareDecimals(a.depthUsd, b.depthUsd, flip)
    case 'marketCap':
      return compareDecimals(a.marketCapUsd, b.marketCapUsd, flip)
  }
}

/** The 5m buys plus sells. */
export function txns(row: DiscoveryRow): bigint {
  return (row.stats5m?.buys ?? 0n) + (row.stats5m?.sells ?? 0n)
}

/** Rank 0 is unset: it sorts after every ranked row. */
function compareRank(a: number, b: number): number {
  if (a === b) return 0
  if (a === 0) return 1
  if (b === 0) return -1
  return a - b
}

/** An unset value sorts last whichever the direction, so blanks never lead the table. */
function compareDecimals(a: string | undefined, b: string | undefined, flip: number): number {
  const da = parseOptionalDecimal(a ?? '')
  const db = parseOptionalDecimal(b ?? '')
  if (!da && !db) return 0
  if (!da) return 1
  if (!db) return -1
  return flip * da.comparedTo(db)
}

function compareBigint(a: bigint, b: bigint): number {
  return a < b ? -1 : a > b ? 1 : 0
}

export type HeldOrder = {
  /** What the table shows. */
  order: string[]
  /** Rows the feed has that the table doesn't show yet. */
  waiting: string[]
  /** Rows the table shows that the feed no longer has. */
  departed: string[]
}

/**
 * The order to show while the table holds still: exactly what was shown, so no row moves under the
 * pointer. New rows wait (a count says so), and departed rows stay until the hold ends.
 */
export function holdOrder(shown: readonly string[], sorted: readonly string[]): HeldOrder {
  const inSorted = new Set(sorted)
  const inShown = new Set(shown)
  return {
    order: [...shown],
    waiting: sorted.filter((key) => !inShown.has(key)),
    departed: shown.filter((key) => !inSorted.has(key)),
  }
}

export type PriceMove = 'up' | 'down'

/** Rows whose display price moved between two versions of the feed. */
export function priceMoves(
  before: Record<string, DiscoveryRow>,
  after: Record<string, DiscoveryRow>,
): Record<string, PriceMove> {
  const moves: Record<string, PriceMove> = {}
  for (const [key, row] of Object.entries(after)) {
    const previous = before[key]
    if (!previous || previous === row || previous.displayPriceUsd === row.displayPriceUsd) continue
    const was = parseOptionalDecimal(previous.displayPriceUsd)
    const now = parseOptionalDecimal(row.displayPriceUsd)
    if (!was || !now || was.eq(now)) continue
    moves[key] = now.gt(was) ? 'up' : 'down'
  }
  return moves
}

/** Rows in `after` that weren't in `before`. */
export function insertedRows(before: Record<string, DiscoveryRow>, after: Record<string, DiscoveryRow>): string[] {
  return Object.keys(after).filter((key) => !(key in before))
}
