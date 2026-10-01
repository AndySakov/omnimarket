import { useMemo, useState } from 'react'
import {
  Activity,
  ArrowDownUp,
  BadgeCheck,
  ChevronDown,
  CircleAlert,
  CircleCheck,
  CircleX,
  Clock3,
  Copy,
  Eye,
  EyeOff,
  Filter,
  Gauge,
  Globe2,
  Info,
  LockKeyhole,
  MoreHorizontal,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Users,
  WalletCards,
} from 'lucide-react'
import { discoveryTokens } from '../../mocks/discoveryFixtures'
import type { Chain, DiscoveryToken } from '../../domains/market/token'

const streams = ['Top', 'Trending', 'Radar', 'Callouts', 'Streamers'] as const
const timeframes = ['1m', '5m', '30m', '1h'] as const
const chains: Array<{ label: 'All' | Chain; tone: string }> = [
  { label: 'All', tone: 'all' },
  { label: 'Base', tone: 'base' },
  { label: 'BNB', tone: 'bnb' },
  { label: 'Solana', tone: 'solana' },
  { label: 'Ethereum', tone: 'ethereum' },
]

const chainAssets: Record<string, string> = {
  all: '/assets/chains/all.svg',
  base: '/assets/chains/base.svg',
  bnb: '/assets/chains/bnb.svg',
  solana: '/assets/chains/solana.svg',
  ethereum: '/assets/chains/ethereum.svg',
}

type DiscoverySurfaceState = 'ready' | 'loading' | 'empty' | 'error'

type DiscoveryPageProps = {
  state?: DiscoverySurfaceState
  onTokenOpen?: (token: DiscoveryToken) => void
}

export function DiscoveryPage({ state = 'ready', onTokenOpen }: DiscoveryPageProps) {
  const [activeStream, setActiveStream] = useState<(typeof streams)[number]>('Top')
  const [activeTimeframe, setActiveTimeframe] = useState<(typeof timeframes)[number]>('5m')
  const [activeChain, setActiveChain] = useState<'All' | Chain>('All')
  const [filtersOpen, setFiltersOpen] = useState(false)
  const [safetyOnly, setSafetyOnly] = useState(false)
  const [selectedTokenId, setSelectedTokenId] = useState<string | null>(null)
  const [tradeNotice, setTradeNotice] = useState('')
  const [activePreset, setActivePreset] = useState('P1')

  const visibleTokens = useMemo(() => {
    return discoveryTokens.filter((token) => {
      const matchesChain = activeChain === 'All' || token.chain === activeChain
      const matchesSafety = !safetyOnly || token.safety.state === 'passed'
      return matchesChain && matchesSafety
    })
  }, [activeChain, safetyOnly])

  function handleQuickBuy(token: DiscoveryToken) {
    setTradeNotice(`Buy draft ready for ${token.symbol}. Review is required before signing.`)
  }

  return (
    <main className="discovery-page" aria-label="Discover market radar">
      <h1 className="sr-only">Discover market radar</h1>
      <DiscoveryToolbar
        activeStream={activeStream}
        activeTimeframe={activeTimeframe}
        filtersOpen={filtersOpen}
        safetyOnly={safetyOnly}
        onStreamChange={setActiveStream}
        onTimeframeChange={setActiveTimeframe}
        onFiltersToggle={() => setFiltersOpen((open) => !open)}
        onSafetyToggle={() => setSafetyOnly((enabled) => !enabled)}
        activePreset={activePreset}
        onPresetChange={setActivePreset}
      />

      {filtersOpen && (
        <div className="discovery-filter-panel" role="region" aria-label="Discovery filters">
          <div className="filter-panel__group">
            <span className="filter-panel__label">Screening</span>
            <button
              className={`filter-chip ${safetyOnly ? 'filter-chip--active' : ''}`}
              type="button"
              aria-pressed={safetyOnly}
              onClick={() => setSafetyOnly((enabled) => !enabled)}
            >
              <ShieldCheck size={14} />
              Sell simulation passed
            </button>
            <button className="filter-chip" type="button">
              <Gauge size={14} />
              Liquidity above $100K
            </button>
          </div>
          <div className="filter-panel__group">
            <span className="filter-panel__label">Confidence</span>
            <span className="filter-panel__hint"><Info size={13} /> Provisional rows remain visible until excluded.</span>
          </div>
        </div>
      )}

      <div className="discovery-layout">
        <ChainRail activeChain={activeChain} onChainChange={setActiveChain} />
        <section className="discovery-table-shell" aria-label={`${activeStream} token stream`}>
          {state === 'loading' && <DiscoveryStatePanel type="loading" />}
          {state === 'empty' && <DiscoveryStatePanel type="empty" />}
          {state === 'error' && <DiscoveryStatePanel type="error" />}
          {state === 'ready' && visibleTokens.length === 0 && <DiscoveryStatePanel type="empty" />}
          {state === 'ready' && visibleTokens.length > 0 && (
            <TokenTable
              tokens={visibleTokens}
              selectedTokenId={selectedTokenId}
              onSelect={setSelectedTokenId}
              onOpen={onTokenOpen}
              onQuickBuy={handleQuickBuy}
            />
          )}
          <div className="discovery-table-footer" aria-live="polite">
            <span>
              <span className="status-dot status-dot--green" aria-hidden="true" />
              {visibleTokens.length} tokens in view · {activeStream} · {activeTimeframe} snapshot
            </span>
            {tradeNotice && <span className="trade-notice">{tradeNotice}</span>}
            <button className="table-footer__refresh" type="button" onClick={() => setTradeNotice('Market snapshot refreshed.') }>
              <RefreshCw size={13} /> Refresh
            </button>
          </div>
        </section>
      </div>
    </main>
  )
}

