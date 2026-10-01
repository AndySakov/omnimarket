// The stream's normalised state and the pure functions that change it (#63). The manager feeds
// every server message through `applyServerMessage`; React reads slices with selectors, so a
// change to one topic's slice leaves every other slice's object identity alone and rerenders only
// the components that select it.
//
// Sequencing (stream.proto): each subscribed topic gets one snapshot, then deltas numbered by
// `seq`, one apart. A delta at or below the last applied seq is a duplicate and dropped; one more
// than one ahead is a gap: the topic's state is discarded and it is resubscribed.

import type { Message } from '@bufbuild/protobuf'
import type {
  Candle,
  DiscoveryRow,
  EngineStatus,
  TokenSnapshot,
  TokenTick,
  Trade,
} from '../generated/omnimarket/api/v1/market_pb'
import type { Account, Position } from '../generated/omnimarket/api/v1/trading_pb'
import type { Quote, TradeReceipt, TradeStatus } from '../generated/omnimarket/api/v1/trading_pb'
import type { Firing, Order } from '../generated/omnimarket/api/v1/automation_pb'
import type {
  Delta,
  ServerMessage,
  Snapshot,
  StreamError,
} from '../generated/omnimarket/api/v1/stream_pb'
import { StreamError_Code } from '../generated/omnimarket/api/v1/stream_pb'

export type ConnectionState =
  /** Not started, or stopped on purpose. */
  | 'idle'
  /** The first connection attempt. */
  | 'connecting'
  /** Connected, and heartbeats arriving. */
  | 'open'
  /** Connected, but three heartbeats missed: what's on screen may be old. */
  | 'stale'
  /** The connection dropped; retrying with backoff. */
  | 'reconnecting'
  /** Several attempts in a row failed; still retrying, at the longest backoff. */
  | 'unavailable'

export type TopicPhase =
  /** Subscribed; no snapshot yet. */
  | 'loading'
  /** Snapshot applied; deltas apply in seq order. */
  | 'live'
  /** Resubscribed after a reconnect: the old data stays until the new snapshot replaces it. */
  | 'resubscribing'
  /** The server said it can't serve the topic (StreamError). */
  | 'unavailable'

export type TopicState = {
  phase: TopicPhase
  /** The last applied seq; meaningful once live. */
  seq: bigint
  /** Why the topic is unavailable, from the server's StreamError. */
  error?: string
}

/** The discovery feed, rows keyed by token address, in the order they arrived. */
export type DiscoveryState = {
  order: string[]
  rows: Record<string, DiscoveryRow>
  blockNumber: bigint
}

/** The `account` topic: the snapshot, then upserts by each record's key. */
export type AccountState = {
  account?: Account
  /** Keyed by token address. */
  positions: Record<string, Position>
  /** Keyed by order ID; an OrderStatus delta replaces its order's `status`. */
  orders: Record<string, Order>
  /** Keyed by trade ID. */
  trades: Record<string, TradeStatus>
  receipts: Record<string, TradeReceipt>
  quotes: Record<string, Quote>
  firings: Record<string, Firing>
}

export type StreamState = {
  connection: ConnectionState
  /** The subscribed topics; a topic absent here isn't subscribed. */
  topics: Record<string, TopicState>
  /**
   * The server's clock, from the latest heartbeat (Unix ms). Ages on screen count from it rather
   * than the browser's clock, so a replay or the fixtures read as they did when recorded.
   */
  serverTimeMs?: bigint
  // Topic data. Each slice is keyed by the full topic string, so deltas land where their topic says.
  status?: EngineStatus
  discovery?: DiscoveryState
  account?: AccountState
  tokens: Record<string, TokenSnapshot>
  /** Newest first, as the snapshot sends them; new trades go on the front. */
  trades: Record<string, Trade[]>
  /** Oldest first, one per open time. */
  candles: Record<string, Candle[]>
}

/** How many trades a topic keeps; older ones fall off the end. */
export const MAX_TRADES_PER_TOPIC = 500

export function initialStreamState(): StreamState {
  return { connection: 'idle', topics: {}, tokens: {}, trades: {}, candles: {} }
}

export type ApplyResult = {
  state: StreamState
  /** Topics whose seq jumped: their state was discarded and the caller must resubscribe. */
  resubscribe: string[]
}

export function applyServerMessage(state: StreamState, message: ServerMessage): ApplyResult {
  switch (message.kind.case) {
    case 'snapshot':
      return { state: applySnapshot(state, message.kind.value), resubscribe: [] }
    case 'delta':
      return applyDelta(state, message.kind.value)
    case 'error':
      return { state: applyError(state, message.kind.value), resubscribe: [] }
    case 'heartbeat':
      return { state: { ...state, serverTimeMs: message.kind.value.serverTimeMs }, resubscribe: [] }
    case undefined:
      return { state, resubscribe: [] }
  }
}

/** Marks a topic subscribed and waiting for its snapshot. Its old data, if any, stays until then. */
export function markSubscribed(state: StreamState, topic: string): StreamState {
  return { ...state, topics: { ...state.topics, [topic]: { phase: 'loading', seq: 0n } } }
}

