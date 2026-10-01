// Switching between fixtures, replay and live needs only an env change (#63).
import { describe, expect, it } from 'vitest'
import { DataSourceError, readDataSource, streamUrlFor } from './source'

describe('readDataSource', () => {
  it('defaults to fixtures, which needs no URL', () => {
    const source = readDataSource({})
    expect(source.kind).toBe('fixtures')
    expect(source.wsUrl).toBe(`${source.apiUrl.replace('http', 'ws')}/v1/stream`)
  })

  it('reads replay and live from the env, deriving the stream URL', () => {
    expect(readDataSource({ VITE_DATA_SOURCE: 'replay', VITE_API_URL: 'http://127.0.0.1:8090/' })).toEqual({
      kind: 'replay',
      apiUrl: 'http://127.0.0.1:8090',
      wsUrl: 'ws://127.0.0.1:8090/v1/stream',
    })
    expect(readDataSource({ VITE_DATA_SOURCE: 'live', VITE_API_URL: 'https://api.example.test' })).toEqual({
      kind: 'live',
      apiUrl: 'https://api.example.test',
      wsUrl: 'wss://api.example.test/v1/stream',
    })
  })

  it('takes an explicit stream URL', () => {
    expect(readDataSource({ VITE_DATA_SOURCE: 'live', VITE_API_URL: 'http://a', VITE_WS_URL: 'ws://b/s' }).wsUrl).toBe('ws://b/s')
  })

  it('refuses an unknown source, or replay and live without a URL', () => {
    expect(() => readDataSource({ VITE_DATA_SOURCE: 'mainnet' })).toThrow(DataSourceError)
    expect(() => readDataSource({ VITE_DATA_SOURCE: 'live' })).toThrow(/VITE_API_URL/)
    expect(() => readDataSource({ VITE_DATA_SOURCE: 'replay', VITE_API_URL: ' ' })).toThrow(/VITE_API_URL/)
  })

  it('keeps a path prefix on the API URL', () => {
    expect(streamUrlFor('https://host.test/api')).toBe('wss://host.test/api/v1/stream')
  })
})
