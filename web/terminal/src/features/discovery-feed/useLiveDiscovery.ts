// The Discover table's live rows (#65): the `discovery` topic, selected and sorted by the trader's
// controls, held still while the pointer or focus is in the table, with the motion each row shows
// (a highlight as it inserts, a flash when its price moves).

import { useEffect, useMemo, useRef, useState } from 'react'
import type { DiscoveryRow } from '../../api/generated/omnimarket/api/v1/market_pb'
import type { DataState } from '../../api/stream/dataState'
import { useDiscoveryStream, useStreamState } from '../../api/stream/hooks'
import type { DiscoveryToken } from '../../domains/market/token'
import {
  holdOrder,
  insertedRows,
  priceMoves,
  selectRows,
  type DiscoveryFilters,
  type DiscoverySort,
  type DiscoveryView,
  type PriceMove,
} from './discoveryView'
import { toDiscoveryToken } from './liveRows'

/** How long an inserted row stays highlighted, and a price flash lasts. Matches discovery.css. */
export const INSERT_HIGHLIGHT_MS = 2_000
export const PRICE_FLASH_MS = 900
/** Prices kept per row for its sparkline. */
const SPARKLINE_POINTS = 30

export type LiveDiscoveryOptions = {
  view: DiscoveryView
  sort: DiscoverySort
  filters: DiscoveryFilters
  /** The pointer or focus is in the table: hold the order still. */
  held: boolean
}

export type LiveDiscovery = {
  tokens: DiscoveryToken[]
  state: DataState
  /** Rows the feed has that wait for the hold to end. */
  waiting: number
  /** True once the feed's snapshot has arrived. */
  loaded: boolean
}

type Motion = {
  moves: Record<string, { direction: PriceMove; seq: number }>
  inserted: Record<string, true>
  prices: Record<string, string[]>
}

const noMotion: Motion = { moves: {}, inserted: {}, prices: {} }

export function useLiveDiscovery({ view, sort, filters, held }: LiveDiscoveryOptions): LiveDiscovery {
  const { data, state } = useDiscoveryStream()
  const nowMs = useStreamState((s) => s.serverTimeMs)
  const rows = data?.rows

  const sorted = useMemo(() => (rows ? selectRows(rows, view, filters, sort, nowMs) : []), [rows, view, filters, sort, nowMs])

  // The order on screen. Outside a hold it follows the sort; during one it stays as it was. (Set
  // during render, React's pattern for state derived from props; it settles in one pass.)
  const [shown, setShown] = useState<string[]>(sorted)
  if (!held && !sameKeys(shown, sorted)) setShown(sorted)
  // The rows as they were when the hold began, for rows that leave the feed during it.
  const [rowsAtHold, setRowsAtHold] = useState<Record<string, DiscoveryRow> | undefined>(undefined)
  if (held && rowsAtHold === undefined && rows) setRowsAtHold(rows)
  if (!held && rowsAtHold !== undefined) setRowsAtHold(undefined)

  const motion = useRowMotion(rows)

  const layout = held ? holdOrder(shown, sorted) : { order: sorted, waiting: [], departed: [] }
  const departed = new Set(layout.departed)
  const tokens: DiscoveryToken[] = []
  for (const key of layout.order) {
    const row = rows?.[key] ?? rowsAtHold?.[key]
    if (!row) continue
    tokens.push(
      toDiscoveryToken(
        row,
        nowMs,
        { priceMove: motion.moves[key], inserted: key in motion.inserted, departed: departed.has(key) },
        motion.prices[key],
      ),
    )
  }

  return { tokens, state, waiting: layout.waiting.length, loaded: rows !== undefined }
}

/**
 * Compares each version of the feed with the one before: which prices moved and which rows are new.
 * The first snapshot marks nothing new. Marks clear after their animation has run, so a row that
 * later moves in the table doesn't replay it.
 */
function useRowMotion(rows: Record<string, DiscoveryRow> | undefined): Motion {
  const [motion, setMotion] = useState<Motion>(noMotion)
  const previous = useRef<Record<string, DiscoveryRow> | undefined>(undefined)
  const seq = useRef(0)
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>())

  useEffect(() => {
    const pending = timers.current
    return () => {
      for (const timer of pending) clearTimeout(timer)
      pending.clear()
    }
  }, [])

  useEffect(() => {
    const before = previous.current
    previous.current = rows
    if (!rows) return
    if (!before) {
      setMotion((m) => ({ ...m, prices: recordPrices(m.prices, rows, Object.keys(rows)) }))
      return
    }
    const moved = priceMoves(before, rows)
    const added = insertedRows(before, rows)
    const changed = [...Object.keys(moved), ...added]
    if (changed.length === 0) return

    seq.current += 1
    const batch = seq.current
    setMotion((m) => {
      const moves = { ...m.moves }
      for (const [key, direction] of Object.entries(moved)) moves[key] = { direction, seq: batch }
      const inserted = { ...m.inserted }
      for (const key of added) inserted[key] = true
      return { moves, inserted, prices: recordPrices(m.prices, rows, changed) }
    })

    const later = (ms: number, clear: (m: Motion) => Motion) => {
      const timer = setTimeout(() => {
        timers.current.delete(timer)
        setMotion(clear)
      }, ms)
      timers.current.add(timer)
    }
    if (Object.keys(moved).length > 0) {
      later(PRICE_FLASH_MS, (m) => {
        const moves = { ...m.moves }
        // Only this batch's flashes: a later move on the same row keeps its own.
        for (const key of Object.keys(moved)) if (moves[key]?.seq === batch) delete moves[key]
        return { ...m, moves }
      })
    }
    if (added.length > 0) {
      later(INSERT_HIGHLIGHT_MS, (m) => {
        const inserted = { ...m.inserted }
        for (const key of added) delete inserted[key]
        return { ...m, inserted }
      })
    }
  }, [rows])

  return motion
}

function recordPrices(prices: Motion['prices'], rows: Record<string, DiscoveryRow>, keys: string[]): Motion['prices'] {
  const next = { ...prices }
  for (const key of keys) {
    const price = rows[key]?.displayPriceUsd
    if (!price) continue
    next[key] = [...(next[key] ?? []), price].slice(-SPARKLINE_POINTS)
  }
  return next
}

function sameKeys(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((key, i) => key === b[i])
}
