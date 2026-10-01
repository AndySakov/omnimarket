// The status bar (#71): updates every block, and goes visibly stale when the feed stops.
import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EngineStatus_Mode, EngineStatusSchema, type EngineStatus } from '../../api/generated/omnimarket/api/v1/market_pb'
import { StreamManager } from '../../api/stream/manager'
import { StreamProvider } from '../../api/stream/StreamProvider'
import { STATUS_TOPIC } from '../../api/stream/topics'
import { apiFixture } from '../../mocks/fixtures/api'
import { fakeSockets, type FakeSocket } from '../../test/fakeSocket'
import { EngineStatusBar, EngineStatusBarView } from './EngineStatusBar'
import { HEAD_STALE_AFTER_MS } from './engineStatus'

const fixture = apiFixture(EngineStatusSchema)

function at(block: bigint): EngineStatus {
  return { ...fixture, headBlockNumber: block }
}

function bar() {
  return screen.getByRole('button', { name: /^Engine:/ })
}

describe('EngineStatusBarView', () => {
  afterEach(cleanup)

  it('shows Base, the head, lag, pools, shadow checks and the mode, in text', () => {
    render(<EngineStatusBarView status={fixture} state="live" source="live" />)
    expect(bar()).toHaveTextContent(/Live.*Base.*Block36,120,450.*Lag1 block · 2.3s.*Pools70,086.*Shadow checks5,117 \/ 5,120/)
    expect(bar()).toHaveAccessibleName(/^Engine: Live\. Base, block 36,120,450, lag 1 block · 2\.3s, 70,086 pools, shadow checks 5,117 \/ 5,120 agreed\. Updates every block\.$/)
  })

  it('names a replay as Replay', () => {
    render(<EngineStatusBarView status={{ ...fixture, mode: EngineStatus_Mode.REPLAY }} state="live" source="replay" />)
    expect(bar()).toHaveAccessibleName(/^Engine: Replay\./)
  })

  it.each([
    ['stale', 'Stale'],
    ['reconnecting', 'Reconnecting'],
    ['unavailable', 'Unavailable'],
  ] as const)('keeps old numbers on screen but says %s instead of the mode', (state, label) => {
    render(<EngineStatusBarView status={fixture} state={state} source="live" />)
    expect(bar()).toHaveAccessibleName(new RegExp(`^Engine: ${label}\\.`))
    expect(bar()).toHaveAttribute('data-state', state)
    expect(bar()).toHaveTextContent('36,120,450')
  })

  it('says so while there is no status yet, or none to be had', () => {
    const { rerender } = render(<EngineStatusBarView status={undefined} state="loading" source="live" />)
    expect(screen.getByText('Engine status loading')).toBeInTheDocument()
    rerender(<EngineStatusBarView status={undefined} state="unavailable" source="live" />)
    expect(screen.getByText('Engine status unavailable')).toBeInTheDocument()
  })
})

describe('EngineStatusBar on the stream', () => {
  let socket: FakeSocket

  beforeEach(() => {
    vi.useFakeTimers()
    const sockets = fakeSockets()
    const manager = new StreamManager({ url: 'ws://api.test/v1/stream', createSocket: sockets.create })
    render(
      <StreamProvider manager={manager} source="live">
        <EngineStatusBar />
      </StreamProvider>,
    )
    act(() => {
      manager.start()
      sockets.latest().acceptConnection()
    })
    socket = sockets.latest()
    act(() => socket.push({ case: 'snapshot', value: { topic: STATUS_TOPIC, seq: 1n, payload: { case: 'status', value: at(100n) } } }))
  })

  afterEach(() => {
    cleanup()
    vi.useRealTimers()
  })

  function block(seq: bigint, head: bigint) {
    act(() => {
      socket.push({ case: 'delta', value: { topic: STATUS_TOPIC, seq, payload: { case: 'status', value: at(head) } } })
      socket.push({ case: 'heartbeat', value: { serverTimeMs: 0n, headBlockNumber: head } })
    })
  }

  it('subscribes the status topic and updates on every block', () => {
    expect(socket.subscriptions()).toContainEqual([STATUS_TOPIC, true])
    expect(bar()).toHaveTextContent('Block100')
    block(2n, 101n)
    expect(bar()).toHaveTextContent('Block101')
    block(3n, 102n)
    expect(bar()).toHaveTextContent('Block102')
    expect(bar()).toHaveAttribute('data-state', 'live')
  })

  it('goes stale when heartbeats stop', () => {
    block(2n, 101n)
    act(() => vi.advanceTimersByTime(3_100))
    expect(bar()).toHaveAttribute('data-state', 'stale')
    expect(bar()).toHaveAccessibleName(/^Engine: Stale\./)
  })

  it('goes stale when blocks stop although heartbeats keep coming, and recovers on the next block', () => {
    for (let second = 0; second * 1_000 <= HEAD_STALE_AFTER_MS; second += 1) {
      act(() => {
        socket.push({ case: 'heartbeat', value: { serverTimeMs: 0n, headBlockNumber: 100n } })
        vi.advanceTimersByTime(1_000)
      })
    }
    expect(bar()).toHaveAttribute('data-state', 'stale')
    block(2n, 101n)
    expect(bar()).toHaveAttribute('data-state', 'live')
  })
})
