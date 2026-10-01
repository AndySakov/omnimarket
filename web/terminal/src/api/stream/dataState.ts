// What the UI says about a piece of streamed data (#63): it is never shown as current when it
// might not be. A region shows its data state next to its numbers.

import type { ConnectionState, TopicState } from './state'

export type DataState =
  /** Nothing to show yet. */
  | 'loading'
  /** Current: snapshot applied, deltas flowing, heartbeats arriving. */
  | 'live'
  /** On screen, but heartbeats stopped: the numbers may be old. */
  | 'stale'
  /** On screen from before a drop, waiting for the new connection's snapshot. */
  | 'reconnecting'
  /** The server can't serve it, or the connection keeps failing. */
  | 'unavailable'

export function dataStateOf(connection: ConnectionState, topic: TopicState | undefined): DataState {
  if (topic?.phase === 'unavailable' || connection === 'unavailable') return 'unavailable'
  if (connection === 'reconnecting' || topic?.phase === 'resubscribing') return 'reconnecting'
  if (connection === 'idle' || connection === 'connecting' || !topic || topic.phase === 'loading') return 'loading'
  if (connection === 'stale') return 'stale'
  return 'live'
}

export const dataStateLabels: Record<DataState, string> = {
  loading: 'Loading',
  live: 'Live',
  stale: 'Stale',
  reconnecting: 'Reconnecting',
  unavailable: 'Unavailable',
}
