// Fixture mode end to end (#63): the stream manager, unchanged, against the MSW fixture stream.
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { create } from '@bufbuild/protobuf'
import Decimal from 'decimal.js'
import { ClientMessageSchema, StreamError_Code } from '../../api/generated/omnimarket/api/v1/stream_pb'
import { readDataSource } from '../../api/source'
import { StreamManager } from '../../api/stream/manager'
import { STATUS_TOPIC, tokenTopic } from '../../api/stream/topics'
import { fixtureHandlers } from './handlers'
import { FixtureStream } from './fixtureStream'
import { FIRST_NEW_POOL_TICK, NEW_POOL_EVERY_TICKS } from './discoveryFeed'

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

  it('serves a discovery feed that moves prices and adds new pools, numbered without gaps', () => {
    const stream = new FixtureStream()
    const [first] = stream.receive(subscribe('discovery'))
    expect(first?.kind.case === 'snapshot' && first.kind.value.payload.case === 'discovery' && first.kind.value.payload.value.rows).toHaveLength(6)

    const seqs: bigint[] = []
    const added: Array<{ name: string; createdAtMs: bigint; heartbeatMs: bigint }> = []
    for (let tick = 1; tick <= FIRST_NEW_POOL_TICK + NEW_POOL_EVERY_TICKS; tick += 1) {
      const messages = stream.tick()
      const heartbeat = messages[0]
      const heartbeatMs = heartbeat?.kind.case === 'heartbeat' ? heartbeat.kind.value.serverTimeMs : 0n
      for (const message of messages) {
        if (message.kind.case !== 'delta') continue
        seqs.push(message.kind.value.seq)
        const payload = message.kind.value.payload
        if (payload.case === 'discoveryRow' && payload.value.poolCreatedAtMs === heartbeatMs) {
          added.push({ name: payload.value.token?.name ?? '', createdAtMs: payload.value.poolCreatedAtMs, heartbeatMs })
        }
      }
    }
    // A price move every tick, plus a new pool at the first-new-pool tick and every few after.
    expect(seqs).toEqual(seqs.map((_, i) => BigInt(i + 2)))
    expect(added.map((a) => a.name)).toEqual(['Brine (fixture)', 'Cobalt (fixture)'])
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

  it('sends the engine status once per block, every second tick, in step with the heartbeat head', () => {
    const stream = new FixtureStream()
    stream.receive(subscribe(STATUS_TOPIC))
    const ticks = [1, 2, 3, 4].map(() => stream.tick())
    const statuses = ticks.map((messages) => messages.find((m) => m.kind.case === 'delta'))
    expect(statuses.map((m) => m !== undefined)).toEqual([false, true, false, true])
    const [, second, , fourth] = statuses.map((m) => (m?.kind.case === 'delta' && m.kind.value.payload.case === 'status' ? m.kind.value : undefined))
    const heartbeatHead = (messages: (typeof ticks)[number]) =>
      messages.find((m) => m.kind.case === 'heartbeat')?.kind.value
    expect(second?.seq).toBe(2n)
    expect(fourth?.seq).toBe(3n)
    expect(second?.payload.value).toMatchObject({ headBlockNumber: 36120451n, headBlockTimeMs: 1790000002000n, uptimeMs: 5402000n })
    expect(fourth?.payload.value).toMatchObject({ headBlockNumber: 36120452n, headBlockTimeMs: 1790000004000n })
    const hb = heartbeatHead(ticks[3]!)
    expect(hb && 'headBlockNumber' in hb ? hb.headBlockNumber : 0n).toBe(36120452n)
    // Shadow checks keep running, and keep agreeing.
    const checks = (m: typeof second) => (m?.payload.case === 'status' ? m.payload.value.shadowChecks : 0n)
    expect(checks(fourth)).toBeGreaterThan(checks(second))
  })
})
