// Discover's states (#65), each on a stream manager whose store holds the fixture feed. No socket
// and no timers run, so a story stays in the state it names.
import type { Meta, StoryObj } from '@storybook/react-vite'
import { useEffect } from 'react'
import { create, type MessageInitShape } from '@bufbuild/protobuf'
import { ServerMessageSchema } from '../../api/generated/omnimarket/api/v1/stream_pb'
import { StreamManager } from '../../api/stream/manager'
import { applyServerMessage, type ConnectionState } from '../../api/stream/state'
import { StreamProvider } from '../../api/stream/StreamProvider'
import { DISCOVERY_TOPIC } from '../../api/stream/topics'
import { FIRST_NEW_POOL_TICK, FixtureDiscovery } from '../../mocks/api/discoveryFeed'
import { DiscoveryPage } from './DiscoveryPage'

const HEAD_MS = 1_790_000_000_000n
const HEAD_BLOCK = 36_120_450n

type Kind = MessageInitShape<typeof ServerMessageSchema>['kind']

type FeedSetup = {
  connection: ConnectionState
  /** Serve the snapshot; without it the table is loading. */
  snapshot: boolean
  /** Insert the first new pool after the snapshot. */
  insert?: boolean
}

function withFeed({ connection, snapshot, insert = false }: FeedSetup) {
  return (Story: () => React.JSX.Element) => {
    const manager = new StreamManager({ url: 'ws://storybook.invalid/v1/stream' })
    // Subscribe before the page does, so its own subscribe keeps the state set here.
    manager.subscribe(DISCOVERY_TOPIC)
    const feed = new FixtureDiscovery(HEAD_MS, HEAD_BLOCK)
    const now = HEAD_MS + 4_000n
    const kinds: Kind[] = []
    if (snapshot) {
      kinds.push({ case: 'snapshot', value: { topic: DISCOVERY_TOPIC, seq: 1n, payload: { case: 'discovery', value: feed.snapshot() } } })
      kinds.push({ case: 'heartbeat', value: { serverTimeMs: now, headBlockNumber: HEAD_BLOCK } })
    }
    apply(manager, kinds)
    manager.store.setState({ connection })
    const inserts: Kind[] = []
    if (insert) {
      let seq = 1n
      for (const payload of feed.tick(FIRST_NEW_POOL_TICK, now)) {
        seq += 1n
        inserts.push({ case: 'delta', value: { topic: DISCOVERY_TOPIC, seq, payload } })
      }
    }
    return (
      <StreamProvider manager={manager} source="fixtures">
        <Story />
        <AfterMount run={() => apply(manager, inserts)} />
      </StreamProvider>
    )
  }
}

function apply(manager: StreamManager, kinds: Kind[]) {
  let state = manager.store.getState()
  for (const kind of kinds) state = applyServerMessage(state, create(ServerMessageSchema, { kind })).state
  manager.store.setState(state)
}

/** Runs once the page has shown the snapshot, so an inserted row arrives as it would live. */
function AfterMount({ run }: { run: () => void }) {
  useEffect(run, [run])
  return null
}

const meta = {
  title: 'Features/Discover',
  component: DiscoveryPage,
} satisfies Meta<typeof DiscoveryPage>

export default meta
type Story = StoryObj<typeof meta>

export const Loading: Story = { decorators: [withFeed({ connection: 'connecting', snapshot: false })] }
export const Live: Story = { decorators: [withFeed({ connection: 'open', snapshot: true })] }
export const Inserting: Story = { decorators: [withFeed({ connection: 'open', snapshot: true, insert: true })] }
export const Stale: Story = { decorators: [withFeed({ connection: 'stale', snapshot: true })] }
export const Empty: Story = { args: { state: 'empty' }, decorators: [withFeed({ connection: 'open', snapshot: true })] }
export const Error: Story = { args: { state: 'error' }, decorators: [withFeed({ connection: 'open', snapshot: true })] }
