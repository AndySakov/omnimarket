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
  TokenWorkspaceStreamStatus,
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

// The live discovery fixture identifies NovaSet by its canonical contract address rather than
// the design fixture's short id. Keep that bridge local until the token-detail REST route is wired.
const canonicalWorkspaceAddresses: Record<string, string> = {
  '0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567': tokenWorkspace.id,
}

const chartGroupSizes: Record<TokenChartInterval, number> = {
  '1s': 1,
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
  disconnectAfterMs?: number
  reconnectDelayMs?: number
  onStatus?: TokenWorkspaceStreamStatusListener
}

export type TokenWorkspaceStreamListener = (snapshot: TokenMarketSnapshot) => void
export type TokenWorkspaceStreamStatusListener = (status: TokenWorkspaceStreamStatus) => void

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

export function getTokenRouteAddress(tokenId: string): string {
  const token = resolveToken(tokenId)
  return token ? token.address.replace('…', '') : tokenId
}

export function resolveTokenIdByAddress(address: string): string | undefined {
  const normalisedAddress = address.toLowerCase().replace(/[^a-z0-9]/g, '')
  const canonicalId = Object.entries(canonicalWorkspaceAddresses).find(([knownAddress]) => knownAddress.replace(/[^a-z0-9]/g, '') === normalisedAddress)?.[1]
  if (canonicalId) return canonicalId
  const candidates = [tokenWorkspace, ...discoveryTokens.map((token) => resolveToken(token.id)).filter((token): token is TokenWorkspaceFixture => Boolean(token))]
  return candidates.find((token) => token.address.toLowerCase().replace(/[^a-z0-9]/g, '') === normalisedAddress)?.id
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
    priceEth: sequence % 2 === 0 ? '0.00036 ETH' : '0.00038 ETH',
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
  const reconnectDelayMs = options.reconnectDelayMs ?? 900
  let sequence = 0
  let tickTimer: number | undefined
  let disconnectTimer: number | undefined
  let reconnectTimer: number | undefined
  let disposed = false
  let reconnectAttempt = 0
  let interruptionScheduled = false

  const reportStatus = (status: TokenWorkspaceStreamStatus) => options.onStatus?.(status)

  const clearTickTimer = () => {
    if (tickTimer !== undefined) {
      window.clearInterval(tickTimer)
      tickTimer = undefined
    }
  }

  const startTicking = () => {
    clearTickTimer()
    tickTimer = window.setInterval(() => {
      sequence += 1
      const state = sequence % 3 === 2 ? 'stale' : 'ready'
      listener(getTokenMarketSnapshot(token, interval, sequence, state))
    }, tickIntervalMs)
  }

  const scheduleInterruption = () => {
    if (options.disconnectAfterMs === undefined || interruptionScheduled) return
    interruptionScheduled = true
    disconnectTimer = window.setTimeout(() => {
      if (disposed) return
      clearTickTimer()
      reconnectAttempt += 1
      reportStatus({ state: 'reconnecting', attempt: reconnectAttempt, label: 'Stream interrupted · reconnecting' })
      reconnectTimer = window.setTimeout(() => {
        if (disposed) return
        listener(getTokenMarketSnapshot(token, interval, sequence, sequence % 3 === 2 ? 'stale' : 'ready'))
        reportStatus({ state: 'connected', attempt: reconnectAttempt, label: 'Fixture stream connected' })
        startTicking()
      }, reconnectDelayMs)
    }, options.disconnectAfterMs)
  }

  reportStatus({ state: 'connecting', attempt: 0, label: 'Connecting to fixture stream' })

  const loadTimer = window.setTimeout(() => {
    if (disposed) return
    listener(getTokenMarketSnapshot(token, interval, sequence))
    reportStatus({ state: 'connected', attempt: 0, label: 'Fixture stream connected' })
    startTicking()
    scheduleInterruption()
  }, loadDelayMs)

  return () => {
    disposed = true
    window.clearTimeout(loadTimer)
    if (disconnectTimer !== undefined) window.clearTimeout(disconnectTimer)
    if (reconnectTimer !== undefined) window.clearTimeout(reconnectTimer)
    clearTickTimer()
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
    price: tick.price,
    marketCap: token.fdv,
    gas: '$0.35',
    trader: tick.wallet,
    tracking: sequence % 2 === 0 ? 'Untracked' : 'Tracked',
    txHash: `0xstream…${String(sequence).padStart(4, '0')}`,
    txUrl: `https://basescan.org/tx/stream-${token.id}-${sequence}`,
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

  const normalisedId = tokenId.toLowerCase().replace(/[^a-z0-9]/g, '')
  const canonicalId = Object.entries(canonicalWorkspaceAddresses).find(([knownAddress]) => knownAddress.replace(/[^a-z0-9]/g, '') === normalisedId)?.[1]
  if (canonicalId === tokenWorkspace.id) return tokenWorkspace

  const source = discoveryTokens.find((candidate) => candidate.id === tokenId)
  if (!source) return undefined

  const asset = nativeAssetForChain(source.chain)
  return {
    ...tokenWorkspace,
    ...source,
    address: `0x${source.id.replaceAll('-', '').slice(0, 4)}…${source.id.replaceAll('-', '').slice(-4)}`,
    price: source.marketCap,
    priceEth: '0.00037 ETH',
    marketCap: source.marketCap,
    depth24h: source.liquidity,
    txns24h: source.txns,
    priceChange: source.marketCapChange,
    fdv: source.marketCap,
    volume24h: source.volume,
    pair: `${source.symbol} / ${asset}`,
    safetyNote: source.safety.state === 'passed'
      ? 'Sell simulation passed in the latest fixture snapshot.'
      : 'This fixture needs review before a trade can be prepared.',
  }
}
