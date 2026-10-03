import { useEffect, useState } from 'react'
import { DiscoveryPage } from '../features/discovery-feed/DiscoveryPage'
import { TokenPage } from '../features/token-workspace/TokenPage'
import type { DiscoveryToken } from '../domains/market/token'
import { getTokenRouteAddress } from '../features/token-workspace/tokenWorkspaceData'
import { TerminalShell } from './layout/TerminalShell'

function readRoute(pathname: string) {
  const tokenMatch = pathname.match(/^\/base\/token\/([^/]+)\/?$/i)
  if (tokenMatch?.[1]) {
    const rawAddress = decodeURIComponent(tokenMatch[1]).toLowerCase()
    return { activeRoute: 'Token', selectedTokenAddress: getTokenRouteAddress(rawAddress) }
  }

  const workspaceMatch = pathname.match(/^\/(portfolio|trackers|wallets|settings)\/?$/i)
  const activeRoute = workspaceMatch?.[1]
    ? workspaceMatch[1][0].toUpperCase() + workspaceMatch[1].slice(1)
    : 'Discover'
  return { activeRoute, selectedTokenAddress: null }
}

export default function TerminalApp() {
  const initialRoute = readRoute(window.location.pathname)
  const [activeRoute, setActiveRoute] = useState(initialRoute.activeRoute)
  const [selectedTokenAddress, setSelectedTokenAddress] = useState<string | null>(initialRoute.selectedTokenAddress)

  useEffect(() => {
    function handlePopState() {
      const route = readRoute(window.location.pathname)
      setActiveRoute(route.activeRoute)
      setSelectedTokenAddress(route.selectedTokenAddress)
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
    if (route !== 'Token') setSelectedTokenAddress(null)
  }

  function handleTokenOpen(token: DiscoveryToken) {
    const address = token.live?.address ?? getTokenRouteAddress(token.id)
    setSelectedTokenAddress(address)
    setActiveRoute('Token')
    pushPath(`/base/token/${encodeURIComponent(address)}`)
  }

  return (
    <TerminalShell activeRoute={activeRoute === 'Token' ? 'Discover' : activeRoute} onNavigate={handleNavigate}>
      {activeRoute === 'Discover' ? (
        <DiscoveryPage onTokenOpen={handleTokenOpen} />
      ) : activeRoute === 'Token' ? (
        <TokenPage address={selectedTokenAddress ?? undefined} onBack={() => handleNavigate('Discover')} />
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
