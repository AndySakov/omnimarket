// The streamed regions (#139), checked against the manifest in `regions.ts`: each renders hostile
// contract values without crashing or printing NaN, each has a story per state it shows, and no
// component under `features/` reads the stream without being listed.
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { toJson, type DescMessage, type JsonObject } from '@bufbuild/protobuf'
import Decimal from 'decimal.js'
import { Component, type ReactElement, type ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EngineStatusSchema } from './api/generated/omnimarket/api/v1/market_pb'
import { ServerMessageSchema, SnapshotSchema } from './api/generated/omnimarket/api/v1/stream_pb'
import hooksSource from './api/stream/hooks.ts?raw'
import { useTokenStream } from './api/stream/hooks'
import { StreamManager } from './api/stream/manager'
import { StreamProvider } from './api/stream/StreamProvider'
import { canonicalWorkspaceAddress } from './features/token-workspace/tokenWorkspaceData'
import { FixtureStream } from './mocks/api/fixtureStream'
import { apiFixture } from './mocks/fixtures/api'
import { regions, streamStates, type Region } from './regions'
import { fakeSockets } from './test/fakeSocket'
import { hostileVariants } from './test/hostile'

// jsdom can't draw the chart. This stand-in takes what the region hands it and refuses any number
// that isn't finite, which the real chart would draw as nothing or as garbage.
vi.mock('lightweight-charts', () => {
  const finite = (point: Record<string, unknown>) => {
    for (const [key, value] of Object.entries(point)) {
      if (typeof value === 'number' && !Number.isFinite(value)) throw new Error(`the chart was given ${key} = ${value}`)
    }
  }
  const series = () => ({ setData: (points: Record<string, unknown>[]) => points.forEach(finite), priceScale: () => ({ applyOptions: () => undefined }) })
  return {
    CandlestickSeries: 'candlestick',
    HistogramSeries: 'histogram',
    ColorType: { Solid: 'solid' },
    createChart: () => ({ addSeries: series, timeScale: () => ({ fitContent: () => undefined }), remove: () => undefined }),
  }
})

const status = apiFixture(EngineStatusSchema)

class Catch extends Component<{ onError: (error: Error) => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() {
    return { failed: true }
  }
  componentDidCatch(error: Error) {
    this.props.onError(error)
  }
  render() {
    return this.state.failed ? null : this.props.children
  }
}

/**
 * Renders `tree` on a socket the fixture stream answers, `answer` choosing each topic's snapshot,
 * then presses `reveal` if given. Returns what the tree threw.
 */
function show(tree: ReactElement, reveal: string | undefined, answer: (topic: string, snapshot: JsonObject) => JsonObject): Error[] {
  const errors: Error[] = []
  const sockets = fakeSockets()
  const manager = new StreamManager({ url: 'ws://regions.invalid/v1/stream', createSocket: sockets.create })
  render(
    <StreamProvider manager={manager} source="fixtures">
      <Catch onError={(error) => errors.push(error)}>{tree}</Catch>
    </StreamProvider>,
  )
  act(() => {
    manager.start()
    sockets.latest().acceptConnection()
  })
  const socket = sockets.latest()
  const fixtures = new FixtureStream()
  act(() => {
    for (const message of [...socket.sent]) {
      if (message.kind.case !== 'subscribe') continue
      const topic = message.kind.value.topic
      for (const reply of fixtures.receive(message)) {
        const json = toJson(ServerMessageSchema, reply) as JsonObject
        const snapshot = json.snapshot as JsonObject | undefined
        socket.pushRaw(JSON.stringify(snapshot ? { snapshot: answer(topic, snapshot) } : json))
      }
    }
    socket.push({ case: 'heartbeat', value: { serverTimeMs: status.headBlockTimeMs + 4_000n, headBlockNumber: status.headBlockNumber } })
  })
  if (reveal) act(() => void fireEvent.click(screen.getByRole('button', { name: reveal })))
  return errors
}

/** The snapshot's payload: its field's JSON name, message and value. */
function payloadOf(snapshot: JsonObject): { key: string; desc: DescMessage; json: JsonObject } | undefined {
  for (const field of SnapshotSchema.fields) {
    const json = snapshot[field.jsonName]
    if (field.fieldKind === 'message' && json && typeof json === 'object' && !Array.isArray(json)) return { key: field.jsonName, desc: field.message, json }
  }
  return undefined
}

/** What goes wrong when each topic `tree` subscribes to sends each hostile variant of its snapshot. */
function hostileFailures(tree: () => ReactElement, reveal?: string): string[] {
  const failures: string[] = []
  const check = (label: string, errors: Error[]) => {
    for (const error of errors) failures.push(`${label} threw ${error.message}`)
    const shown = /NaN|Infinity|undefined/.exec(document.body.textContent ?? '')
    if (shown) failures.push(`${label} showed "${shown[0]}"`)
    cleanup()
  }
  const snapshots = new Map<string, JsonObject>()
  check('the fixtures', show(tree(), reveal, (topic, snapshot) => (snapshots.set(topic, snapshot), snapshot)))
  for (const [topic, snapshot] of snapshots) {
    const payload = payloadOf(snapshot)
    if (!payload) continue
    for (const variant of hostileVariants(payload.desc, payload.json)) {
      const errors = show(tree(), reveal, (t, s) => (t === topic ? { ...s, [payload.key]: variant.json } : s))
      check(`${topic}: ${variant.change}`, errors)
    }
  }
  return failures
}

