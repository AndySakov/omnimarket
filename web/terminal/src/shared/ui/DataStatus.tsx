import type { DataState } from '../../api/stream/dataState'
import { dataStateLabels } from '../../api/stream/dataState'

const tones: Record<DataState, 'green' | 'amber' | 'red' | 'muted'> = {
  loading: 'muted',
  live: 'green',
  stale: 'amber',
  reconnecting: 'amber',
  unavailable: 'red',
}

const details: Record<DataState, string> = {
  loading: 'Waiting for the first data.',
  live: 'Up to date.',
  stale: 'No heartbeat for 3 seconds: these numbers may be old.',
  reconnecting: 'Connection lost: these numbers are from before the drop.',
  unavailable: 'This data is unavailable right now.',
}

/**
 * The state of one region's streamed data (#63): every region that shows streamed numbers shows
 * this next to them, so old numbers are never passed off as current.
 */
export function DataStatus({ state, label }: { state: DataState; label?: string }) {
  const text = dataStateLabels[state]
  return (
    <span
      className={`data-status data-status--${tones[state]}`}
      role="status"
      aria-label={`${label ? `${label}: ` : ''}${text}. ${details[state]}`}
      title={details[state]}
      data-state={state}
    >
      <span className="status-dot" aria-hidden="true" />
      <span>{text}</span>
    </span>
  )
}
