import { useMemo, useState } from 'react'
import {
  Activity,
  ArrowDownUp,
  ArrowDownWideNarrow,
  ArrowUpNarrowWide,
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
  PauseCircle,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Users,
  WalletCards,
} from 'lucide-react'
import Decimal from 'decimal.js'
import type { DataState } from '../../api/stream/dataState'
import type { Chain, DiscoveryToken, SafetyVerdict } from '../../domains/market/token'
import { DataStatus } from '../../shared/ui/DataStatus'
import { sortLabels, type DiscoveryFilters, type DiscoverySort, type SortKey } from './discoveryView'
import { useLiveDiscovery } from './useLiveDiscovery'

// New lists pools by creation; Trending ranks by 5m volume and txns above a liquidity floor (#81).
const streams = ['New', 'Trending'] as const
const sortKeys: SortKey[] = ['feed', 'age', 'volume', 'txns', 'liquidity', 'marketCap']

// Base is served now; BNB Chain and MegaETH come after it, so they show as coming.
const chains: Array<{ label: Chain; tone: string; chainId?: bigint }> = [
  { label: 'Base', tone: 'base', chainId: 8453n },
  { label: 'BNB', tone: 'bnb' },
  { label: 'MegaETH', tone: 'megaeth' },
]

const chainAssets: Record<string, string> = {
  base: '/assets/chains/base.svg',
  bnb: '/assets/chains/bnb.svg',
  megaeth: '/assets/chains/megaeth.svg',
}

const liquidityFloors = [
  { label: 'Any', value: '' },
  { label: '$10K', value: '10000' },
  { label: '$50K', value: '50000' },
  { label: '$100K', value: '100000' },
  { label: '$500K', value: '500000' },
]

const maxAges = [
  { label: 'Any', value: '' },
  { label: '5m', value: '300000' },
  { label: '15m', value: '900000' },
  { label: '1h', value: '3600000' },
  { label: '6h', value: '21600000' },
  { label: '24h', value: '86400000' },
]

type DiscoverySurfaceState = 'ready' | 'loading' | 'empty' | 'error'

type DiscoveryPageProps = {
  state?: DiscoverySurfaceState
}