/** Forgets a topic: its subscription and its data. */
export function forgetTopic(state: StreamState, topic: string): StreamState {
  const topics = { ...state.topics }
  delete topics[topic]
  return clearTopicData({ ...state, topics }, topic)
}

/**
 * After a reconnect every topic waits for a fresh snapshot. Data stays on screen meanwhile; the
 * UI labels it reconnecting until the snapshot replaces it.
 */
export function markAllResubscribing(state: StreamState): StreamState {
  const topics: Record<string, TopicState> = {}
  for (const topic of Object.keys(state.topics)) topics[topic] = { phase: 'resubscribing', seq: 0n }
  return { ...state, topics }
}

function applySnapshot(state: StreamState, snapshot: Snapshot): StreamState {
  const { topic, seq, payload } = snapshot
  // A snapshot for a topic we've since left is late; ignore it.
  if (!(topic in state.topics)) return state
  const topics = { ...state.topics, [topic]: { phase: 'live' as const, seq } }

  switch (payload.case) {
    case 'status':
      return { ...state, topics, status: payload.value }
    case 'discovery': {
      const rows: Record<string, DiscoveryRow> = {}
      const order: string[] = []
      for (const row of payload.value.rows) {
        const key = rowKey(row)
        if (!(key in rows)) order.push(key)
        rows[key] = row
      }
      return { ...state, topics, discovery: { rows, order, blockNumber: payload.value.blockNumber } }
    }
    case 'token':
      return { ...state, topics, tokens: { ...state.tokens, [topic]: payload.value } }
    case 'trades':
      return {
        ...state,
        topics,
        trades: { ...state.trades, [topic]: dedupeTrades(payload.value.trades).slice(0, MAX_TRADES_PER_TOPIC) },
      }
    case 'candles':
      return { ...state, topics, candles: { ...state.candles, [topic]: sortCandles(payload.value.candles) } }
    case 'account': {
      const snap = payload.value
      return {
        ...state,
        topics,
        account: {
          account: snap.account,
          positions: byKey(snap.positions, positionKey),
          orders: byKey(snap.orders, (order) => order.orderId),
          trades: byKey(snap.openTrades, (trade) => trade.tradeId),
          receipts: {},
          quotes: {},
          firings: {},
        },
      }
    }
    case undefined:
      return { ...state, topics }
  }
}

function applyDelta(state: StreamState, delta: Delta): ApplyResult {
  const { topic, seq, payload } = delta
  const current = state.topics[topic]
  // Not subscribed, or waiting for the snapshot: the snapshot will include this change.
  if (!current || current.phase !== 'live') return { state, resubscribe: [] }
  // Already applied: a duplicate or a late message.
  if (seq <= current.seq) return { state, resubscribe: [] }
  if (seq !== current.seq + 1n) {
    const topics = { ...state.topics, [topic]: { phase: 'loading' as const, seq: 0n } }
    return { state: clearTopicData({ ...state, topics }, topic), resubscribe: [topic] }
  }

  const next = { ...state, topics: { ...state.topics, [topic]: { phase: 'live' as const, seq } } }
  switch (payload.case) {
    case 'status':
      return done({ ...next, status: payload.value })
    case 'discoveryRow': {
      const row = payload.value
      const key = rowKey(row)
      const discovery = next.discovery ?? { rows: {}, order: [], blockNumber: 0n }
      return done({
        ...next,
        discovery: {
          rows: { ...discovery.rows, [key]: row },
          order: key in discovery.rows ? discovery.order : [...discovery.order, key],
          blockNumber: row.blockNumber > discovery.blockNumber ? row.blockNumber : discovery.blockNumber,
        },
      })
    }
    case 'discoveryRowRemoved': {
      const key = payload.value.token.toLowerCase()
      if (!next.discovery || !(key in next.discovery.rows)) return done(next)
      const rows = { ...next.discovery.rows }
      delete rows[key]
      return done({
        ...next,
        discovery: { ...next.discovery, rows, order: next.discovery.order.filter((k) => k !== key) },
      })
    }
    case 'tokenTick': {
      const snapshot = next.tokens[topic]
      if (!snapshot) return done(next)
      return done({ ...next, tokens: { ...next.tokens, [topic]: applyTick(snapshot, payload.value) } })
    }
    case 'trade': {
      const trades = next.trades[topic] ?? []
      const key = tradeKey(payload.value)
      if (trades.some((trade) => tradeKey(trade) === key)) return done(next)
      return done({
        ...next,
        trades: { ...next.trades, [topic]: [payload.value, ...trades].slice(0, MAX_TRADES_PER_TOPIC) },
      })
    }
    case 'candle':
      return done({
        ...next,
        candles: { ...next.candles, [topic]: upsertCandle(next.candles[topic] ?? [], payload.value) },
      })
    case 'account':
      return done(updateAccount(next, (account) => ({ ...account, account: payload.value })))
    case 'position': {
      const position = payload.value
      return done(updateAccount(next, (account) => ({
        ...account,
        positions: { ...account.positions, [positionKey(position)]: position },
      })))
    }
    case 'quote': {
      const quote = payload.value
      return done(updateAccount(next, (account) => ({ ...account, quotes: { ...account.quotes, [quote.quoteId]: quote } })))
    }
    case 'tradeStatus': {
      const status = payload.value
      return done(updateAccount(next, (account) => ({ ...account, trades: { ...account.trades, [status.tradeId]: status } })))
    }
    case 'tradeReceipt': {
      const receipt = payload.value
      return done(updateAccount(next, (account) => ({
        ...account,
        receipts: { ...account.receipts, [receipt.tradeId]: receipt },
      })))
    }
    case 'order': {
      const order = payload.value
      return done(updateAccount(next, (account) => ({ ...account, orders: { ...account.orders, [order.orderId]: order } })))
    }
    case 'orderStatus': {
      const status = payload.value
      return done(updateAccount(next, (account) => {
        const order = account.orders[status.orderId]
        // A status for an order we haven't seen: the order itself arrives with its own delta.
        if (!order) return account
        return { ...account, orders: { ...account.orders, [status.orderId]: { ...order, status } } }
      }))
    }
    case 'firing': {
      const firing = payload.value
      return done(updateAccount(next, (account) => ({ ...account, firings: { ...account.firings, [firing.firingId]: firing } })))
    }
    case undefined:
      return done(next)
  }
}

