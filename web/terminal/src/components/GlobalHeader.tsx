import { useState } from 'react'
import {
  BarChart3,
  ChevronDown,
  Command,
  History,
  Menu,
  Search,
  Settings,
  Star,
  X,
} from 'lucide-react'
import { ConnectionStatus, type ConnectionStatusProps } from './ConnectionStatus'

const routes = ['Discover', 'Portfolio', 'Trackers', 'Wallets', 'Settings']

type GlobalHeaderProps = {
  activeRoute: string
  onNavigate: (route: string) => void
  /** The market-data connection the status indicator shows. */
  connection: ConnectionStatusProps
}

export function GlobalHeader({ activeRoute, onNavigate, connection }: GlobalHeaderProps) {
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const [searchOpen, setSearchOpen] = useState(false)

  function handleNavigate(route: string) {
    onNavigate(route)
    setMobileMenuOpen(false)
  }

  return (
    <header className="global-header">
      <div className="global-header__primary">
        <div className="global-header__identity">
          <button className="brand" type="button" onClick={() => handleNavigate('Discover')}>
            <span className="brand__mark" aria-hidden="true">
              <img src="/assets/omnimarket-logo.png" alt="" />
            </span>
            <span className="brand__name">OmniMarket</span>
          </button>
          <span className="brand__divider" aria-hidden="true" />
          <nav className="primary-nav" aria-label="Primary navigation">
            {routes.map((route) => (
              <button
                className={`nav-link ${activeRoute === route ? 'nav-link--active' : ''}`}
                key={route}
                type="button"
                aria-current={activeRoute === route ? 'page' : undefined}
                onClick={() => handleNavigate(route)}
              >
                {route}
              </button>
            ))}
          </nav>
        </div>

        <div className="global-header__actions">
          <button
            className="icon-button mobile-control"
            type="button"
            aria-label={searchOpen ? 'Close search' : 'Open search'}
            aria-expanded={searchOpen}
            onClick={() => setSearchOpen((open) => !open)}
          >
            {searchOpen ? <X size={17} /> : <Search size={17} />}
          </button>
          <SearchControl mobileOpen={searchOpen} />
          <ChainSelector />
          <ConnectionStatus source={connection.source} state={connection.state} />
          <button className="button button--primary" type="button">
            Deposit
          </button>
          <button className="icon-button" type="button" aria-label="Open favorites">
            <Star size={17} />
          </button>
          <WalletMenu />
          <button
            className="icon-button mobile-control"
            type="button"
            aria-label={mobileMenuOpen ? 'Close navigation menu' : 'Open navigation menu'}
            aria-expanded={mobileMenuOpen}
            onClick={() => setMobileMenuOpen((open) => !open)}
          >
            {mobileMenuOpen ? <X size={18} /> : <Menu size={18} />}
          </button>
        </div>
      </div>

      {mobileMenuOpen && (
        <nav className="mobile-nav" aria-label="Mobile navigation">
          {routes.map((route) => (
            <button
              className={`mobile-nav__link ${activeRoute === route ? 'mobile-nav__link--active' : ''}`}
              key={route}
              type="button"
              aria-current={activeRoute === route ? 'page' : undefined}
              onClick={() => handleNavigate(route)}
            >
              <span>{route}</span>
              {activeRoute === route && <span className="status-dot status-dot--blue" aria-hidden="true" />}
            </button>
          ))}
        </nav>
      )}

      <UtilityRail />
    </header>
  )
}

function SearchControl({ mobileOpen }: { mobileOpen: boolean }) {
  return (
    <label className={`search-control ${mobileOpen ? 'search-control--mobile-open' : ''}`}>
      <Search className="search-control__icon" size={16} aria-hidden="true" />
      <span className="sr-only">Search by token or contract address</span>
      <input placeholder="Search token or contract address" type="search" />
      <span className="search-control__shortcut" aria-hidden="true">
        <Command size={12} />
        <span>K</span>
      </span>
    </label>
  )
}

function ChainSelector() {
  return (
    <button className="context-control" type="button" aria-label="Select chain">
      <img className="chain-mark" src="/assets/chains/base.svg" alt="" />
      <span>Base</span>
      <ChevronDown size={14} aria-hidden="true" />
    </button>
  )
}

function WalletMenu() {
  return (
    <button className="wallet-menu" type="button" aria-label="Open wallet menu">
      <span className="wallet-avatar" aria-hidden="true">OM</span>
      <span className="wallet-menu__details">
        <span className="wallet-menu__address">0x3a7e…9c21</span>
        <span className="wallet-menu__balance">$12,430.50</span>
      </span>
      <ChevronDown size={14} aria-hidden="true" />
    </button>
  )
}

function UtilityRail() {
  return (
    <div className="utility-rail">
      <div className="utility-rail__tools" aria-label="Terminal utilities">
        <button className="utility-button" type="button" aria-label="Open terminal settings">
          <Settings size={15} />
        </button>
        <button className="utility-button" type="button" aria-label="Open saved favorites">
          <Star size={15} />
        </button>
        <button className="utility-button" type="button" aria-label="Open market activity">
          <BarChart3 size={15} />
        </button>
        <button className="utility-button" type="button" aria-label="Open history">
          <History size={15} />
        </button>
      </div>
      <div className="utility-rail__ticker" aria-label="Market status">
        <span className="market-live"><span className="status-dot status-dot--green" aria-hidden="true" />Live</span>
        <span className="ticker-item"><strong>BTC</strong><span>$63,241</span><span className="value-up">+1.24%</span></span>
        <span className="ticker-item"><strong>ETH</strong><span>$2,487.31</span><span className="value-down">−0.62%</span></span>
        <span className="ticker-item ticker-item--gas"><strong>Gas</strong><span className="value-up">0.21 Gwei</span></span>
      </div>
    </div>
  )
}
