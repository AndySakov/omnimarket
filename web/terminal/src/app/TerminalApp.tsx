import { useEffect, useState } from 'react'
import { DiscoveryPage } from '../features/discovery-feed/DiscoveryPage'
import { TokenPage } from '../features/token-workspace/TokenPage'
import type { DiscoveryToken } from '../domains/market/token'
import { getTokenRouteAddress, resolveTokenIdByAddress } from '../features/token-workspace/tokenWorkspaceData'
import { TerminalShell } from './layout/TerminalShell'

function readRoute(pathname: string) {
  const tokenMatch = pathname.match(/^\/base\/token\/([^/]+)\/?$/i)
  if (tokenMatch?.[1]) {
    return { activeRoute: 'Token', selectedTokenId: resolveTokenIdByAddress(decodeURIComponent(tokenMatch[1])) ?? '__missing__' }
  }

  const workspaceMatch = pathname.match(/^\/(portfolio|trackers|wallets|settings)\/?$/i)
  const activeRoute = workspaceMatch?.[1]
    ? workspaceMatch[1][0].toUpperCase() + workspaceMatch[1].slice(1)
    : 'Discover'
  return { activeRoute, selectedTokenId: null }
}

export default function TerminalApp() {
  const initialRoute = readRoute(window.location.pathname)
  const [activeRoute, setActiveRoute] = useState(initialRoute.activeRoute)
  const [selectedTokenId, setSelectedTokenId] = useState<string | null>(initialRoute.selectedTokenId)

  useEffect(() => {
    function handlePopState() {
      const route = readRoute(window.location.pathname)
      setActiveRoute(route.activeRoute)
      setSelectedTokenId(route.selectedTokenId)
    }

    window.addEventListener('popstate', handlePopState)
    return () => window.removeEventListener('popstate', handlePopState)
  }, [])

  function pushPath(path: string) {
    if (window.location.pathname === path) return
    window.history.pushState({}, '', path)
  }

  function handleNavigate(route: string) {
    pushPath(route === 'Discover' ? '/' : `/${route.toLowerCase()}`)
    setActiveRoute(route)
    if (route !== 'Token') setSelectedTokenId(null)
  }

  function handleTokenOpen(token: DiscoveryToken) {
    setSelectedTokenId(token.id)
    setActiveRoute('Token')
    pushPath(`/base/token/${encodeURIComponent(getTokenRouteAddress(token.id))}`)
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