function applyError(state: StreamState, error: StreamError): StreamState {
  // Errors about the connection as a whole (rate limits) end in a close, which the manager handles.
  if (!error.topic || !(error.topic in state.topics)) return state
  switch (error.code) {
    case StreamError_Code.UNKNOWN_TOPIC:
    case StreamError_Code.UNAUTHENTICATED:
    case StreamError_Code.UNAVAILABLE:
    case StreamError_Code.INTERNAL:
    case StreamError_Code.UNSPECIFIED: {
      const topics = {
        ...state.topics,
        [error.topic]: { phase: 'unavailable' as const, seq: 0n, error: error.message || 'unavailable' },
      }
      return clearTopicData({ ...state, topics }, error.topic)
    }
    case StreamError_Code.RATE_LIMITED:
      return state
  }
}

/** A tick moves the token's price fields; everything else stays as the snapshot had it. */
function applyTick(snapshot: TokenSnapshot, tick: TokenTick): TokenSnapshot {
  return {
    ...snapshot,
    lineage: tick.lineage,
    displayPriceUsd: tick.displayPriceUsd,
    displayPriceQuote: tick.displayPriceQuote,
    marketCapUsd: tick.marketCapUsd,
    depthUsd: tick.depthUsd,
    thin: tick.thin,
    blockNumber: tick.blockNumber,
    blockTimeMs: tick.blockTimeMs,
  }
}

function clearTopicData(state: StreamState, topic: string): StreamState {
  if (topic === 'status') return { ...state, status: undefined }
  if (topic === 'discovery') return { ...state, discovery: undefined }
  if (topic === 'account') return { ...state, account: undefined }
  return {
    ...state,
    tokens: without(state.tokens, topic),
    trades: without(state.trades, topic),
    candles: without(state.candles, topic),
  }
}

function updateAccount(state: StreamState, update: (account: AccountState) => AccountState): StreamState {
  const account = state.account ?? {
    positions: {},
    orders: {},
    trades: {},
    receipts: {},
    quotes: {},
    firings: {},
  }
  return { ...state, account: update(account) }
}

function upsertCandle(candles: Candle[], candle: Candle): Candle[] {
  const index = candles.findIndex((c) => c.openTimeMs === candle.openTimeMs)
  if (index >= 0) {
    const next = candles.slice()
    next[index] = candle
    return next
  }
  return sortCandles([...candles, candle])
}

function sortCandles(candles: Candle[]): Candle[] {
  return candles.slice().sort((a, b) => (a.openTimeMs < b.openTimeMs ? -1 : a.openTimeMs > b.openTimeMs ? 1 : 0))
}

function dedupeTrades(trades: Trade[]): Trade[] {
  const seen = new Set<string>()
  return trades.filter((trade) => {
    const key = tradeKey(trade)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

/** A trade's natural key (market.proto): its block hash and log index. */
export function tradeKey(trade: Trade): string {
  return `${trade.blockHash}:${trade.logIndex}`
}

function rowKey(row: DiscoveryRow): string {
  return (row.token?.address ?? '').toLowerCase()
}

function positionKey(position: Position): string {
  return (position.token?.address ?? '').toLowerCase()
}

function byKey<T extends Message>(records: T[], key: (record: T) => string): Record<string, T> {
  const out: Record<string, T> = {}
  for (const record of records) out[key(record)] = record
  return out
}

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  if (!(key in record)) return record
  const next = { ...record }
  delete next[key]
  return next
}

function done(state: StreamState): ApplyResult {
  return { state, resubscribe: [] }
}
