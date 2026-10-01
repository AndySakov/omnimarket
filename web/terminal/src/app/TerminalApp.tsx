import { useState } from 'react'
import { DiscoveryPage } from '../features/discovery-feed/DiscoveryPage'
import { TokenPage } from '../features/token-workspace/TokenPage'
import type { DiscoveryToken } from '../domains/market/token'
import { TerminalShell } from './layout/TerminalShell'

export default function TerminalApp() {
  const [activeRoute, setActiveRoute] = useState('Discover')
  const [selectedTokenId, setSelectedTokenId] = useState<string | null>(null)

  function handleNavigate(route: string) {
    setActiveRoute(route)
    if (route !== 'Token') setSelectedTokenId(null)
  }

  function handleTokenOpen(token: DiscoveryToken) {
    setSelectedTokenId(token.id)
    setActiveRoute('Token')
  }

  return (
    <TerminalShell activeRoute={activeRoute === 'Token' ? 'Discover' : activeRoute} onNavigate={handleNavigate}>
      {activeRoute === 'Discover' ? (
        <DiscoveryPage onTokenOpen={handleTokenOpen} />
      ) : activeRoute === 'Token' ? (
        <TokenPage tokenId={selectedTokenId ?? undefined} onBack={() => handleNavigate('Discover')} />
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
  )
}
