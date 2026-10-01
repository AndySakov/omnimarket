import { useState } from 'react'
import { DiscoveryPage } from '../features/discovery-feed/DiscoveryPage'
import { ApiProvider } from './ApiProvider'
import { TerminalShell } from './layout/TerminalShell'
import type { ApiServices } from './services'

export default function TerminalApp({ services }: { services: ApiServices }) {
  const [activeRoute, setActiveRoute] = useState('Discover')

  return (
    <ApiProvider services={services}>
      <TerminalShell activeRoute={activeRoute} onNavigate={setActiveRoute}>
        {activeRoute === 'Discover' ? (
          <DiscoveryPage />
        ) : (
          <main className="shell-preview" aria-label={`${activeRoute} workspace preview`}>
            <div className="shell-preview__eyebrow">{activeRoute} workspace</div>
            <h1>This workspace is queued for a later phase.</h1>
            <p>
              OmniMarket starts with market discovery. The next route will be added against the
              same shell and token system.
            </p>
          </main>
        )}
      </TerminalShell>
    </ApiProvider>
  )
}
