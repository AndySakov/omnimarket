// Per-token rendering (#63): a tick on one token rerenders only that token's components.
import { act, cleanup, render, screen } from '@testing-library/react'
import { Profiler, type ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { TokenSnapshotSchema } from '../generated/omnimarket/api/v1/market_pb'
import { apiFixture } from '../../mocks/fixtures/api'
import { fakeSockets, type FakeSocket } from '../../test/fakeSocket'
import { StreamManager } from './manager'
import { StreamProvider } from './StreamProvider'
import { useConnection, useTokenStream } from './hooks'
import { tokenTopic } from './topics'

const A = '0x00000000000000000000000000000000000000aa'
const B = '0x00000000000000000000000000000000000000bb'

function TokenRow({ address }: { address: string }) {
  const { data, state } = useTokenStream(address)
  return (
    <div data-testid={address}>
      {data?.displayPriceUsd ?? '—'} {state}
    </div>
  )
}

function Header() {
  const { state, source } = useConnection()
  return <div data-testid="connection">{source} {state}</div>
}

function setup(children: ReactNode) {
  const sockets = fakeSockets()
  const manager = new StreamManager({ url: 'ws://api.test/v1/stream', createSocket: sockets.create })
  const renders: Record<string, number> = {}
  const onRender = (id: string) => {
    renders[id] = (renders[id] ?? 0) + 1
  }
  render(
    <StreamProvider manager={manager} source="live">
      <Profiler id="header" onRender={onRender}><Header /></Profiler>
      <Profiler id={A} onRender={onRender}><TokenRow address={A} /></Profiler>
      <Profiler id={B} onRender={onRender}><TokenRow address={B} /></Profiler>
      {children}
    </StreamProvider>,
  )
  act(() => {
    manager.start()
    sockets.latest().acceptConnection()
  })
  return { manager, socket: sockets.latest(), renders }
}

function snapshot(socket: FakeSocket, address: string) {
  const fixture = apiFixture(TokenSnapshotSchema)
  act(() =>
    socket.push({
      case: 'snapshot',
      value: { topic: tokenTopic(address), seq: 1n, payload: { case: 'token', value: { ...fixture, displayPriceUsd: '1' } } },
    }),
  )
}

beforeEach(() => vi.useFakeTimers())
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('stream hooks', () => {
  it('subscribe the topics of mounted components only', () => {
    const { socket } = setup(null)
    expect(socket.subscriptions()).toEqual([[tokenTopic(A), true], [tokenTopic(B), true]])
  })

  it('a tick on one token doesn\'t rerender another token\'s row', () => {
    const { socket, renders } = setup(null)
    snapshot(socket, A)
    snapshot(socket, B)
    expect(screen.getByTestId(A)).toHaveTextContent('1 live')

    const before = { ...renders }
    act(() =>
      socket.push({
        case: 'delta',
        value: { topic: tokenTopic(A), seq: 2n, payload: { case: 'tokenTick', value: { displayPriceUsd: '2' } } },
      }),
    )
    expect(screen.getByTestId(A)).toHaveTextContent('2 live')
    expect(renders[A]).toBe(before[A]! + 1)
    expect(renders[B]).toBe(before[B])
    expect(renders.header).toBe(before.header)
  })

  it('label every row reconnecting after a drop, with the old numbers still visible', () => {
    const { socket } = setup(null)
    snapshot(socket, A)
    act(() => socket.drop())
    expect(screen.getByTestId(A)).toHaveTextContent('1 reconnecting')
    expect(screen.getByTestId('connection')).toHaveTextContent('live reconnecting')
  })

  it('label rows stale after three missed heartbeats', () => {
    const { socket } = setup(null)
    snapshot(socket, A)
    act(() => vi.advanceTimersByTime(3_000))
    expect(screen.getByTestId(A)).toHaveTextContent('1 stale')
  })
})
