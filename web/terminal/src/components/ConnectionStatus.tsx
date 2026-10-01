import type { DataSourceKind } from '../api/source'
import type { ConnectionState } from '../api/stream/state'

type Tone = 'green' | 'blue' | 'amber' | 'red'

type Presentation = { label: string; tone: Tone; detail: string }

const sourceNames: Record<DataSourceKind, Presentation> = {
  live: { label: 'Live', tone: 'green', detail: 'Streaming from the live API.' },
  replay: { label: 'Replay', tone: 'blue', detail: 'Streaming a recorded session from the replay server, not the live chain.' },
  fixtures: { label: 'Fixtures', tone: 'blue', detail: 'Showing built-in sample data, not the chain.' },
}

/**
 * What the header says about the market-data connection (#63). Text and dot, never colour alone.
 * Once the engine's status arrives, its own mode decides Live or Replay; fixtures stay fixtures.
 */
function presentConnection(source: DataSourceKind, state: ConnectionState, engineMode?: EngineModeLabel): Presentation {
  switch (state) {
    case 'open':
      return sourceNames[source === 'fixtures' ? 'fixtures' : (engineMode ?? source)]
    case 'idle':
    case 'connecting':
      return { label: 'Connecting', tone: 'amber', detail: 'Connecting to market data.' }
    case 'stale':
      return { label: 'Stale', tone: 'amber', detail: 'No heartbeat for 3 seconds: prices on screen may be old.' }
    case 'reconnecting':
      return { label: 'Reconnecting', tone: 'amber', detail: 'Connection lost. Reconnecting; prices on screen may be old.' }
    case 'unavailable':
      return { label: 'Unavailable', tone: 'red', detail: 'Market data is unavailable. Still retrying.' }
  }
}

export type EngineModeLabel = 'live' | 'replay'

export type ConnectionStatusProps = {
  source: DataSourceKind
  state: ConnectionState
  /** The mode the engine reports on the `status` topic, once it has. */
  engineMode?: EngineModeLabel
}

export function ConnectionStatus({ source, state, engineMode }: ConnectionStatusProps) {
  const { label, tone, detail } = presentConnection(source, state, engineMode)
  return (
    <span
      className={`connection-status connection-status--${tone}`}
      role="status"
      aria-label={`Market data: ${label}. ${detail}`}
      title={detail}
      data-state={state}
    >
      <span className={`status-dot status-dot--${tone}`} aria-hidden="true" />
      <span>{label}</span>
    </span>
  )
}
