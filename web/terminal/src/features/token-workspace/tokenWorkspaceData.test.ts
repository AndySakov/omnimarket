import { describe, expect, it } from 'vitest'
import { canonicalWorkspaceAddress, getTokenChartSnapshot, getTokenRouteAddress, getTokenWorkspaceSnapshot, resolveTokenIdByAddress } from './tokenWorkspaceData'

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

  it('round-trips the public address route for the default fixture', () => {
    const address = getTokenRouteAddress('nova-set')

    expect(address).toBe(canonicalWorkspaceAddress)
    expect(resolveTokenIdByAddress(address)).toBe('nova-set')
  })

  it('preserves an explicitly requested stale state', () => {
    const snapshot = getTokenWorkspaceSnapshot('nova-set', 'stale')

    expect(snapshot.state).toBe('stale')
    expect(snapshot.updatedLabel).toBe('Older than 2m')
    expect(snapshot.token.symbol).toBe('NOVA')
  })

  it('aggregates the fixture candles when the contract hourly interval is selected', () => {
    const token = getTokenWorkspaceSnapshot().token
    const fiveMinute = getTokenChartSnapshot(token, '5m')
    const hourly = getTokenChartSnapshot(token, '1h')

    expect(fiveMinute.data?.candles).toHaveLength(16)
    expect(hourly.data?.candles).toHaveLength(4)
    expect(hourly.data?.volume[0]?.value).toBeGreaterThan(fiveMinute.data?.volume[0]?.value ?? 0)
  })

  it('exposes explicit empty and error chart states without inventing candles', () => {
    const token = getTokenWorkspaceSnapshot().token

    expect(getTokenChartSnapshot(token, '5m', 'empty').data).toBeNull()
    expect(getTokenChartSnapshot(token, '5m', 'error').updatedLabel).toBe('Snapshot unavailable')
  })

})
