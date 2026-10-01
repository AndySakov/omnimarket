import { describe, expect, it } from 'vitest'
import { candlesTopic, tokenTopic, topicKind, tradesTopic } from './topics'

const ADDRESS = '0x9A1B2C3D4E5F60718293A4B5C6D7E8F901234567'

describe('topics', () => {
  it('builds topic strings with lowercase addresses', () => {
    expect(tokenTopic(ADDRESS)).toBe('token:0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567')
    expect(tradesTopic(ADDRESS)).toBe('trades:0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567')
    expect(candlesTopic(ADDRESS, '5m')).toBe('candles:0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567:5m')
  })

  it('recognises the contract\'s topics and nothing else', () => {
    expect(topicKind('status')).toBe('status')
    expect(topicKind('discovery')).toBe('discovery')
    expect(topicKind('account')).toBe('account')
    expect(topicKind(tokenTopic(ADDRESS))).toBe('token')
    expect(topicKind(tradesTopic(ADDRESS))).toBe('trades')
    expect(topicKind(candlesTopic(ADDRESS, '1h'))).toBe('candles')
    for (const bad of ['', 'token', 'token:0x12', `token:${ADDRESS}`, `candles:${ADDRESS.toLowerCase()}:2m`, `token:${ADDRESS.toLowerCase()}:1m`, 'pools']) {
      expect(topicKind(bad), bad).toBeUndefined()
    }
  })
})
