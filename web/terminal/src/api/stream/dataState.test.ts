import { describe, expect, it } from 'vitest'
import { dataStateOf } from './dataState'

const live = { phase: 'live' as const, seq: 1n }

describe('dataStateOf', () => {
  it('is live only with a live topic on an open connection', () => {
    expect(dataStateOf('open', live)).toBe('live')
  })

  it('never calls data live while the connection is in doubt', () => {
    expect(dataStateOf('stale', live)).toBe('stale')
    expect(dataStateOf('reconnecting', live)).toBe('reconnecting')
    expect(dataStateOf('unavailable', live)).toBe('unavailable')
    expect(dataStateOf('open', { phase: 'resubscribing', seq: 0n })).toBe('reconnecting')
  })

  it('is loading before the first snapshot', () => {
    expect(dataStateOf('connecting', undefined)).toBe('loading')
    expect(dataStateOf('open', undefined)).toBe('loading')
    expect(dataStateOf('open', { phase: 'loading', seq: 0n })).toBe('loading')
    expect(dataStateOf('idle', live)).toBe('loading')
  })

  it('is unavailable when the server refused the topic', () => {
    expect(dataStateOf('open', { phase: 'unavailable', seq: 0n, error: 'x' })).toBe('unavailable')
  })
})
