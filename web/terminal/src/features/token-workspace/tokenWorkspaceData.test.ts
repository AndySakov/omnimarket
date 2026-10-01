import { afterEach, describe, expect, it, vi } from 'vitest'
import { createTokenWorkspaceStream, getTokenChartSnapshot, getTokenRouteAddress, getTokenWorkspaceSnapshot, resolveTokenIdByAddress } from './tokenWorkspaceData'

afterEach(() => {
  vi.useRealTimers()
})

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

    expect(address).toBe('0x7b3f8a42')
    expect(resolveTokenIdByAddress(address)).toBe('nova-set')
  })

  it('preserves an explicitly requested stale state', () => {
    const snapshot = getTokenWorkspaceSnapshot('nova-set', 'stale')

    expect(snapshot.state).toBe('stale')
    expect(snapshot.updatedLabel).toBe('Older than 2m')
    expect(snapshot.token.symbol).toBe('NOVA')
  })

  it('aggregates the fixture candles when a larger interval is selected', () => {
    const token = getTokenWorkspaceSnapshot().token
    const fifteenMinute = getTokenChartSnapshot(token, '15m')
    const hourly = getTokenChartSnapshot(token, '1h')

    expect(fifteenMinute.data?.candles).toHaveLength(16)
    expect(hourly.data?.candles).toHaveLength(4)
    expect(hourly.data?.volume[0]?.value).toBeGreaterThan(fifteenMinute.data?.volume[0]?.value ?? 0)
  })

  it('exposes explicit empty and error chart states without inventing candles', () => {
    const token = getTokenWorkspaceSnapshot().token

    expect(getTokenChartSnapshot(token, '5m', 'empty').data).toBeNull()
    expect(getTokenChartSnapshot(token, '5m', 'error').updatedLabel).toBe('Snapshot unavailable')
  })

  it('emits deterministic market ticks and stops after unsubscribe', () => {
    vi.useFakeTimers()
    const listener = vi.fn()
    const token = getTokenWorkspaceSnapshot().token
    const unsubscribe = createTokenWorkspaceStream(token, '1h', listener, { loadDelayMs: 100, tickIntervalMs: 200 })

    vi.advanceTimersByTime(99)
    expect(listener).not.toHaveBeenCalled()

    vi.advanceTimersByTime(1)
    expect(listener).toHaveBeenCalledTimes(1)
    expect(listener.mock.calls[0]?.[0].interval).toBe('1h')

    vi.advanceTimersByTime(200)
    expect(listener).toHaveBeenCalledTimes(2)

    unsubscribe()
    vi.advanceTimersByTime(400)
    expect(listener).toHaveBeenCalledTimes(2)
  })

  it('reports connection lifecycle and resumes after a fixture interruption', () => {
    vi.useFakeTimers()
    const listener = vi.fn()
    const statuses: string[] = []
    const token = getTokenWorkspaceSnapshot().token
    const unsubscribe = createTokenWorkspaceStream(token, '5m', listener, {
      loadDelayMs: 10,
      tickIntervalMs: 200,
      disconnectAfterMs: 50,
      reconnectDelayMs: 20,
      onStatus: (status) => statuses.push(status.state),
    })

    expect(statuses).toEqual(['connecting'])
    vi.advanceTimersByTime(10)
    expect(statuses).toEqual(['connecting', 'connected'])
    expect(listener).toHaveBeenCalledTimes(1)

    vi.advanceTimersByTime(50)
    expect(statuses).toEqual(['connecting', 'connected', 'reconnecting'])
    vi.advanceTimersByTime(20)
    expect(statuses).toEqual(['connecting', 'connected', 'reconnecting', 'connected'])
    expect(listener).toHaveBeenCalledTimes(2)

    unsubscribe()
  })
})
