import { describe, expect, it } from 'vitest'
import { getTokenWorkspaceSnapshot } from './tokenWorkspaceData'

describe('getTokenWorkspaceSnapshot', () => {
  it('resolves the default token into a ready fixture snapshot', () => {
    const snapshot = getTokenWorkspaceSnapshot()

    expect(snapshot.token.symbol).toBe('NOVA')
    expect(snapshot.state).toBe('ready')
    expect(snapshot.source).toBe('fixture')
    expect(snapshot.freshness).toBe('fresh')
  })

  it('maps a selected discovery token into the shared workspace shape', () => {
    const snapshot = getTokenWorkspaceSnapshot('flux-core')

    expect(snapshot.token.symbol).toBe('FLUX')
    expect(snapshot.token.pair).toBe('FLUX / SOL')
    expect(snapshot.token.trades.length).toBeGreaterThan(0)
  })

  it('returns a missing state for an unknown token instead of silently showing another token', () => {
    const snapshot = getTokenWorkspaceSnapshot('not-in-fixtures')

    expect(snapshot.state).toBe('missing')
    expect(snapshot.updatedLabel).toBe('Unavailable')
    expect(snapshot.freshness).toBe('stale')
  })

  it('preserves an explicitly requested stale state', () => {
    const snapshot = getTokenWorkspaceSnapshot('nova-set', 'stale')

    expect(snapshot.state).toBe('stale')
    expect(snapshot.updatedLabel).toBe('Older than 2m')
    expect(snapshot.token.symbol).toBe('NOVA')
  })
})
