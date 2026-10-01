import { discoveryTokens } from '../../mocks/discoveryFixtures'
import { tokenWorkspace } from '../../mocks/tokenWorkspaceFixtures'
import type {
  TokenChartData,
  TokenChartInterval,
  TokenChartSnapshot,
  TokenChartState,
  TokenMarketSnapshot,
  TokenTradeRow,
  TokenWorkspaceFixture,
  TokenWorkspaceState,
  TokenWorkspaceStreamState,
} from '../../domains/market/tokenWorkspace'
import { nativeAssetForChain } from '../../domains/market/tokenWorkspace'

export type TokenWorkspaceSnapshot = {
  token: TokenWorkspaceFixture
  state: TokenWorkspaceState
  source: 'fixture'
  freshness: 'fresh' | 'stale'
  updatedLabel: string
}

const chartGroupSizes: Record<TokenChartInterval, number> = {
  '1m': 1,
  '5m': 1,
  '15m': 1,
  '1h': 4,
  '4h': 8,
  '1D': 16,
}

const streamTicks = [
  { price: '$0.96', priceChange: '+28.6%', amount: '18.4K', value: '$17.7K', wallet: '0x3a7e…9c21' },
  { price: '$0.98', priceChange: '+31.2%', amount: '9.6K', value: '$9.4K', wallet: '0x8b29…e812' },
  { price: '$0.94', priceChange: '+25.9%', amount: '4.8K', value: '$4.5K', wallet: '0xf11d…048a' },
] as const

export type TokenWorkspaceStreamOptions = {
  loadDelayMs?: number
  tickIntervalMs?: number
}

export type TokenWorkspaceStreamListener = (snapshot: TokenMarketSnapshot) => void

/**
 * Resolves the selected token into the complete view model consumed by the
 * workspace regions. The fixture source is deliberately isolated here so a
 * future market subscription can replace this function without changing the
 * header, chart, tabs, or trade panel components.
 */
export function getTokenWorkspaceSnapshot(
  tokenId?: string,
  state: TokenWorkspaceState = 'ready',
): TokenWorkspaceSnapshot {
  const token = resolveToken(tokenId)

  if (!token) {
    return {
      token: tokenWorkspace,
      state: state === 'ready' ? 'missing' : state,
      source: 'fixture',
      freshness: 'stale',
      updatedLabel: 'Unavailable',
    }
  }

  return {
    token,
    state,
    source: 'fixture',
    freshness: state === 'stale' ? 'stale' : 'fresh',
    updatedLabel: state === 'stale' ? 'Older than 2m' : 'Updated 12s ago',
  }
}

export function getTokenChartSnapshot(
  token: TokenWorkspaceFixture,
  interval: TokenChartInterval,
  state: TokenChartState = 'ready',
): TokenChartSnapshot {
  if (state === 'empty' || state === 'error') {
    return {
      interval,
      data: null,
      state,
      source: 'fixture',
      updatedLabel: state === 'empty' ? 'No candles in snapshot' : 'Snapshot unavailable',
    }
  }

  return {
    interval,
    data: aggregateChartData(token.chart, chartGroupSizes[interval]),
    state,
    source: 'fixture',
    updatedLabel: state === 'stale' ? 'Older than 2m' : 'Updated 12s ago',
  }
}

export function getTokenMarketSnapshot(
  token: TokenWorkspaceFixture,
  interval: TokenChartInterval,
  sequence = 0,
  state: Exclude<TokenWorkspaceStreamState, 'loading'> = 'ready',
): TokenMarketSnapshot {
  const tick = streamTicks[sequence % streamTicks.length]
  const streamedToken: TokenWorkspaceFixture = {
    ...token,
    price: tick.price,
    priceChange: tick.priceChange,
  }
  const chart = getTokenChartSnapshot(streamedToken, interval, state === 'stale' ? 'stale' : 'ready').data
  const trades = sequence === 0 ? token.trades : [createStreamTrade(streamedToken, sequence, tick), ...token.trades].slice(0, 4)

  return {
    interval,
    token: streamedToken,
    chart: chart ?? token.chart,
    trades,
    sequence,
    state,
    updatedLabel: state === 'stale' ? 'Older than 2m' : sequence === 0 ? 'Updated 12s ago' : 'Updated just now',
  }
}

