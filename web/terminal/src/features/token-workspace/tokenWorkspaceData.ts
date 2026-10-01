import { discoveryTokens } from '../../mocks/discoveryFixtures'
import { tokenWorkspace } from '../../mocks/tokenWorkspaceFixtures'
import type { TokenWorkspaceFixture, TokenWorkspaceState } from '../../domains/market/tokenWorkspace'
import { nativeAssetForChain } from '../../domains/market/tokenWorkspace'

export type TokenWorkspaceSnapshot = {
  token: TokenWorkspaceFixture
  state: TokenWorkspaceState
  source: 'fixture'
  freshness: 'fresh' | 'stale'
  updatedLabel: string
}

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