type DiscoveryToolbarProps = {
  activeStream: (typeof streams)[number]
  activeTimeframe: (typeof timeframes)[number]
  filtersOpen: boolean
  safetyOnly: boolean
  onStreamChange: (stream: (typeof streams)[number]) => void
  onTimeframeChange: (timeframe: (typeof timeframes)[number]) => void
  onFiltersToggle: () => void
  onSafetyToggle: () => void
  activePreset: string
  onPresetChange: (preset: string) => void
}

function DiscoveryToolbar({
  activeStream,
  activeTimeframe,
  filtersOpen,
  safetyOnly,
  onStreamChange,
  onTimeframeChange,
  onFiltersToggle,
  onSafetyToggle,
  activePreset,
  onPresetChange,
}: DiscoveryToolbarProps) {
  return (
    <div className="discovery-toolbar">
      <div className="stream-tabs" role="tablist" aria-label="Discovery streams">
        {streams.map((stream) => (
          <button
            className={`stream-tab ${activeStream === stream ? 'stream-tab--active' : ''}`}
            key={stream}
            type="button"
            role="tab"
            aria-selected={activeStream === stream}
            onClick={() => onStreamChange(stream)}
          >
            <span>{stream}</span>
            {(stream === 'Radar' || stream === 'Streamers') && <span className="new-badge">NEW</span>}
          </button>
        ))}
      </div>

      <div className="discovery-toolbar__controls">
        <div className="segmented-control" role="tablist" aria-label="Sparkline timeframe">
          {timeframes.map((timeframe) => (
            <button
              className={`segmented-control__item ${activeTimeframe === timeframe ? 'segmented-control__item--active' : ''}`}
              key={timeframe}
              type="button"
              role="tab"
              aria-selected={activeTimeframe === timeframe}
              onClick={() => onTimeframeChange(timeframe)}
            >
              {timeframe}
            </button>
          ))}
        </div>
        <button className="toolbar-icon-button" type="button" aria-label="Open discovery settings">
          <Settings2 size={16} />
        </button>
        <button className="toolbar-icon-button" type="button" aria-label="Toggle hidden market data">
          <EyeOff size={16} />
        </button>
        <button className="toolbar-icon-button" type="button" aria-label="Set refresh interval">
          <Clock3 size={16} />
          <ChevronDown size={12} />
        </button>
        <button className="toolbar-wallet-control" type="button" aria-label="Select trading wallet">
          <WalletCards size={15} />
          <span>1</span>
          <span className="toolbar-wallet-control__asset">◎ 0</span>
          <ChevronDown size={13} />
        </button>
        <button className={`toolbar-control ${filtersOpen ? 'toolbar-control--active' : ''}`} type="button" aria-pressed={filtersOpen} onClick={onFiltersToggle}>
          <Filter size={15} /> Filters
          {safetyOnly && <span className="toolbar-control__count">1</span>}
        </button>
        <button className={`toolbar-control ${safetyOnly ? 'toolbar-control--active' : ''}`} type="button" aria-pressed={safetyOnly} onClick={onSafetyToggle}>
          <SlidersHorizontal size={15} /> Screening
        </button>
        <div className="quick-buy-control" aria-label="Quick buy amount">
          <span>Quick Buy</span>
          <input aria-label="Quick buy amount" defaultValue="0.1" inputMode="decimal" />
          <span className="quick-buy-control__asset">ETH <ChevronDown size={13} /></span>
        </div>
        <div className="toolbar-presets" role="group" aria-label="Quick buy presets">
          {['P1', 'P2', 'P3'].map((preset) => (
            <button
              className={`toolbar-preset ${activePreset === preset ? 'toolbar-preset--active' : ''}`}
              key={preset}
              type="button"
              aria-pressed={activePreset === preset}
              onClick={() => onPresetChange(preset)}
            >
              {preset}
            </button>
          ))}
        </div>
      </div>
    </div>
  )
}