export function DiscoveryPage({ state = 'ready' }: DiscoveryPageProps) {
  const [activeStream, setActiveStream] = useState<(typeof streams)[number]>('New')
  const [sort, setSort] = useState<DiscoverySort>({ key: 'feed', direction: 'asc' })
  const [activeChain, setActiveChain] = useState<Chain>('Base')
  const [filtersOpen, setFiltersOpen] = useState(false)
  const [safetyOnly, setSafetyOnly] = useState(false)
  const [selectedTokenId, setSelectedTokenId] = useState<string | null>(null)
  const [tradeNotice, setTradeNotice] = useState('')
  const [activePreset, setActivePreset] = useState('P1')
  const [minLiquidity, setMinLiquidity] = useState('')
  const [maxAge, setMaxAge] = useState('')
  // The pointer or keyboard focus is in the table: its order holds still (#65).
  const [pointerInTable, setPointerInTable] = useState(false)
  const [focusInTable, setFocusInTable] = useState(false)

  const filters = useMemo<DiscoveryFilters>(() => ({
    chainId: chains.find((chain) => chain.label === activeChain)?.chainId ?? -1n,
    minLiquidityUsd: minLiquidity ? new Decimal(minLiquidity) : undefined,
    maxAgeMs: maxAge ? BigInt(maxAge) : undefined,
    safetyPassedOnly: safetyOnly,
  }), [activeChain, minLiquidity, maxAge, safetyOnly])
  const activeFilterCount = [minLiquidity, maxAge, safetyOnly].filter(Boolean).length

  const held = pointerInTable || focusInTable
  const live = useLiveDiscovery({ view: activeStream, sort, filters, held })
  const visibleTokens = live.tokens
  const liveSurface: DiscoverySurfaceState = !live.loaded ? (live.state === 'unavailable' ? 'error' : 'loading') : 'ready'
  const surface = state === 'ready' ? liveSurface : state
  const dimmed = live.state === 'stale' || live.state === 'reconnecting' || live.state === 'unavailable'

  function handleQuickBuy(token: DiscoveryToken) {
    setTradeNotice(`Buy draft ready for ${token.symbol}. Review is required before signing.`)
  }

  return (
    <main className="discovery-page" aria-label="Discover market radar">
      <h1 className="sr-only">Discover market radar</h1>
      <DiscoveryToolbar
        activeStream={activeStream}
        filtersOpen={filtersOpen}
        safetyOnly={safetyOnly}
        onStreamChange={setActiveStream}
        sort={sort}
        onSortChange={setSort}
        activeFilterCount={activeFilterCount}
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
              Safety check passed
            </button>
          </div>
          <label className="filter-panel__group filter-select">
            <span className="filter-panel__label"><Gauge size={13} aria-hidden="true" /> Min liquidity</span>
            <select value={minLiquidity} onChange={(event) => setMinLiquidity(event.target.value)}>
              {liquidityFloors.map((floor) => <option key={floor.label} value={floor.value}>{floor.label}</option>)}
            </select>
          </label>
          <label className="filter-panel__group filter-select">
            <span className="filter-panel__label"><Clock3 size={13} aria-hidden="true" /> Max age</span>
            <select value={maxAge} onChange={(event) => setMaxAge(event.target.value)}>
              {maxAges.map((age) => <option key={age.label} value={age.value}>{age.label}</option>)}
            </select>
          </label>
          <div className="filter-panel__group">
            <span className="filter-panel__hint"><Info size={13} /> Liquidity is ±2% depth in USD. An unchecked token never counts as passed.</span>
          </div>
        </div>
      )}

      <div className="discovery-layout">
        <ChainRail activeChain={activeChain} onChainChange={setActiveChain} />
        <section
          className={`discovery-table-shell ${dimmed && surface === 'ready' ? 'discovery-table-shell--stale' : ''}`}
          aria-label={`${activeStream} token stream`}
          onPointerEnter={(event) => event.pointerType !== 'touch' && setPointerInTable(true)}
          onPointerLeave={() => setPointerInTable(false)}
          onFocus={() => setFocusInTable(true)}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setFocusInTable(false)
          }}
        >
          {dimmed && surface === 'ready' && <StaleBanner state={live.state} />}
          {surface === 'loading' && <DiscoveryStatePanel type="loading" />}
          {surface === 'empty' && <DiscoveryStatePanel type="empty" />}
          {surface === 'error' && <DiscoveryStatePanel type="error" />}
          {surface === 'ready' && visibleTokens.length === 0 && <DiscoveryStatePanel type="empty" />}
          {surface === 'ready' && visibleTokens.length > 0 && (
            <TokenTable
              tokens={visibleTokens}
              selectedTokenId={selectedTokenId}
              onSelect={setSelectedTokenId}
              onQuickBuy={handleQuickBuy}
            />
          )}
          <div className="discovery-table-footer">
            <span className="table-footer__summary">
              <DataStatus state={live.state} label="Discovery feed" />
              {visibleTokens.length} tokens in view · {activeStream} · 5m stats
            </span>
            {held && (
              <span className="table-footer__hold" data-testid="discovery-hold">
                <PauseCircle size={13} aria-hidden="true" />
                Order held while you're in the table{live.waiting > 0 ? ` · ${live.waiting} new waiting` : ''}
              </span>
            )}
            <span className="table-footer__notice" role="status">
              {tradeNotice && <span className="trade-notice">{tradeNotice}</span>}
            </span>
          </div>
        </section>
      </div>
    </main>
  )
}

type DiscoveryToolbarProps = {
  activeStream: (typeof streams)[number]
  filtersOpen: boolean
  safetyOnly: boolean
  onStreamChange: (stream: (typeof streams)[number]) => void
  sort: DiscoverySort
  onSortChange: (sort: DiscoverySort) => void
  activeFilterCount: number
  onFiltersToggle: () => void
  onSafetyToggle: () => void
  activePreset: string
  onPresetChange: (preset: string) => void
}

function DiscoveryToolbar({
  activeStream,
  filtersOpen,
  safetyOnly,
  onStreamChange,
  sort,
  onSortChange,
  activeFilterCount,
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
          </button>
        ))}
      </div>

      <div className="discovery-toolbar__controls">
        <div className="sort-control" role="group" aria-label="Sort tokens">
          <label className="sort-control__select">
            <span className="sr-only">Sort by</span>
            <ArrowDownUp size={14} aria-hidden="true" />
            <select value={sort.key} onChange={(event) => onSortChange(defaultSort(event.target.value as SortKey))}>
              {sortKeys.map((key) => <option key={key} value={key}>{sortLabels[key]}</option>)}
            </select>
          </label>
          <button
            className="toolbar-icon-button"
            type="button"
            aria-label={sortDirectionLabel(sort)}
            title={sortDirectionLabel(sort)}
            onClick={() => onSortChange({ ...sort, direction: sort.direction === 'asc' ? 'desc' : 'asc' })}
          >
            {sort.direction === 'asc' ? <ArrowUpNarrowWide size={16} /> : <ArrowDownWideNarrow size={16} />}
          </button>
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
          {activeFilterCount > 0 && <span className="toolbar-control__count">{activeFilterCount}</span>}
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

/** A new sort key starts in its natural direction: the list's own order, youngest, or highest first. */
function defaultSort(key: SortKey): DiscoverySort {
  return { key, direction: key === 'feed' || key === 'age' ? 'asc' : 'desc' }
}