export function createTokenWorkspaceStream(
  token: TokenWorkspaceFixture,
  interval: TokenChartInterval,
  listener: TokenWorkspaceStreamListener,
  options: TokenWorkspaceStreamOptions = {},
): () => void {
  const loadDelayMs = options.loadDelayMs ?? 320
  const tickIntervalMs = options.tickIntervalMs ?? 6_000
  let sequence = 0
  let tickTimer: number | undefined
  let disposed = false

  const loadTimer = window.setTimeout(() => {
    if (disposed) return
    listener(getTokenMarketSnapshot(token, interval, sequence))
    tickTimer = window.setInterval(() => {
      sequence += 1
      const state = sequence % 3 === 2 ? 'stale' : 'ready'
      listener(getTokenMarketSnapshot(token, interval, sequence, state))
    }, tickIntervalMs)
  }, loadDelayMs)

  return () => {
    disposed = true
    window.clearTimeout(loadTimer)
    if (tickTimer !== undefined) window.clearInterval(tickTimer)
  }
}

function createStreamTrade(
  token: TokenWorkspaceFixture,
  sequence: number,
  tick: (typeof streamTicks)[number],
): TokenTradeRow {
  const side = sequence % 2 === 0 ? 'Sell' : 'Buy'
  return {
    id: `stream-trade-${token.id}-${sequence}`,
    side,
    amount: `${tick.amount} ${token.symbol}`,
    value: tick.value,
    wallet: tick.wallet,
    time: 'just now',
    marketCap: token.fdv,
    gas: '$0.35',
    trader: tick.wallet,
    tracking: sequence % 2 === 0 ? 'Untracked' : 'Tracked',
  }
}

function aggregateChartData(chart: TokenChartData, groupSize: number): TokenChartData {
  const candles: TokenChartData['candles'] = []
  const volume: TokenChartData['volume'] = []

  for (let index = 0; index < chart.candles.length; index += groupSize) {
    const candleGroup = chart.candles.slice(index, index + groupSize)
    const volumeGroup = chart.volume.slice(index, index + groupSize)
    const first = candleGroup[0]
    const last = candleGroup[candleGroup.length - 1]
    if (!first || !last) continue

    candles.push({
      time: first.time,
      open: first.open,
      high: Math.max(...candleGroup.map((candle) => candle.high)),
      low: Math.min(...candleGroup.map((candle) => candle.low)),
      close: last.close,
    })
    volume.push({
      time: first.time,
      value: volumeGroup.reduce((total, point) => total + point.value, 0),
      tone: last.close >= first.open ? 'up' : 'down',
    })
  }

  return { candles, volume }
}

function resolveToken(tokenId?: string): TokenWorkspaceFixture | undefined {
  if (!tokenId || tokenId === tokenWorkspace.id) return tokenWorkspace

  const source = discoveryTokens.find((candidate) => candidate.id === tokenId)
  if (!source) return undefined

  const asset = nativeAssetForChain(source.chain)
  return {
    ...tokenWorkspace,
    ...source,
    address: `0x${source.id.replaceAll('-', '').slice(0, 4)}…${source.id.replaceAll('-', '').slice(-4)}`,
    price: source.marketCap,
    priceChange: source.marketCapChange,
    fdv: source.marketCap,
    volume24h: source.volume,
    pair: `${source.symbol} / ${asset}`,
    safetyNote: source.safety.state === 'passed'
      ? 'Sell simulation passed in the latest fixture snapshot.'
      : 'This fixture needs review before a trade can be prepared.',
  }
}