type ChainRailProps = {
  activeChain: 'All' | Chain
  onChainChange: (chain: 'All' | Chain) => void
}

function ChainRail({ activeChain, onChainChange }: ChainRailProps) {
  return (
    <aside className="chain-rail" aria-label="Filter by chain">
      {chains.map(({ label, tone }) => (
        <button
          className={`chain-rail__item ${activeChain === label ? 'chain-rail__item--active' : ''}`}
          key={label}
          type="button"
          aria-pressed={activeChain === label}
          onClick={() => onChainChange(label)}
        >
          <span className={`chain-rail__mark chain-rail__mark--${tone}`} aria-hidden="true">
            <img src={chainAssets[tone]} alt="" />
          </span>
          <span>{label}</span>
        </button>
      ))}
    </aside>
  )
}

type TokenTableProps = {
  tokens: DiscoveryToken[]
  selectedTokenId: string | null
  onSelect: (id: string) => void
  onOpen?: (token: DiscoveryToken) => void
  onQuickBuy: (token: DiscoveryToken) => void
}

function TokenTable({ tokens, selectedTokenId, onSelect, onOpen, onQuickBuy }: TokenTableProps) {
  return (
    <div className="discovery-table-scroll">
      <table className="discovery-table">
        <caption className="sr-only">Discoverable tokens with market and safety evidence</caption>
        <thead>
          <tr>
            <th scope="col">Pair info <ArrowDownUp size={13} aria-hidden="true" /></th>
            <th scope="col">Market cap <ArrowDownUp size={13} aria-hidden="true" /></th>
            <th scope="col">Liquidity <ArrowDownUp size={13} aria-hidden="true" /></th>
            <th scope="col">Volume ({'5m'}) <ArrowDownUp size={13} aria-hidden="true" /></th>
            <th scope="col">Txns ({'5m'}) <ArrowDownUp size={13} aria-hidden="true" /></th>
            <th scope="col">Token info</th>
            <th scope="col">Action</th>
          </tr>
        </thead>
        <tbody>
          {tokens.map((token, index) => (
            <TokenRow
              key={token.id}
              token={token}
              index={index + 1}
              selected={selectedTokenId === token.id}
              onSelect={() => onSelect(token.id)}
              onOpen={() => onOpen?.(token)}
              onQuickBuy={() => onQuickBuy(token)}
            />
          ))}
        </tbody>
      </table>
    </div>
  )
}

