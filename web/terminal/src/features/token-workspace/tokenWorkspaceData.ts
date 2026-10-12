import { discoveryTokens } from '../../mocks/discoveryFixtures'
import { tokenWorkspace } from '../../mocks/tokenWorkspaceFixtures'
import type {
  TokenChartData,
  TokenChartInterval,
  TokenChartSnapshot,
  TokenChartState,
  TokenWorkspaceFixture,
  TokenWorkspaceState,
} from '../../domains/market/tokenWorkspace'
import { nativeAssetForChain } from '../../domains/market/tokenWorkspace'

export type TokenWorkspaceSnapshot = {
  token: TokenWorkspaceFixture
  state: TokenWorkspaceState
  source: 'fixture'
  freshness: 'fresh' | 'stale'
  updatedLabel: string
}

/** The address used by the contract fixture for the NovaSet design fixture. */
export const canonicalWorkspaceAddress = '0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567'

const chartGroupSizes: Record<TokenChartInterval, number> = {
  '1s': 1,
  '1m': 1,
  '5m': 1,
  '1h': 4,
}

/**
 * The static fixture supplies images and the secondary panels while the typed
 * API stream supplies market values. Keeping this lookup pure makes it usable
 * by the context rail and Storybook without creating a second data client.
 */
export function getTokenWorkspaceSnapshot(tokenId?: string, state: TokenWorkspaceState = 'ready'): TokenWorkspaceSnapshot {
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

/** Resolves a UI token id to the full address used by stream topics and routes. */
export function getTokenRouteAddress(tokenId: string): string {
  if (/^0x[0-9a-f]{40}$/i.test(tokenId)) return tokenId.toLowerCase()
  if (tokenId === tokenWorkspace.id || tokenId === '0x7b3f8a42') return canonicalWorkspaceAddress
  const token = discoveryTokens.find((candidate) => candidate.id === tokenId)
  return token ? fixtureAddressForTokenId(token.id) : tokenId
}

/** Resolves the legacy fixture route and full live addresses for compatibility. */
export function resolveTokenIdByAddress(address: string): string | undefined {
  const normalized = address.toLowerCase().replace(/[^a-z0-9]/g, '')
  if (normalized === canonicalWorkspaceAddress.replace(/[^a-z0-9]/g, '') || normalized === '0x7b3f8a42') return tokenWorkspace.id
  if (/^0x[0-9a-f]{40}$/.test(address.toLowerCase())) return address.toLowerCase()
  const token = discoveryTokens.find((candidate) => fixtureAddressForTokenId(candidate.id).replace(/[^a-z0-9]/g, '') === normalized)
  return token?.id
}

/** Stable, valid Base address for the static context-rail fixtures. */
export function fixtureAddressForTokenId(tokenId: string): string {
  const hex = Array.from(tokenId)
    .map((character) => character.charCodeAt(0).toString(16))
    .join('')
    .padEnd(40, '0')
    .slice(0, 40)
  return `0x${hex}`
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

function aggregateChartData(chart: TokenChartData, groupSize: number): TokenChartData {
  if (groupSize === 1) return chart
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
  if (!tokenId || tokenId === tokenWorkspace.id || tokenId === '0x7b3f8a42' || tokenId.toLowerCase() === canonicalWorkspaceAddress || /^0x[0-9a-f]{40}$/i.test(tokenId)) {
    return tokenWorkspace
  }

  const source = discoveryTokens.find((candidate) => candidate.id === tokenId)
  if (!source) return undefined

  return {
    ...tokenWorkspace,
    ...source,
    address: fixtureAddressForTokenId(source.id),
    price: source.marketCap,
    priceEth: '0.00037 ETH',
    marketCap: source.marketCap,
    depth24h: source.liquidity,
    txns24h: source.txns,
    priceChange: source.marketCapChange,
    fdv: source.marketCap,
    totalSupply: '1B',
    volume24h: source.volume,
    pair: `${source.symbol} / ${nativeAssetForChain(source.chain)}`,
    safetyNote: source.safety.state === 'passed'
      ? 'Sell simulation passed in the latest fixture snapshot.'
      : 'This fixture needs review before a trade can be prepared.',
  }
}