describe('every streamed region, on hostile contract values', () => {
  beforeEach(() => {
    // React reports each caught error; the test collects them itself.
    vi.spyOn(console, 'error').mockImplementation(() => undefined)
  })
  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
  })

  const pages = new Map<Region['render'], Region[]>()
  for (const region of regions) pages.set(region.render, [...(pages.get(region.render) ?? []), region])

  it.each([...pages.values()].map((page) => [page.map((region) => region.name).join(', '), page[0]] as const))(
    'shows %s without throwing or printing NaN',
    (_names, region) => {
      expect(hostileFailures(region.render, region.reveal)).toEqual([])
    },
  )

  const topic = `token:${canonicalWorkspaceAddress}`

  it('fails a region that parses an amount without a guard', () => {
    function Unguarded() {
      const { data } = useTokenStream(canonicalWorkspaceAddress)
      return <span>{data ? new Decimal(data.displayPriceUsd).toFixed(2) : 'Loading'}</span>
    }
    expect(hostileFailures(() => <Unguarded />)).toContain(`${topic}: TokenSnapshot.displayPriceUsd = "abc" threw [DecimalError] Invalid argument: abc`)
  })

  it('fails a region that prints NaN', () => {
    function Unchecked() {
      const { data } = useTokenStream(canonicalWorkspaceAddress)
      return <span>{data ? Number(data.marketCapUsd).toFixed(0) : 'Loading'}</span>
    }
    expect(hostileFailures(() => <Unchecked />)).toContain(`${topic}: TokenSnapshot.marketCapUsd = "abc" showed "NaN"`)
  })
})

const storySources = import.meta.glob<string>('./**/*.stories.tsx', { query: '?raw', import: 'default', eager: true })

const storyName = (state: string) => `${state[0]?.toUpperCase() ?? ''}${state.slice(1)}`

/** Each state a region shows that its stories file has no story for. */
function missingStories(list: Region[], sources: Record<string, string>): string[] {
  return list.flatMap((region) => {
    const source = sources[`./${region.stories}`]
    if (source === undefined) return [`${region.name} has no stories file at ${region.stories}`]
    const stories = new Set([...source.matchAll(/^export const (\w+)/gm)].map((match) => match[1]))
    return [...streamStates, ...region.extraStates]
      .map(storyName)
      .filter((story) => !stories.has(story))
      .map((story) => `${region.name} has no ${story} story`)
  })
}

describe('every streamed region has a story for each state it shows', () => {
  it('in its stories file', () => {
    expect(missingStories(regions, storySources)).toEqual([])
  })

  it('fails naming the region and the state when one is missing', () => {
    const file = './features/token-workspace/TokenTradesTape.stories.tsx'
    const withoutStale = { ...storySources, [file]: storySources[file]!.replace(/^export const Stale\b.*$/m, '') }
    expect(missingStories(regions, withoutStale)).toEqual(['token trades has no Stale story'])
  })
})

const featureSources = import.meta.glob<string>('./features/**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true })

/** Components under `features/` that read the stream, through any hook, but aren't in the manifest. */
function unlistedComponents(sources: Record<string, string>, list: Region[]): string[] {
  const hooks = new Set([...hooksSource.matchAll(/^export function (use\w+)/gm)].map((match) => match[1]!))
  const calls = (source: string) => [...hooks].some((hook) => new RegExp(`\\b${hook}\\(`).test(source))
  // A hook defined beside a call to a stream hook reads the stream too.
  for (let grew = true; grew; ) {
    grew = false
    for (const source of Object.values(sources)) {
      if (!calls(source)) continue
      for (const [, hook] of source.matchAll(/function (use\w+)/g)) {
        if (!hooks.has(hook!)) {
          hooks.add(hook!)
          grew = true
        }
      }
    }
  }
  const listed = new Set(list.map((region) => region.component))
  return Object.entries(sources)
    .map(([path, source]) => [path.slice('./'.length), source] as const)
    .filter(([path]) => path.endsWith('.tsx') && !/\.(test|stories)\.tsx$/.test(path))
    .filter(([path, source]) => calls(source) && !listed.has(path))
    .map(([path]) => path)
}

describe('the region manifest', () => {
  it('lists every component under features/ that reads the stream', () => {
    expect(unlistedComponents(featureSources, regions)).toEqual([])
  })

  it('fails a new component that reads the stream outside it, naming its file', () => {
    const added = { ...featureSources, './features/orders/OrdersPanel.tsx': 'export function OrdersPanel() {\n  const { data } = useAccountStream()\n}\n' }
    expect(unlistedComponents(added, regions)).toEqual(['features/orders/OrdersPanel.tsx'])
  })
})