type TokenRowProps = {
  token: DiscoveryToken
  index: number
  selected: boolean
  onSelect: () => void
  onOpen: () => void
  onQuickBuy: () => void
}

function TokenRow({ token, index, selected, onSelect, onOpen, onQuickBuy }: TokenRowProps) {
  return (
    <tr className={selected ? 'token-row token-row--selected' : 'token-row'}>
      <td data-label="Pair info">
        <div className="pair-cell">
          <span className="row-index" aria-hidden="true">{index}</span>
          <TokenAvatar src={token.avatarSrc} tone={token.avatarTone} />
          <div className="pair-cell__body">
            <div className="pair-cell__title">
              <button className="token-name" type="button" onClick={() => { onSelect(); onOpen() }}>{token.name}</button>
              <span className="token-symbol">{token.symbol}</span>
              <button className="copy-button" type="button" aria-label={`Copy ${token.name} address`}><Copy size={13} /></button>
            </div>
            <div className="pair-cell__meta">
              <span className="token-age"><Clock3 size={13} />{token.age}</span>
              <span className={`chain-label chain-label--${token.chain.toLowerCase()}`}><span className="chain-label__dot" />{token.chain}</span>
              {token.provisional && <span className="confidence-label confidence-label--provisional"><Info size={12} />Provisional</span>}
            </div>
            <div className="pair-cell__signals" aria-label="Token context">
              <span className="pair-signal" role="img" aria-label="Verified market metadata" title="Verified market metadata"><BadgeCheck size={14} /></span>
              <span className="pair-signal" role="img" aria-label="Token website available" title="Token website available"><Globe2 size={14} /></span>
              <span className="pair-signal" role="img" aria-label="Token analytics available" title="Token analytics available"><Search size={14} /></span>
              <span className="pair-signal" role="img" aria-label="Token watch status" title="Token watch status"><Eye size={14} /></span>
            </div>
            <div className="token-tags">
              {token.tags.map((tag) => <span className="token-tag" key={tag}>{tag}</span>)}
            </div>
          </div>
        </div>
      </td>
      <td data-label="Market cap">
        <MarketCapCell token={token} />
      </td>
      <MetricCell label="Liquidity" value={token.liquidity} change={token.liquidityChange} />
      <td data-label="Volume">
        <div className="volume-cell">
          <span className="metric-value">{token.volume}</span>
          <span className={`metric-change ${token.volumeChange.startsWith('-') ? 'metric-change--negative' : 'metric-change--positive'}`}>{token.volumeChange}</span>
        </div>
      </td>
      <td data-label="Txns">
        <TransactionCell token={token} />
      </td>
      <td data-label="Token info">
        <SafetyEvidence safety={token.safety} holders={token.holders} />
      </td>
      <td data-label="Action">
        <div className="row-action">
          <button className="quick-buy-button" type="button" onClick={onQuickBuy}>Buy 0.1 {token.chain === 'Solana' ? 'SOL' : token.chain === 'BNB' ? 'BNB' : token.chain === 'Ethereum' ? 'ETH' : 'ETH'}</button>
          <button className="row-more-button" type="button" aria-label={`More actions for ${token.name}`}><MoreHorizontal size={17} /></button>
        </div>
      </td>
    </tr>
  )
}

function MetricCell({ label, value, change }: { label: string; value: string; change: string }) {
  const negative = change.startsWith('-')
  return (
    <td data-label={label}>
      <div className="metric-cell">
        <span className="metric-value">{value}</span>
        <span className={`metric-change ${negative ? 'metric-change--negative' : 'metric-change--positive'}`}>{change}</span>
      </div>
    </td>
  )
}