function sortDirectionLabel(sort: DiscoverySort): string {
  const ascending = sort.direction === 'asc'
  if (sort.key === 'feed') return ascending ? 'Feed order: top first' : 'Feed order: reversed'
  if (sort.key === 'age') return ascending ? 'Youngest first' : 'Oldest first'
  return ascending ? `${sortLabels[sort.key]}: lowest first` : `${sortLabels[sort.key]}: highest first`
}

type ChainRailProps = {
  activeChain: Chain
  onChainChange: (chain: Chain) => void
}

/**
 * Base is live; the chains after it show as coming. A coming chain stays focusable (aria-disabled,
 * not disabled) so keyboard and screen-reader users reach its explanation too.
 */
function ChainRail({ activeChain, onChainChange }: ChainRailProps) {
  return (
    <aside className="chain-rail" aria-label="Filter by chain">
      {chains.map(({ label, tone, chainId }) => {
        const live = chainId !== undefined
        const hint = live ? `${label}: live` : `${label} is coming. OmniMarket serves Base first; ${label} follows.`
        return (
          <button
            className={`chain-rail__item ${activeChain === label ? 'chain-rail__item--active' : ''} ${live ? '' : 'chain-rail__item--coming'}`}
            key={label}
            type="button"
            aria-pressed={live ? activeChain === label : undefined}
            aria-disabled={live ? undefined : true}
            aria-describedby={`chain-hint-${tone}`}
            title={hint}
            onClick={() => live && onChainChange(label)}
          >
            <span className={`chain-rail__mark chain-rail__mark--${tone}`} aria-hidden="true">
              <img src={chainAssets[tone]} alt="" />
            </span>
            <span>{label}</span>
            <span className={`chain-rail__status ${live ? 'chain-rail__status--live' : ''}`} aria-hidden="true">{live ? 'Live' : 'Soon'}</span>
            <span className="sr-only" id={`chain-hint-${tone}`}>{hint}</span>
          </button>
        )
      })}
    </aside>
  )
}

type TokenTableProps = {
  tokens: DiscoveryToken[]
  selectedTokenId: string | null
  onSelect: (id: string) => void
  onQuickBuy: (token: DiscoveryToken) => void
}

function TokenTable({ tokens, selectedTokenId, onSelect, onQuickBuy }: TokenTableProps) {
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
  onQuickBuy: () => void
}

function TokenRow({ token, index, selected, onSelect, onQuickBuy }: TokenRowProps) {
  return (
    <tr className={selected ? 'token-row token-row--selected' : 'token-row'}>
      <td data-label="Pair info">
        <div className="pair-cell">
          <span className="row-index" aria-hidden="true">{index}</span>
          <TokenAvatar src={token.avatarSrc} tone={token.avatarTone} />
          <div className="pair-cell__body">
            <div className="pair-cell__title">
              <button className="token-name" type="button" onClick={onSelect}>{token.name}</button>
              <span className="token-symbol">{token.symbol}</span>
              <button className="copy-button" type="button" aria-label={`Copy ${token.name} address`}><Copy size={13} /></button>
            </div>
            <div className="pair-cell__meta">
              <span className="token-age"><Clock3 size={13} />{token.age}</span>
              <span className={`chain-label chain-label--${token.chain.toLowerCase()}`}><span className="chain-label__dot" />{token.chain}</span>
              {token.provisional && <span className="confidence-label confidence-label--provisional"><Info size={12} />Provisional</span>}
            </div>
            {/* Fixture rows only: the live feed carries no metadata or links yet. */}
            {!token.live && (
              <div className="pair-cell__signals" aria-label="Token context">
                <span className="pair-signal" role="img" aria-label="Verified market metadata" title="Verified market metadata"><BadgeCheck size={14} /></span>
                <span className="pair-signal" role="img" aria-label="Token website available" title="Token website available"><Globe2 size={14} /></span>
                <span className="pair-signal" role="img" aria-label="Token analytics available" title="Token analytics available"><Search size={14} /></span>
                <span className="pair-signal" role="img" aria-label="Token watch status" title="Token watch status"><Eye size={14} /></span>
              </div>
            )}
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
        {change && <span className={`metric-change ${negative ? 'metric-change--negative' : 'metric-change--positive'}`}>{change}</span>}
      </div>
    </td>
  )
}

/**
 * Market cap, with the live price under it. A live row's motion shows here: the price flashes on a
 * move (the `key` remounts the span so each move replays the flash), and the row-motion marker lets
 * the whole row highlight as it inserts or dim once it has left the feed (discovery.css, `:has`).
 */
