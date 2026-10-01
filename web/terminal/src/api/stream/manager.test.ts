// The stream manager's connection handling (#63): visible topics only, reconnect with backoff,
// resubscribe, and stale on missed heartbeats.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EngineStatusSchema, TokenSnapshotSchema } from '../generated/omnimarket/api/v1/market_pb'
import { apiFixture } from '../../mocks/fixtures/api'
import { fakeSockets } from '../../test/fakeSocket'
import { StreamManager, type StreamManagerOptions } from './manager'
import { tokenTopic } from './topics'

const NOVA = '0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567'
const novaTopic = tokenTopic(NOVA)

function setup(options: Partial<StreamManagerOptions> = {}) {
  const sockets = fakeSockets()
  const manager = new StreamManager({ url: 'ws://api.test/v1/stream', createSocket: sockets.create, ...options })
  return { manager, sockets }
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('subscriptions', () => {
  it('subscribes a topic once however many components want it, and unsubscribes after the last leaves', () => {
    const { manager, sockets } = setup()
    manager.start()
    sockets.latest().acceptConnection()

    const leaveA = manager.subscribe(novaTopic)
    const leaveB = manager.subscribe(novaTopic)
    expect(sockets.latest().subscriptions()).toEqual([[novaTopic, true]])

    leaveA()
    leaveA() // releasing twice counts once
    expect(sockets.latest().subscriptions()).toEqual([[novaTopic, true]])
    leaveB()
    expect(sockets.latest().subscriptions()).toEqual([[novaTopic, true], [novaTopic, false]])
    expect(manager.store.getState().topics[novaTopic]).toBeUndefined()
  })

  it('sends subscriptions made before the connection opens once it does', () => {
    const { manager, sockets } = setup()
    manager.subscribe('status')
    manager.start()
    expect(sockets.latest().sent).toEqual([])
    sockets.latest().acceptConnection()
    expect(sockets.latest().subscriptions()).toEqual([['status', true]])
  })

  it('sends the session token only with the account topic', () => {
    const { manager, sockets } = setup({ getSessionToken: () => 'sess_1' })
    manager.start()
    sockets.latest().acceptConnection()
    manager.subscribe('account')
    manager.subscribe('status')
    const tokens = sockets.latest().sent.map((m) => (m.kind.case === 'subscribe' ? m.kind.value.sessionToken : ''))
    expect(tokens).toEqual(['sess_1', ''])
  })

  it('resubscribes a topic whose seq jumped', () => {
    const { manager, sockets } = setup()
    manager.start()
    const socket = sockets.latest()
    socket.acceptConnection()
    manager.subscribe(novaTopic)
    socket.push({ case: 'snapshot', value: { topic: novaTopic, seq: 1n, payload: { case: 'token', value: apiFixture(TokenSnapshotSchema) } } })
    socket.push({ case: 'delta', value: { topic: novaTopic, seq: 3n, payload: { case: 'tokenTick', value: { displayPriceUsd: '1' } } } })
    expect(socket.subscriptions()).toEqual([[novaTopic, true], [novaTopic, false], [novaTopic, true]])
    expect(manager.store.getState().tokens[novaTopic]).toBeUndefined()
  })

  it('ignores a frame that is not a ServerMessage', () => {
    const onProtocolError = vi.fn()
    const { manager, sockets } = setup({ onProtocolError })
    manager.start()
    sockets.latest().acceptConnection()
    const before = manager.store.getState()
    sockets.latest().pushRaw('{"nonsense": true')
    expect(onProtocolError).toHaveBeenCalledOnce()
    expect(manager.store.getState()).toBe(before)
  })
})

describe('reconnect', () => {
  it('shows reconnecting after a drop, then reconnects with backoff and resubscribes', () => {
    const { manager, sockets } = setup({ initialBackoffMs: 500, maxBackoffMs: 4000 })
    manager.start()
    expect(manager.store.getState().connection).toBe('connecting')
    sockets.latest().acceptConnection()
    expect(manager.store.getState().connection).toBe('open')
    manager.subscribe('status')
    manager.subscribe(novaTopic)
    sockets.latest().push({ case: 'snapshot', value: { topic: 'status', seq: 1n, payload: { case: 'status', value: apiFixture(EngineStatusSchema) } } })

    sockets.latest().drop()
    expect(manager.store.getState().connection).toBe('reconnecting')
    // Old data stays on screen while reconnecting; the UI labels it.
    expect(manager.store.getState().status).toBeDefined()

    vi.advanceTimersByTime(499)
    expect(sockets.sockets).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(sockets.sockets).toHaveLength(2)

    // The second attempt fails too: the wait doubles.
    sockets.latest().drop()
    vi.advanceTimersByTime(999)
    expect(sockets.sockets).toHaveLength(2)
    vi.advanceTimersByTime(1)
    expect(sockets.sockets).toHaveLength(3)

    sockets.latest().acceptConnection()
    expect(manager.store.getState().connection).toBe('open')
    expect(sockets.latest().subscriptions()).toEqual([['status', true], [novaTopic, true]])
    expect(manager.store.getState().topics.status?.phase).toBe('resubscribing')

    sockets.latest().push({ case: 'snapshot', value: { topic: 'status', seq: 1n, payload: { case: 'status', value: apiFixture(EngineStatusSchema) } } })
    expect(manager.store.getState().topics.status?.phase).toBe('live')
  })

  it('caps the backoff and shows unavailable after repeated failures, still retrying', () => {
    const { manager, sockets } = setup({ initialBackoffMs: 100, maxBackoffMs: 400, unavailableAfterAttempts: 3 })
    manager.start()
    sockets.latest().drop()
    expect(manager.store.getState().connection).toBe('reconnecting')
    vi.advanceTimersByTime(100)
    sockets.latest().drop()
    vi.advanceTimersByTime(200)
    sockets.latest().drop()
    expect(manager.store.getState().connection).toBe('unavailable')
    vi.advanceTimersByTime(399)
    expect(sockets.sockets).toHaveLength(3)
    vi.advanceTimersByTime(1)
    expect(sockets.sockets).toHaveLength(4)
    sockets.latest().drop()
    vi.advanceTimersByTime(400)
    expect(sockets.sockets).toHaveLength(5)
    sockets.latest().acceptConnection()
    expect(manager.store.getState().connection).toBe('open')
  })

  it('starts counting failures afresh once a connection works, so a long session never drifts to unavailable', () => {
    const { manager, sockets } = setup({ initialBackoffMs: 100, unavailableAfterAttempts: 3 })
    manager.start()
    for (let i = 0; i < 3; i += 1) {
      sockets.latest().acceptConnection()
      sockets.latest().push({ case: 'heartbeat', value: { serverTimeMs: 1n, headBlockNumber: 1n } })
      sockets.latest().drop()
      expect(manager.store.getState().connection).toBe('reconnecting')
      vi.advanceTimersByTime(100)
    }
    sockets.latest().acceptConnection()
    sockets.latest().push({ case: 'heartbeat', value: { serverTimeMs: 1n, headBlockNumber: 1n } })
    sockets.latest().drop()
    expect(manager.store.getState().connection).toBe('reconnecting')
  })

  it('counts a server that accepts and closes at once as failing', () => {
    const { manager, sockets } = setup({ initialBackoffMs: 100, maxBackoffMs: 100, unavailableAfterAttempts: 3 })
    manager.start()
    for (let i = 0; i < 3; i += 1) {
      sockets.latest().acceptConnection()
      sockets.latest().drop()
      vi.advanceTimersByTime(100)
    }
    expect(manager.store.getState().connection).toBe('unavailable')
    expect(sockets.sockets).toHaveLength(4)
  })

  it('counts a socket that can\'t be created as a failed attempt, and keeps retrying', () => {
    const onProtocolError = vi.fn()
    let attempts = 0
    const manager = new StreamManager({
      url: 'ws://api.test/v1/stream',
      createSocket: () => {
        attempts += 1
        throw new Error('blocked')
      },
      initialBackoffMs: 100,
      maxBackoffMs: 100,
      unavailableAfterAttempts: 2,
      onProtocolError,
    })
    manager.start()
    expect(manager.store.getState().connection).toBe('reconnecting')
    vi.advanceTimersByTime(100)
    expect(manager.store.getState().connection).toBe('unavailable')
    vi.advanceTimersByTime(100)
    expect(attempts).toBe(3)
    manager.stop()
  })

  it('stop closes the socket and doesn\'t reconnect', () => {
    const { manager, sockets } = setup()
    manager.start()
    sockets.latest().acceptConnection()
    manager.stop()
    expect(sockets.latest().closed).toBe(true)
    expect(manager.store.getState().connection).toBe('idle')
    vi.advanceTimersByTime(60_000)
    expect(sockets.sockets).toHaveLength(1)

    manager.start()
    expect(sockets.sockets).toHaveLength(2)
  })
})

describe('heartbeats', () => {
  it('marks the connection stale after three missed heartbeats, and live again on the next', () => {
    const { manager, sockets } = setup()
    manager.start()
    const socket = sockets.latest()
    socket.acceptConnection()
    const heartbeat = () => socket.push({ case: 'heartbeat', value: { serverTimeMs: 1n, headBlockNumber: 1n } })

    vi.advanceTimersByTime(2_000)
    heartbeat()
    vi.advanceTimersByTime(2_999)
    expect(manager.store.getState().connection).toBe('open')
    vi.advanceTimersByTime(1)
    expect(manager.store.getState().connection).toBe('stale')

    heartbeat()
    expect(manager.store.getState().connection).toBe('open')
  })

  it('replaces a connection that stays silent', () => {
    const { manager, sockets } = setup({ deadAfterMs: 10_000, initialBackoffMs: 500 })
    manager.start()
    sockets.latest().acceptConnection()
    manager.subscribe('status')
    vi.advanceTimersByTime(10_000)
    expect(sockets.sockets[0]!.closed).toBe(true)
    expect(manager.store.getState().connection).toBe('reconnecting')
    vi.advanceTimersByTime(500)
    expect(sockets.sockets).toHaveLength(2)
    sockets.latest().acceptConnection()
    expect(sockets.latest().subscriptions()).toEqual([['status', true]])
  })
})