function MarketCapCell({ token }: { token: DiscoveryToken }) {
  const positive = !token.marketCapChange.startsWith('-')
  return (
    <div className="market-cell">
      <Sparkline values={token.sparkline} positive={positive} />
      <div className="metric-cell">
        <span className="metric-value">{token.marketCap}</span>
        <span className={`metric-change ${positive ? 'metric-change--positive' : 'metric-change--negative'}`}>{token.marketCapChange}</span>
      </div>
    </div>
  )
}

function TransactionCell({ token }: { token: DiscoveryToken }) {
  const total = token.buys + token.sells
  const buyPercent = Math.round((token.buys / total) * 100)
  return (
    <div className="txn-cell">
      <div className="txn-cell__top"><span className="metric-value">{token.txns}</span><Activity size={14} /></div>
      <div className="txn-cell__split" role="img" aria-label={`${buyPercent}% buys and ${100 - buyPercent}% sells`}><span>{token.buys.toLocaleString()} buys</span><span>{token.sells.toLocaleString()} sells</span></div>
    </div>
  )
}

function SafetyEvidence({ safety, holders }: { safety: DiscoveryToken['safety']; holders: string }) {
  const stateIcon = safety.state === 'passed' ? <CircleCheck size={14} /> : safety.state === 'review' ? <CircleAlert size={14} /> : <CircleX size={14} />
  return (
    <div className="safety-evidence">
      <div className={`evidence-line evidence-line--${safety.state}`}>{stateIcon}<span>{safety.liquidity}</span></div>
      <div className="evidence-line"><LockKeyhole size={14} /><span>Mint off</span></div>
      <div className="evidence-line"><Users size={14} /><span>{holders}</span></div>
      <div className={`evidence-line evidence-line--${safety.state}`}><ShieldCheck size={14} /><span>{safety.taxes.replace(' taxes', '')}</span><span className="evidence-separator">·</span><span>{safety.ownership}</span></div>
    </div>
  )
}

function Sparkline({ values, positive }: { values: number[]; positive: boolean }) {
  const max = Math.max(...values)
  const min = Math.min(...values)
  const points = values.map((value, index) => {
    const x = (index / (values.length - 1)) * 100
    const y = 28 - ((value - min) / Math.max(max - min, 1)) * 23
    return `${x},${y}`
  }).join(' ')
  return (
    <svg className={`sparkline ${positive ? 'sparkline--positive' : 'sparkline--negative'}`} viewBox="0 0 100 32" role="img" aria-label={positive ? 'Positive recent trend' : 'Negative recent trend'}>
      <polygon points={`0,32 ${points} 100,32`} fill="currentColor" opacity="0.12" />
      <polyline points={points} fill="none" stroke="currentColor" strokeWidth="1.8" vectorEffect="non-scaling-stroke" />
    </svg>
  )
}

function TokenAvatar({ src, tone }: { src: string; tone: DiscoveryToken['avatarTone'] }) {
  return (
    <span className={`token-avatar token-avatar--${tone}`} aria-hidden="true">
      <img src={src} alt="" loading="eager" decoding="async" />
    </span>
  )
}

function DiscoveryStatePanel({ type }: { type: Exclude<DiscoverySurfaceState, 'ready'> }) {
  const content = {
    loading: { icon: <RefreshCw size={20} />, title: 'Loading market radar', body: 'Keeping the table shape stable while the latest snapshot arrives.' },
    empty: { icon: <Search size={20} />, title: 'No tokens match these filters', body: 'Broaden the chain or screening rules to see more opportunities.' },
    error: { icon: <CircleAlert size={20} />, title: 'Market radar is unavailable', body: 'The snapshot could not be refreshed. Try again when the connection is restored.' },
  }[type]
  return (
    <div className={`discovery-state discovery-state--${type}`} role={type === 'error' ? 'alert' : 'status'}>
      <span className="discovery-state__icon">{content.icon}</span>
      <div><h2>{content.title}</h2><p>{content.body}</p></div>
      {type === 'error' && <button className="button button--secondary" type="button"><RefreshCw size={14} /> Retry</button>}
    </div>
  )
}
