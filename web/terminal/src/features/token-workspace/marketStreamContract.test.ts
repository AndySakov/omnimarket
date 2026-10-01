import { afterEach, describe, expect, it, vi } from 'vitest'
import { createFixtureMarketStreamClient } from './marketStreamClient'
import { applyMarketStreamEvent, type MarketStreamEvent } from './marketStreamContract'
import { getTokenMarketSnapshot, getTokenWorkspaceSnapshot } from './tokenWorkspaceData'

afterEach(() => {
  vi.useRealTimers()
})

describe('market stream contract', () => {
  it('emits a snapshot, update, and heartbeat through the fixture client', () => {
    vi.useFakeTimers()
    const events: MarketStreamEvent[] = []
    const client = createFixtureMarketStreamClient({ loadDelayMs: 10, tickIntervalMs: 100, heartbeatIntervalMs: 50 })
    const unsubscribe = client.subscribe(
      { tokenId: 'nova-set', interval: '5m' },
      { onEvent: (event) => events.push(event), onStatus: vi.fn() },
    )

    vi.advanceTimersByTime(10)
    expect(events[0]?.type).toBe('snapshot')
    expect(events[0]?.sequence).toBe(0)
    expect(events[0]?.serverTime).toBe('2026-09-30T09:00:00.000Z')

    vi.advanceTimersByTime(100)
    expect(events.some((event) => event.type === 'heartbeat')).toBe(true)
    expect(events.some((event) => event.type === 'update' && event.sequence === 1)).toBe(true)

    unsubscribe()
    const eventCount = events.length
    vi.advanceTimersByTime(200)
    expect(events).toHaveLength(eventCount)
  })

  it('keeps the latest snapshot for heartbeats and rejects older updates', () => {
    const token = getTokenWorkspaceSnapshot().token
    const current = getTokenMarketSnapshot(token, '5m', 2)
    const older = getTokenMarketSnapshot(token, '5m', 1)

    expect(applyMarketStreamEvent(current, { type: 'update', tokenId: token.id, sequence: 1, serverTime: '2026-09-30T09:00:01.000Z', snapshot: older })).toBe(current)
    expect(applyMarketStreamEvent(current, { type: 'heartbeat', tokenId: token.id, sequence: 2, serverTime: '2026-09-30T09:00:02.000Z' })).toBe(current)
  })

  it('preserves market values while exposing a retryable stream error', () => {
    const token = getTokenWorkspaceSnapshot().token
    const current = getTokenMarketSnapshot(token, '5m')
    const next = applyMarketStreamEvent(current, {
      type: 'error',
      tokenId: token.id,
      sequence: 0,
      serverTime: '2026-09-30T09:00:03.000Z',
      error: { code: 'STREAM_UNAVAILABLE', message: 'Gateway unavailable', retryable: true },
    })

    expect(next.token.price).toBe(current.token.price)
    expect(next.state).toBe('error')
    expect(next.updatedLabel).toBe('Stream reconnecting')
  })

  it('can simulate a gateway error and stop fixture updates', () => {
    vi.useFakeTimers()
    const events: MarketStreamEvent[] = []
    const statuses: string[] = []
    const client = createFixtureMarketStreamClient({ loadDelayMs: 10, tickIntervalMs: 100, errorAfterMs: 50 })
    const unsubscribe = client.subscribe(
      { tokenId: 'nova-set', interval: '5m' },
      { onEvent: (event) => events.push(event), onStatus: (status) => statuses.push(status.state) },
    )

    vi.advanceTimersByTime(10)
    vi.advanceTimersByTime(50)
    expect(events.at(-1)?.type).toBe('error')
    expect(statuses.at(-1)).toBe('error')

    const eventCount = events.length
    vi.advanceTimersByTime(200)
    expect(events).toHaveLength(eventCount)
    unsubscribe()
  })
})
