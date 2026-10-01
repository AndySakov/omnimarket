import { Tooltip } from 'radix-ui'
import type { EngineStatus } from '../../api/generated/omnimarket/api/v1/market_pb'
import type { DataSourceKind } from '../../api/source'
import { dataStateLabels, type DataState } from '../../api/stream/dataState'
import { presentEngineStatus } from './engineStatus'
import { useEngineStatus } from './useEngineStatus'
import './engine-status.css'

/** The utility rail's engine status (#71), connected to the `status` topic. */
export function EngineStatusBar() {
  const { status, state, source } = useEngineStatus()
  return <EngineStatusBarView status={status} state={state} source={source} />
}

export type EngineStatusBarViewProps = {
  status: EngineStatus | undefined
  state: DataState
  source: DataSourceKind
}

const stateDetails: Record<DataState, string> = {
  loading: 'Waiting for the engine’s first status.',
  live: 'Updates every block.',
  stale: 'No new block for a while: these numbers may be old.',
  reconnecting: 'Connection lost: these numbers are from before the drop.',
  unavailable: 'The engine’s status is unavailable right now.',
}

export function EngineStatusBarView({ status, state, source }: EngineStatusBarViewProps) {
  if (!status) {
    const text = state === 'loading' ? 'Engine status loading' : `Engine status ${dataStateLabels[state].toLowerCase()}`
    return (
      <span className={`engine-status engine-status--${state}`} data-state={state} aria-label={`${text}. ${stateDetails[state]}`}>
        <span className="status-dot" aria-hidden="true" />
        <span>{text}</span>
      </span>
    )
  }

  const view = presentEngineStatus(status, source)
  // Live and Replay name the engine's mode; any other state says what's wrong instead.
  const badge = state === 'live' ? view.mode : dataStateLabels[state]
  const summary =
    `Engine: ${badge}. ${view.chain}, block ${view.block}, lag ${view.lag}, ${view.pools} pools, ` +
    `shadow checks ${view.shadowChecks} agreed. ${stateDetails[state]}`

  return (
    <Tooltip.Provider delayDuration={200}>
      <Tooltip.Root>
        <Tooltip.Trigger asChild>
          <button
            className={`engine-status engine-status--${state} engine-status--mode-${view.mode.toLowerCase()}`}
            type="button"
            data-state={state}
            aria-label={summary}
          >
            <span className="engine-status__badge">
              <span className="status-dot" aria-hidden="true" />
              {badge}
            </span>
            <span className="engine-status__item"><strong>{view.chain}</strong></span>
            <span className="engine-status__item"><span>Block</span><strong>{view.block}</strong></span>
            <span className="engine-status__item"><span>Lag</span><strong>{view.lag}</strong></span>
            <span className="engine-status__item"><span>Pools</span><strong>{view.pools}</strong></span>
            <span className="engine-status__item"><span>Shadow checks</span><strong>{view.shadowChecks}</strong></span>
          </button>
        </Tooltip.Trigger>
        <Tooltip.Portal>
          <Tooltip.Content className="engine-status-tooltip" side="bottom" align="end" sideOffset={6}>
            <EngineStatusDetails status={status} source={source} state={state} />
            <Tooltip.Arrow className="engine-status-tooltip__arrow" />
          </Tooltip.Content>
        </Tooltip.Portal>
      </Tooltip.Root>
    </Tooltip.Provider>
  )
}

/** The detail behind the bar: in its tooltip, and in the "How it works" view. */
export function EngineStatusDetails({ status, source, state }: EngineStatusBarViewProps & { status: EngineStatus }) {
  const view = presentEngineStatus(status, source)
  return (
    <div className="engine-status-details">
      <p className="engine-status-details__state">{stateDetails[state]}</p>
      <dl>
        <dt>Mode</dt>
        <dd>{view.mode}</dd>
        <dt>Core instance</dt>
        <dd>{view.coreInstance}</dd>
        <dt>Recording</dt>
        <dd>{view.recording}</dd>
        <dt>Uptime</dt>
        <dd>{view.uptime}</dd>
        <dt>Head</dt>
        <dd>{view.block} ({view.headHash})</dd>
        <dt>Lag to head</dt>
        <dd>{view.lag}</dd>
        {view.venues.map((venue) => (
          <div className="engine-status-details__row" key={venue.venue}>
            <dt>{venue.venue}</dt>
            <dd>{venue.known} pools, {venue.active} active</dd>
          </div>
        ))}
        <dt>Shadow checks</dt>
        <dd>{view.shadowChecksDetail}</dd>
      </dl>
    </div>
  )
}
