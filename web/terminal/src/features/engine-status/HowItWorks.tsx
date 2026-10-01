import { Dialog } from 'radix-ui'
import { ArrowRight, Info, X } from 'lucide-react'
import type { DataSourceKind } from '../../api/source'
import { EngineStatusDetails } from './EngineStatusBar'
import { useEngineStatus } from './useEngineStatus'
import './engine-status.css'

const REPO = 'https://github.com/AndySakov/omnimarket'

// Each stage of the data path, as built today. Keep in step with README.md and the D-entries
// cited: this copy describes the backend, so it must match what's built (#71).
const stages = [
  { name: 'Chain engine', detail: 'Follows Base block by block and keeps Uniswap v2 and v3 pool state in memory. Records every input, so any run replays exactly.' },
  { name: 'Kafka', detail: 'Carries the engine’s records, each with lineage IDs: pool updates now; trades and status being built (#77, #79).' },
  { name: 'API', detail: 'Serves those records over REST and one WebSocket stream (being built: #78).' },
  { name: 'Terminal', detail: 'This page: subscribes to what’s on screen and marks anything old as stale.' },
]

const sourceNotes: Record<DataSourceKind, string> = {
  fixtures: 'This build shows built-in sample data, not the chain.',
  replay: 'This build is playing back a recorded session, not the live chain.',
  live: 'This build is connected to the live API.',
}

/** The header's "How it works" button and the view it opens (#71), connected to the stream. */
export function HowItWorks() {
  const { status, state, source } = useEngineStatus()
  return (
    <Dialog.Root>
      <Dialog.Trigger asChild>
        <button className="utility-button how-it-works-trigger" type="button" aria-label="How it works">
          <Info size={15} aria-hidden="true" />
          <span className="how-it-works-trigger__label" aria-hidden="true">How it works</span>
        </button>
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Overlay className="how-it-works-overlay" />
        <Dialog.Content className="how-it-works">
          <HowItWorksContent source={source}>
            {status ? (
              <EngineStatusDetails status={status} state={state} source={source} />
            ) : (
              <p>{state === 'loading' ? 'Waiting for the engine’s first status.' : 'The engine’s status is unavailable right now.'}</p>
            )}
          </HowItWorksContent>
          <Dialog.Close asChild>
            <button className="icon-button how-it-works__close" type="button" aria-label="Close">
              <X size={16} aria-hidden="true" />
            </button>
          </Dialog.Close>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

/** The view's copy. `children` is the engine's status right now. */
export function HowItWorksContent({ source, children }: { source: DataSourceKind; children: React.ReactNode }) {
  return (
    <>
      <Dialog.Title>How it works</Dialog.Title>
      <Dialog.Description>
        OmniMarket is a proof of concept: the backend a trading terminal would need on EVM chains, starting with
        Base. This terminal is a window onto it. {sourceNotes[source]}
      </Dialog.Description>

      <h3>Live and shadow</h3>
      <div className="how-it-works__split">
        <section className="how-it-works__card how-it-works__card--live" aria-labelledby="how-live">
          <h4 id="how-live"><span className="status-dot status-dot--green" aria-hidden="true" />Live</h4>
          <ul>
            <li>Base market data: the engine reads every block and keeps pool state current.</li>
            <li>Prices and quotes, computed in memory from that state (D21; being built: #76, #83).</li>
          </ul>
        </section>
        <section className="how-it-works__card how-it-works__card--shadow" aria-labelledby="how-shadow">
          <h4 id="how-shadow"><span className="status-dot status-dot--amber" aria-hidden="true" />Shadow</h4>
          <ul>
            <li>Execution is simulated: trades and trigger orders run against live chain data and stop just before sending (being built: #84, #86).</li>
            <li>Nothing is broadcast and no real funds move. Balances, trades and receipts say “shadow”.</li>
          </ul>
        </section>
      </div>

      <h3>The data path</h3>
      <ol className="how-it-works__flow" aria-label="Data path from the chain to this terminal">
        {stages.map((stage, index) => (
          <li key={stage.name}>
            <div className="how-it-works__stage">
              <strong>{stage.name}</strong>
              <span>{stage.detail}</span>
            </div>
            {index < stages.length - 1 && <ArrowRight className="how-it-works__arrow" size={16} aria-hidden="true" />}
          </li>
        ))}
      </ol>

      <h3>The engine right now</h3>
      {children}

      <h3>Read more</h3>
      <ul className="how-it-works__links">
        <li><a href={`${REPO}#readme`} target="_blank" rel="noreferrer">The project README</a></li>
        <li><a href={`${REPO}/blob/main/docs/spec/decisions.md`} target="_blank" rel="noreferrer">The decision log</a></li>
        <li>
          <a href={`${REPO}/pull/52`} target="_blank" rel="noreferrer">#52: a live measurement of the engine</a>
          {' '}(15 minutes on Base: 576 pools, 450 blocks, all 3,445 spot checks matched, identical replay)
        </li>
      </ul>
    </>
  )
}
