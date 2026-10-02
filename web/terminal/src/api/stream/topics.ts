// The WebSocket topics (proto/omnimarket/api/v1/stream.proto). v0 is Base-only, so topics carry no
// chain. Each topic's state lives in the stream store under its topic string.

/** The candle intervals, as topics and `GET /v1/candles` name them (CandleInterval in market.proto). */
export const CANDLE_INTERVAL_LABELS = ['1s', '1m', '5m', '15m', '1h', '4h', '1d'] as const

export type CandleIntervalLabel = (typeof CANDLE_INTERVAL_LABELS)[number]

export const STATUS_TOPIC = 'status'
export const DISCOVERY_TOPIC = 'discovery'
export const ACCOUNT_TOPIC = 'account'

export function tokenTopic(address: string): string {
  return `token:${address.toLowerCase()}`
}

export function tradesTopic(address: string): string {
  return `trades:${address.toLowerCase()}`
}

export function candlesTopic(address: string, interval: CandleIntervalLabel): string {
  return `candles:${address.toLowerCase()}:${interval}`
}

export type TopicKind = 'status' | 'discovery' | 'account' | 'token' | 'trades' | 'candles'

/** The kind of a topic string, or undefined for one the contract doesn't define. */
export function topicKind(topic: string): TopicKind | undefined {
  if (topic === STATUS_TOPIC) return 'status'
  if (topic === DISCOVERY_TOPIC) return 'discovery'
  if (topic === ACCOUNT_TOPIC) return 'account'
  const [prefix, address, interval, ...rest] = topic.split(':')
  if (rest.length > 0 || !address || !/^0x[0-9a-f]{40}$/.test(address)) return undefined
  if (prefix === 'token' && interval === undefined) return 'token'
  if (prefix === 'trades' && interval === undefined) return 'trades'
  if (prefix === 'candles' && (CANDLE_INTERVAL_LABELS as readonly string[]).includes(interval ?? '')) return 'candles'
  return undefined
}
