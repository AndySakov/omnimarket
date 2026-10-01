// Fixture mode end to end (#63): the stream manager, unchanged, against the MSW fixture stream.
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { create } from '@bufbuild/protobuf'
import Decimal from 'decimal.js'
import { ClientMessageSchema, StreamError_Code } from '../../api/generated/omnimarket/api/v1/stream_pb'
import { readDataSource } from '../../api/source'
import { StreamManager } from '../../api/stream/manager'
import { tokenTopic } from '../../api/stream/topics'
import { fixtureHandlers } from './handlers'
import { FixtureStream } from './fixtureStream'

const NOVA = '0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567'
const source = readDataSource({})
const server = setupServer(...fixtureHandlers(source, { tickIntervalMs: 20 }))

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterAll(() => server.close())

function subscribe(topic: string, sessionToken = '') {
  return create(ClientMessageSchema, { kind: { case: 'subscribe', value: { topic, sessionToken } } })
}

describe('fixture stream', () => {
  it('serves snapshots then numbered ticks to the real stream manager', async () => {
    const manager = new StreamManager({ url: source.wsUrl })
    const topic = tokenTopic(NOVA)
    manager.subscribe(topic)
    manager.start()
    try {
      await vi.waitFor(() => expect(manager.store.getState().topics[topic]?.seq).toBeGreaterThan(2n), { timeout: 2000 })
      const state = manager.store.getState()
      expect(state.connection).toBe('open')
      expect(state.topics[topic]?.phase).toBe('live')
      expect(state.tokens[topic]?.token?.address).toBe(NOVA)
    } finally {
      manager.stop()
    }
  })

  it('moves prices deterministically and exactly', () => {
    const run = () => {
      const stream = new FixtureStream()
      stream.receive(subscribe(tokenTopic(NOVA)))
      return [1, 2, 3].map(() => stream.tick())
    }
    const first = run()
    expect(run()).toEqual(first)
    const ticks = first.map((messages) => messages.find((m) => m.kind.case === 'delta'))
    const prices = ticks.map((m) => (m?.kind.case === 'delta' && m.kind.value.payload.case === 'tokenTick' ? m.kind.value.payload.value.displayPriceUsd : ''))
    expect(prices[0]).toBe(new Decimal('0.0124').times('1.004').toFixed())
    expect(ticks.map((m) => (m?.kind.case === 'delta' ? m.kind.value.seq : 0n))).toEqual([2n, 3n, 4n])
  })

  it('stops ticking a topic after unsubscribe', () => {
    const stream = new FixtureStream()
    stream.receive(subscribe(tokenTopic(NOVA)))
    stream.receive(create(ClientMessageSchema, { kind: { case: 'unsubscribe', value: { topic: tokenTopic(NOVA) } } }))
    expect(stream.tick().map((m) => m.kind.case)).toEqual(['heartbeat'])
  })

  it('refuses unknown topics and an account subscribe without a session', () => {
    const stream = new FixtureStream()
    const [unknown] = stream.receive(subscribe('pools'))
    expect(unknown?.kind.case === 'error' && unknown.kind.value.code).toBe(StreamError_Code.UNKNOWN_TOPIC)
    const [unauthenticated] = stream.receive(subscribe('account'))
    expect(unauthenticated?.kind.case === 'error' && unauthenticated.kind.value.code).toBe(StreamError_Code.UNAUTHENTICATED)
    const [account] = stream.receive(subscribe('account', 'sess_1'))
    expect(account?.kind.case).toBe('snapshot')
  })
})