function MarketCapCell({ token }: { token: DiscoveryToken }) {
  const positive = !token.marketCapChange.startsWith('-')
  const live = token.live
  const motion = live?.departed ? 'departed' : live?.inserted ? 'inserted' : undefined
  return (
    <div className={`market-cell ${motion ? `row-motion row-motion--${motion}` : ''}`}>
      <Sparkline values={token.sparkline} positive={positive} />
      <div className="metric-cell">
        <span className="metric-value">{token.marketCap}</span>
        {token.marketCapChange && <span className={`metric-change ${positive ? 'metric-change--positive' : 'metric-change--negative'}`}>{token.marketCapChange}</span>}
        {live && (
          <span
            key={live.priceMove?.seq ?? 0}
            className={`metric-price ${live.priceMove ? `price-flash price-flash--${live.priceMove.direction}` : ''}`}
            data-testid="row-price"
          >
            {live.priceUsd}
            {live.priceMove && <span className="sr-only">{live.priceMove.direction === 'up' ? ' (up)' : ' (down)'}</span>}
          </span>
        )}
        {live?.departed && <span className="metric-departed">Left the feed</span>}
      </div>
    </div>
  )
}

function TransactionCell({ token }: { token: DiscoveryToken }) {
  const total = token.buys + token.sells
  const buyPercent = total > 0 ? Math.round((token.buys / total) * 100) : 0
  if (total === 0) {
    return (
      <div className="txn-cell">
        <div className="txn-cell__top"><span className="metric-value">{token.txns}</span><Activity size={14} /></div>
        <div className="txn-cell__split txn-cell__split--none"><span>No trades yet</span></div>
      </div>
    )
  }
  return (
    <div className="txn-cell">
      <div className="txn-cell__top"><span className="metric-value">{token.txns}</span><Activity size={14} /></div>
      <div className="txn-cell__split" role="img" aria-label={`${buyPercent}% buys and ${100 - buyPercent}% sells`}><span>{token.buys.toLocaleString()} buys</span><span>{token.sells.toLocaleString()} sells</span></div>
    </div>
  )
}

const verdictCopy: Record<SafetyVerdict, { label: string; tone: 'passed' | 'review' | 'blocked' | 'unchecked' }> = {
  unchecked: { label: 'Not checked yet', tone: 'unchecked' },
  passed: { label: 'Sell check passed', tone: 'passed' },
  warning: { label: 'Red flag found', tone: 'review' },
  honeypot: { label: 'Honeypot: sells fail', tone: 'blocked' },
}

function SafetyEvidence({ safety, holders }: { safety: DiscoveryToken['safety']; holders: string }) {
  if (safety.verdict) {
    // A live row: the check's verdict (D29), and never "safe" before a check has run.
    const { label, tone } = verdictCopy[safety.verdict]
    const icon = tone === 'passed' ? <CircleCheck size={14} /> : tone === 'blocked' ? <CircleX size={14} /> : <CircleAlert size={14} />
    return (
      <div className="safety-evidence" data-verdict={safety.verdict}>
        <div className={`evidence-line evidence-line--${tone}`}>{icon}<span>{label}</span></div>
        {safety.taxes && <div className="evidence-line"><ShieldCheck size={14} /><span>{safety.taxes}</span></div>}
        <div className="evidence-line"><Gauge size={14} /><span>{safety.liquidity}</span></div>
      </div>
    )
  }
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
  // A live row starts with one price seen; its line grows as prices arrive.
  if (values.length < 2) {
    return (
      <svg className="sparkline sparkline--flat" viewBox="0 0 100 32" role="img" aria-label="Not enough prices for a trend yet">
        <polyline points="0,28 100,28" fill="none" stroke="currentColor" strokeWidth="1.2" strokeDasharray="3 4" vectorEffect="non-scaling-stroke" />
      </svg>
    )
  }
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
    error: { icon: <CircleAlert size={20} />, title: 'Market radar is unavailable', body: 'The discovery feed can\'t be reached. The terminal keeps retrying and fills this table when it answers.' },
  }[type]
  return (
    <div className={`discovery-state discovery-state--${type}`} role={type === 'error' ? 'alert' : 'status'}>
      <span className="discovery-state__icon">{content.icon}</span>
      <div><h2>{content.title}</h2><p>{content.body}</p></div>
    </div>
  )
}

const staleCopy: Partial<Record<DataState, string>> = {
  stale: 'No heartbeat for 3 seconds: these rows may be out of date.',
  reconnecting: 'Connection lost: these rows are from before the drop. Reconnecting.',
  unavailable: 'The feed keeps failing: these rows are out of date. Still retrying.',
}

/** Over the table while its rows may be old (#63's data states): the rows stay, dimmed, and say so. */
function StaleBanner({ state }: { state: DataState }) {
  return (
    <div className="discovery-stale-banner" role="status" data-state={state}>
      <CircleAlert size={14} aria-hidden="true" />
      <span>{staleCopy[state]}</span>
    </div>
  )
}
