import { useEffect, useMemo, useRef, useState, type ChangeEvent, type ReactNode } from 'react'
import Decimal from 'decimal.js'
import {
  AlertTriangle,
  ArrowLeft,
  CandlestickChart,
  Check,
  ChevronDown,
  CircleAlert,
  Clock3,
  Copy,
  Crosshair,
  ExternalLink,
  Eye,
  Gauge,
  Info,
  LayoutGrid,
  ListFilter,
  Maximize2,
  RefreshCw,
  Search,
  Settings2,
  Star,
  WalletCards,
  Zap,
} from 'lucide-react'
import {
  CandlestickSeries,
  ColorType,
  HistogramSeries,
  createChart,
  type UTCTimestamp,
} from 'lightweight-charts'
import type {
  TokenCandle,
  TokenChartData,
  TokenChartInterval,
  TokenMarketTab,
  TokenOrder,
  TokenPosition,
  TokenTradeRow,
  TokenWorkspaceFixture,
  TokenWorkspaceConnectionState,
  TokenWorkspaceState,
  TokenWorkspaceStreamStatus,
  TopTrader,
  TradeDraft,
  TradeExecutionState,
  TradePanelMode,
  TradeQuoteState,
  TokenWorkspaceStreamState,
  TokenSafetyEvidence,
} from '../../domains/market/tokenWorkspace'
import { nativeAssetForChain, tokenChartIntervals } from '../../domains/market/tokenWorkspace'
import { discoveryTokens } from '../../mocks/discoveryFixtures'
import { parseOptionalDecimal } from '../../shared/decimal'
import { getTokenRouteAddress, getTokenWorkspaceSnapshot } from './tokenWorkspaceData'
import { useTokenWorkspaceStream } from './useTokenWorkspaceStream'

const marketTabs: TokenMarketTab[] = ['Trades', 'Positions', 'Orders', 'Holders', 'Top Traders', 'Dev Token']

type TokenPageProps = { tokenId?: string; address?: string; state?: TokenWorkspaceState; onBack: () => void }

export function TokenPage({ tokenId, address, state = 'ready', onBack }: TokenPageProps) {
  const [activeTokenId, setActiveTokenId] = useState(tokenId ?? '')
  const [activeAddress, setActiveAddress] = useState(address ?? getTokenRouteAddress(tokenId ?? 'nova-set'))
  const [activeTab, setActiveTab] = useState<TokenMarketTab>('Trades')
  const [activeInterval, setActiveInterval] = useState<TokenChartInterval>('5m')
  const snapshot = useMemo(() => getTokenWorkspaceSnapshot(activeTokenId, state), [activeTokenId, state])
  const token = snapshot.token
  const stream = useTokenWorkspaceStream({ tokenId: activeTokenId, address: activeAddress, interval: activeInterval })
  const marketSnapshot = stream.snapshot
  const marketToken = marketSnapshot.token

  function handleChartIntervalChange(interval: TokenChartInterval) {
    if (interval === activeInterval) return
    setActiveInterval(interval)
  }

  function handleTokenChange(id: string) {
    setActiveTokenId(id)
    setActiveAddress(getTokenRouteAddress(id))
    setActiveTab('Trades')
    setActiveInterval('5m')
  }

  if (snapshot.state !== 'ready') return <TokenStatePanel state={snapshot.state} onBack={onBack} />

  return (
    <main className="token-page" aria-label={`${token.name} token workspace`}>
      <TokenWorkspaceHeader token={marketToken} onBack={onBack} updatedLabel={marketSnapshot.updatedLabel} connection={stream.connection} state={stream.regions.header} />
      <div className="token-workspace">
        <ContextRail token={marketToken} activeTokenId={activeTokenId} onTokenChange={handleTokenChange} state={stream.regions.header} />
        <section className="token-decision-workspace" aria-label="Token analysis">
          <TokenChartPanel
            token={marketToken}
            chartData={marketSnapshot.chart}
            updatedLabel={marketSnapshot.updatedLabel}
            activeInterval={activeInterval}
            displayedInterval={marketSnapshot.interval}
            state={stream.regions.chart}
            connection={stream.connection}
            onIntervalChange={handleChartIntervalChange}
          />
          <TokenMarketTabs activeTab={activeTab} onTabChange={setActiveTab} token={marketToken} streamState={stream.regions.trades} />
        </section>
        <TradePanel token={marketToken} />
      </div>
    </main>
  )
}

export function TokenWorkspaceHeader({ token, onBack, updatedLabel, connection, state = 'ready' }: { token: TokenWorkspaceFixture; onBack: () => void; updatedLabel: string; connection: TokenWorkspaceStreamStatus; state?: TokenWorkspaceStreamState }) {
  const connectionTone = getConnectionTone(connection.state)
  const freshnessLabel = connection.state === 'connected' ? updatedLabel : connection.label
  const unavailable = state === 'loading' || state === 'empty' || state === 'error'
  return (
    <header className="token-workspace-header">
      <div className="token-workspace-header__topline"><button className="back-button" type="button" onClick={onBack}><ArrowLeft size={15} aria-hidden="true" /><span>Discover</span></button><span className="workspace-breadcrumb">Token workspace / fixture snapshot</span></div>
      <div className="token-workspace-header__body">
        <div className="token-identity"><div className="token-identity__avatar-wrap"><img className="token-identity__avatar" src={token.avatarSrc} alt="" /><span className="token-identity__chain-mark" aria-label={`${token.chain} network`}>{token.chain === 'Solana' ? 'S' : token.chain === 'BNB' ? 'B' : 'E'}</span></div><div className="token-identity__copy"><div className="token-identity__title-row"><h1>{token.name}</h1><span className="token-identity__symbol">{token.symbol}</span><button className="copy-button" type="button" aria-label={`Copy ${token.name} contract address`}><Copy size={14} aria-hidden="true" /></button><button className="icon-button icon-button--small" type="button" aria-label={`Open ${token.name} in a block explorer`}><ExternalLink size={14} aria-hidden="true" /></button></div><div className="token-identity__meta"><span className={`chain-label chain-label--${token.chain.toLowerCase()}`}><span className="chain-label__dot" />{token.chain}</span><span className="address-label">{token.address}</span><span className="freshness-label"><span className={`status-dot status-dot--${connectionTone}`} aria-hidden="true" />{freshnessLabel}</span></div></div></div>
        <div className="token-summary-metrics" aria-label="Token market summary" aria-busy={unavailable} tabIndex={0}><Metric label="Price (USD)" value={unavailable ? '—' : token.price} change={unavailable ? undefined : token.priceChange} positive={!token.priceChange.startsWith('-')} emphasis /><Metric label="Price (quote)" value={unavailable ? '—' : token.priceEth} /><Metric label="Market cap" value={unavailable ? '—' : token.marketCap} /><Metric label="±2% depth" value={unavailable ? '—' : token.depth24h} /><Metric label={`${token.statsWindow ?? '24h'} volume`} value={unavailable ? '—' : token.volume24h} /><Metric label={`${token.statsWindow ?? '24h'} txns`} value={unavailable ? '—' : token.txns24h} /><Metric label="Total supply" value={unavailable ? '—' : token.totalSupply} /></div>
        <div className="token-header-actions"><button className="icon-button" type="button" aria-label="Open token alerts"><AlertTriangle size={15} aria-hidden="true" /></button><button className="watch-button" type="button" aria-label={`Add ${token.name} to watchlist`}><Star size={15} aria-hidden="true" /><span>Watch</span></button></div>
      </div>
      <div className="token-pool-list" aria-label="Token pools"><span className="token-pool-list__label">Pools</span>{token.pools.map((pool) => <a className="token-pool" key={`${pool.venue}-${pool.address}`} href={pool.explorerUrl} target="_blank" rel="noreferrer"><span>{pool.venue}</span><span>{pool.feeTier}</span><span className="mono-text">{pool.address}</span></a>)}</div>
      {state !== 'ready' && <div className={`token-banner token-banner--${state === 'error' ? 'warning' : 'info'}`} role={state === 'error' ? 'alert' : 'status'}><CircleAlert size={15} aria-hidden="true" /><span>{state === 'loading' ? 'Loading market data from the shared stream…' : state === 'empty' ? 'No market snapshot is available for this token yet.' : state === 'stale' ? 'Market data may be out of date while the stream reconnects.' : 'Market data is unavailable.'}</span></div>}
      {state === 'ready' && token.provisional && <div className="token-banner token-banner--warning" role="status"><CircleAlert size={15} aria-hidden="true" /><span>Provisional market data. Confirm the address and liquidity before preparing a trade.</span></div>}
    </header>
  )
}

function Metric({ label, value, change, positive, emphasis, tone }: { label: string; value: string; change?: string; positive?: boolean; emphasis?: boolean; tone?: 'blue' }) {
  return <div className={`token-metric ${emphasis ? 'token-metric--emphasis' : ''}`}><span className="token-metric__label">{label}</span><strong className={tone === 'blue' ? 'value-blue' : undefined}>{value}</strong>{change && <span className={positive ? 'value-up' : 'value-down'}>{change}</span>}</div>
}

function ContextRail({ token, activeTokenId, onTokenChange, state }: { token: TokenWorkspaceFixture; activeTokenId: string; onTokenChange: (id: string) => void; state: TokenWorkspaceStreamState }) {
  return (
    <aside className="token-context-rail" aria-label="Token context">
      <div className="context-rail__section context-rail__section--watch"><div className="context-rail__heading"><span>Watchlist</span><button type="button" aria-label="Add token to watchlist"><span aria-hidden="true">+</span></button></div><div className="context-rail__watch-card"><span className="status-dot status-dot--green" aria-hidden="true" /><div><strong>Market radar</strong><span>Live snapshot</span></div><ChevronDown size={14} aria-hidden="true" /></div></div>
      <div className="context-rail__section context-rail__section--recent"><div className="context-rail__heading"><span>Recent discovery</span><span className="context-rail__count">{discoveryTokens.length}</span></div><div className="recent-token-list">{discoveryTokens.slice(0, 6).map((token) => <button className={`recent-token ${activeTokenId === token.id ? 'recent-token--active' : ''}`} key={token.id} type="button" onClick={() => onTokenChange(token.id)}><img src={token.avatarSrc} alt="" /><span className="recent-token__copy"><strong>{token.symbol}</strong><span>{token.name}</span></span><span className={token.marketCapChange.startsWith('-') ? 'value-down recent-token__change' : 'value-up recent-token__change'}>{token.marketCapChange}</span></button>)}</div></div>
      <div className="context-rail__section context-rail__section--signals"><div className="context-rail__heading"><span>Signals</span><Gauge size={14} aria-hidden="true" /></div><Signal label="Liquidity" value={token.thin ? 'Thin' : 'Depth available'} tone={token.thin ? 'warning' : 'positive'} /></div>
      <SafetyEvidencePanel evidence={token.safetyEvidence} state={state} />
      <div className="context-rail__footer"><span className="status-dot status-dot--blue" aria-hidden="true" />All context is fixture data</div>
    </aside>
  )
}

function Signal({ label, value, tone }: { label: string; value: string; tone: 'positive' | 'warning' }) {
  return <div className="context-signal"><span>{label}</span><strong className={tone === 'positive' ? 'value-up' : 'value-amber'}>{value}</strong></div>
}

export function SafetyEvidencePanel({ evidence, state = 'ready' }: { evidence: TokenSafetyEvidence; state?: TokenWorkspaceStreamState }) {
  const checks = [
    ['Sellable', evidence.sellable],
    ['Buy tax', evidence.buyTax],
    ['Sell tax', evidence.sellTax],
  ] as const
  return <section className="safety-evidence-panel" aria-label="Safety evidence" aria-busy={state === 'loading'}><div className="context-rail__heading"><span>Safety evidence</span><span className="safety-evidence-panel__status">{state === 'loading' ? 'Loading' : state === 'error' ? 'Unavailable' : state === 'stale' ? 'Stale' : evidence.updatedLabel}</span></div>{state === 'loading' || state === 'empty' || state === 'error' ? <div className={`safety-state safety-state--${state}`} role={state === 'error' ? 'alert' : 'status'}>{state === 'loading' ? 'Waiting for safety evidence…' : state === 'empty' ? 'No safety evidence reported yet.' : 'Safety evidence is unavailable.'}</div> : <div className="safety-check-list">{checks.map(([label, check]) => <div className="safety-check" key={label}><span className={`safety-check__icon safety-check__icon--${check.status}`} aria-hidden="true">{check.status === 'passed' ? <Check size={12} /> : check.status === 'failed' ? <CircleAlert size={12} /> : <Clock3 size={12} />}</span><span className="safety-check__copy"><strong>{label}</strong><span>{check.value}</span></span></div>)}</div>}</section>
}

function TokenChartPanel({ token, chartData, updatedLabel, activeInterval, displayedInterval, state, connection, onIntervalChange }: { token: TokenWorkspaceFixture; chartData: TokenChartData; updatedLabel: string; activeInterval: TokenChartInterval; displayedInterval: TokenChartInterval; state: TokenWorkspaceStreamState; connection: TokenWorkspaceStreamStatus; onIntervalChange: (interval: TokenChartInterval) => void }) {
  const chartHigh = chartData.candles.length > 0 ? Math.max(...chartData.candles.map((candle) => candle.high)) : null
  const chartLow = chartData.candles.length > 0 ? Math.min(...chartData.candles.map((candle) => candle.low)) : null
  const chartStatus = state === 'loading' ? `Loading ${activeInterval} candles…` : state === 'empty' ? 'No candles reported' : connection.state === 'connected' ? `${displayedInterval} candles` : connection.label
  const connectionTone = getConnectionTone(connection.state)
  const footerStatus = connection.state === 'connected'
    ? state === 'stale' ? `Data stale · ${connection.label}` : connection.label
    : connection.label

  return (
    <section className="token-chart-panel" aria-label="Token price chart" aria-busy={state === 'loading' || connection.state === 'reconnecting'}>
      <div className="chart-toolbar"><div className="chart-toolbar__title"><CandlestickChart size={15} aria-hidden="true" /><span>{token.pair}</span><span className="chart-toolbar__source" role="status"><span className={`status-dot status-dot--${connectionTone}`} aria-hidden="true" />{chartStatus}</span></div><div className="chart-toolbar__controls"><div className="chart-intervals" role="tablist" aria-label="Chart interval">{tokenChartIntervals.map((interval) => <button className={`chart-interval ${activeInterval === interval ? 'chart-interval--active' : ''}`} key={interval} type="button" role="tab" aria-selected={activeInterval === interval} onClick={() => onIntervalChange(interval)}>{interval}</button>)}</div><button className="chart-control" type="button" aria-label="Toggle multi-chart layout"><LayoutGrid size={15} aria-hidden="true" /></button><button className="chart-control" type="button" aria-label="Toggle crosshair"><Crosshair size={15} aria-hidden="true" /></button><button className="chart-control" type="button" aria-label="Open chart settings"><Settings2 size={15} aria-hidden="true" /></button></div></div>
      <div className="chart-subtoolbar"><span className="chart-subtoolbar__active">Price / MC</span><span>USD / SOL</span><span>Display <ChevronDown size={13} aria-hidden="true" /></span><span className="chart-subtoolbar__spacer" /><button type="button" aria-label="Search chart"><Search size={14} aria-hidden="true" /></button><button type="button" aria-label="Maximize chart"><Maximize2 size={14} aria-hidden="true" /></button></div>
      <div className="chart-legend"><span className="chart-legend__price">{token.price}</span><span className="value-up">{token.priceChange}</span><span>High {chartHigh === null ? '—' : formatChartPrice(chartHigh)}</span><span>Low {chartLow === null ? '—' : formatChartPrice(chartLow)}</span><span className="chart-legend__volume">Vol {token.volume24h}</span></div>
      <div className={`chart-visual chart-visual--${state}`}>
        <LightweightTokenChart token={token} chartData={chartData} interval={displayedInterval} />
        {state !== 'ready' && <ChartStateOverlay state={state} activeInterval={activeInterval} />}
      </div>
      <div className="chart-footer"><span><span className={`status-dot status-dot--${connectionTone}`} aria-hidden="true" />{state === 'loading' ? `Loading ${activeInterval} chart…` : footerStatus}</span><span>{state === 'loading' ? 'Waiting for the new interval snapshot' : updatedLabel}</span></div>
    </section>
  )
}

function getConnectionTone(state: TokenWorkspaceConnectionState): 'blue' | 'green' | 'amber' | 'red' {
  if (state === 'connected') return 'green'
  if (state === 'stale') return 'amber'
  if (state === 'reconnecting') return 'amber'
  if (state === 'error') return 'red'
  return 'blue'
}

function LightweightTokenChart({ token, chartData, interval }: { token: TokenWorkspaceFixture; chartData: TokenChartData; interval: TokenChartInterval }) {
  const containerRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!containerRef.current) return
    const container = containerRef.current
    const chart = createChart(container, { autoSize: true, layout: { background: { type: ColorType.Solid, color: 'transparent' }, textColor: '#7d879b', attributionLogo: false }, grid: { vertLines: { color: 'rgba(32, 38, 50, 0.55)' }, horzLines: { color: 'rgba(32, 38, 50, 0.55)' } }, crosshair: { vertLine: { color: 'rgba(109, 124, 255, 0.35)' }, horzLine: { color: 'rgba(109, 124, 255, 0.35)' } }, rightPriceScale: { borderColor: '#202632', scaleMargins: { top: 0.08, bottom: 0.2 } }, timeScale: { borderColor: '#202632', timeVisible: true, secondsVisible: false } })
    const candles = chart.addSeries(CandlestickSeries, { upColor: '#43d69a', downColor: '#ff6678', borderVisible: false, wickUpColor: '#43d69a', wickDownColor: '#ff6678' })
    candles.setData(chartData.candles.map((candle, index) => toChartCandle(candle, index, interval)))
    const volume = chart.addSeries(HistogramSeries, { priceFormat: { type: 'volume' }, priceScaleId: '', color: 'rgba(67, 214, 154, 0.28)' })
    volume.priceScale().applyOptions({ scaleMargins: { top: 0.82, bottom: 0 } })
    volume.setData(chartData.volume.map((point, index) => ({ time: toChartTime(point.time, index, interval), value: point.value, color: point.tone === 'up' ? 'rgba(67, 214, 154, 0.30)' : 'rgba(255, 102, 120, 0.26)' })))
    if (chartData.candles.length > 0) chart.timeScale().fitContent()
    return () => chart.remove()
  }, [chartData, interval, token.name])
  return <div className="price-chart price-chart--lightweight" ref={containerRef} aria-label={`${token.name} candlestick price chart (${interval})`} role="img" />
}

function toChartTime(time: string, index: number, interval: TokenChartInterval): UTCTimestamp {
  if (/^\d+$/.test(time)) return Number(BigInt(time) / 1000n) as UTCTimestamp
  const intervalSeconds: Record<TokenChartInterval, number> = { '1s': 1, '1m': 60, '5m': 300, '1h': 3600 }
  return Math.floor(Date.UTC(2026, 8, 30, 9, 0, index * intervalSeconds[interval]) / 1000) as UTCTimestamp
}

function toChartCandle(candle: TokenCandle, index: number, interval: TokenChartInterval) {
  return { time: toChartTime(candle.time, index, interval), open: candle.open, high: candle.high, low: candle.low, close: candle.close }
}

function formatChartPrice(value: number) { return `$${value.toFixed(2)}` }

function ChartStateOverlay({ state, activeInterval }: { state: TokenWorkspaceStreamState; activeInterval: TokenChartInterval }) {
  if (state === 'ready') return null
  const content = {
    loading: { title: `Loading ${activeInterval} chart`, body: 'Waiting for the new interval snapshot before drawing candles.' },
    empty: { title: 'No candles in this interval', body: 'The shared stream has not reported any candles for this token yet.' },
    stale: { title: 'Chart data is stale', body: 'This snapshot is older than the trade freshness window.' },
    error: { title: 'Chart snapshot unavailable', body: 'The chart could not be refreshed. Retry when the connection is restored.' },
  }[state]
  return <div className={`chart-state-overlay chart-state-overlay--${state}`} role={state === 'error' ? 'alert' : 'status'}><div><strong>{content.title}</strong><span>{content.body}</span></div></div>
}

function TokenMarketTabs({ activeTab, onTabChange, token, streamState }: { activeTab: TokenMarketTab; onTabChange: (tab: TokenMarketTab) => void; token: TokenWorkspaceFixture; streamState: TokenWorkspaceStreamState }) {
  return <section className="token-detail-panel" aria-label="Token market details"><div className="token-detail-toolbar"><div className="token-detail-tabs" role="tablist" aria-label="Token market views">{marketTabs.map((tab) => <button className={`token-detail-tab ${activeTab === tab ? 'token-detail-tab--active' : ''}`} key={tab} type="button" role="tab" aria-selected={activeTab === tab} onClick={() => onTabChange(tab)}>{tab}{tab === 'Trades' && <span className="tab-count">{token.trades.length}</span>}{tab === 'Holders' && <span className="tab-count">{token.topHolders}</span>}</button>)}</div><div className="token-detail-actions"><button className="quiet-button" type="button"><RefreshCw size={14} aria-hidden="true" /> Refresh</button><button className="icon-button icon-button--small" type="button" aria-label="Filter market table"><ListFilter size={14} aria-hidden="true" /></button></div></div><div className="token-detail-content">{renderMarketTab(activeTab, token, streamState)}</div></section>
}

function renderMarketTab(tab: TokenMarketTab, token: TokenWorkspaceFixture, streamState: TokenWorkspaceStreamState) {
  if (tab === 'Trades') return <TradesPanel rows={token.trades} state={streamState} />
  if (tab === 'Positions') return <PositionsPanel rows={token.positions} />
  if (tab === 'Orders') return <OrdersPanel rows={token.orders} />
  if (tab === 'Holders') return <HoldersPanel token={token} />
  if (tab === 'Top Traders') return <TopTradersPanel rows={token.topTraders} />
  return <DevTokenPanel token={token} />
}

export function TradesPanel({ rows, state = 'ready' }: { rows: TokenTradeRow[]; state?: TokenWorkspaceStreamState }) {
  const effectiveState = state === 'ready' && rows.length === 0 ? 'empty' : state
  return <MarketTable title="Trades" description="Latest observed activity for this pair." state={effectiveState} headers={['Age', 'Type', 'Price', 'Amount', 'Total USD', 'Gas', 'Trader', 'Tx hash', 'Tracking']} rows={rows.map((row) => [row.time, <span className={`activity-side activity-side--${row.side.toLowerCase()}`}><span className="status-dot" aria-hidden="true" />{row.side}</span>, row.price, row.amount, row.value, row.gas, row.trader, <a className="table-link mono-text" href={row.txUrl} target="_blank" rel="noreferrer">{row.txHash}</a>, <span className={row.tracking === 'Tracked' ? 'value-up' : 'muted-text'}>{row.tracking}</span>])} />
}
function PositionsPanel({ rows }: { rows: TokenPosition[] }) { return <MarketTable title="Open positions" description="Fixture positions observed across tracked wallets." headers={['Wallet', 'Side', 'Size', 'Entry', 'Unrealized PnL']} rows={rows.map((row) => [row.wallet, <span className="value-up">{row.side}</span>, row.size, row.entry, <span className="value-up">{row.pnl}</span>])} /> }
function OrdersPanel({ rows }: { rows: TokenOrder[] }) { return <MarketTable title="Orders" description="Pending and recently completed fixture orders." headers={['Type', 'Side', 'Amount', 'Trigger', 'Status']} rows={rows.map((row) => [row.type, <span className={row.side === 'Buy' ? 'value-up' : 'value-down'}>{row.side}</span>, row.amount, row.trigger, <span className={row.status === 'Open' ? 'value-amber' : 'muted-text'}>{row.status}</span>])} /> }

function HoldersPanel({ token }: { token: TokenWorkspaceFixture }) {
  return <div className="simple-detail-panel"><div className="detail-panel-heading"><div><h2>Holder distribution</h2><p>{token.topHolders} across the latest snapshot.</p></div><span className="detail-panel__value">Top 10 · 24.8%</span></div><div className="distribution-bar" aria-label="Top holder distribution"><span style={{ width: '34%' }} /><span style={{ width: '22%' }} /><span style={{ width: '18%' }} /><span style={{ width: '26%' }} /></div><div className="distribution-legend"><span><i className="legend-dot legend-dot--blue" />Top holder 12.4%</span><span><i className="legend-dot legend-dot--green" />Top 2–5 22.1%</span><span><i className="legend-dot legend-dot--amber" />Top 6–10 18.4%</span><span><i className="legend-dot legend-dot--muted" />Other 47.1%</span></div><div className="holders-table"><div className="holders-table__head"><span>Wallet</span><span>Share</span><span>Balance</span><span>Label</span></div>{token.holderRows.map((holder) => <div className="holders-table__row" key={holder.id}><span className="mono-text">{holder.wallet}</span><strong>{holder.share}</strong><span>{holder.balance}</span><span className="muted-text">{holder.label}</span></div>)}</div></div>
}
function TopTradersPanel({ rows }: { rows: TopTrader[] }) { return <MarketTable title="Top traders" description="Highest-volume wallets in the current fixture window." headers={['Wallet', 'Volume', 'Realized PnL', 'Win rate']} rows={rows.map((row) => [row.wallet, row.volume, <span className={row.realizedPnl.startsWith('+') ? 'value-up' : 'value-down'}>{row.realizedPnl}</span>, row.winRate])} /> }
function DevTokenPanel({ token }: { token: TokenWorkspaceFixture }) { return <div className="simple-detail-panel dev-token-panel"><div className="detail-panel-heading"><div><h2>Developer token activity</h2><p>Ownership and recent contract-level actions.</p></div><span className="muted-text">Fixture metadata</span></div><div className="dev-token-grid"><div><span>Wallet</span><strong className="mono-text">{token.developerToken.wallet}</strong></div><div><span>Balance</span><strong>{token.developerToken.balance}</strong></div><div><span>Supply share</span><strong>{token.developerToken.share}</strong></div><div><span>Last action</span><strong>{token.developerToken.lastAction}</strong></div></div></div> }

function MarketTable({ title, description, headers, rows, state = 'ready' }: { title: string; description: string; headers: string[]; rows: ReactNode[][]; state?: TokenWorkspaceStreamState }) {
  const showRows = state === 'ready' || state === 'stale'
  const statusLabel = state === 'error' ? 'Unavailable' : state === 'loading' ? 'Loading' : state === 'empty' ? 'Empty' : state === 'stale' ? 'Stale' : 'Live stream'
  return <div className={`market-table-panel ${state === 'stale' ? 'market-table-panel--stale' : ''}`}><div className="detail-panel-heading"><div><h2>{title}</h2><p>{description}</p></div><span className="table-live"><span className={`status-dot status-dot--${state === 'error' ? 'red' : state === 'loading' || state === 'stale' ? 'amber' : 'green'}`} aria-hidden="true" />{statusLabel}</span></div>{!showRows ? <div className={`market-table-state market-table-state--${state}`} role={state === 'error' ? 'alert' : 'status'}><strong>{state === 'loading' ? `Loading ${title.toLowerCase()}…` : state === 'empty' ? `No ${title.toLowerCase()} yet` : `${title} unavailable`}</strong><span>{state === 'loading' ? 'Keeping the table shape stable while the snapshot arrives.' : state === 'empty' ? 'The stream has not reported any records for this token.' : 'Retry when the market stream is available again.'}</span></div> : <div className="market-table-wrap"><table className="market-table"><thead><tr>{headers.map((header) => <th key={header} scope="col">{header}</th>)}</tr></thead><tbody>{rows.map((row, rowIndex) => <tr key={`${title}-${rowIndex}`}>{row.map((cell, cellIndex) => <td key={`${title}-${rowIndex}-${cellIndex}`}>{cell}</td>)}</tr>)}</tbody></table></div>}</div>
}

function TradePanel({ token }: { token: TokenWorkspaceFixture }) {
  const [draft, setDraft] = useState<TradeDraft>({ side: 'Buy', amount: '', asset: nativeAssetForChain(token.chain), quoteState: 'idle' })
  const [mode, setMode] = useState<TradePanelMode>('Market')
  const [executionState, setExecutionState] = useState<TradeExecutionState>('idle')
  const [notice, setNotice] = useState('')
  const presets = ['0.01', '0.1', '0.5', '1']
  useEffect(() => {
    if (draft.quoteState !== 'fresh') return
    const expiry = window.setTimeout(() => {
      setDraft((current) => current.quoteState === 'fresh' ? { ...current, quoteState: 'expired' } : current)
    }, 18_000)
    return () => window.clearTimeout(expiry)
  }, [draft.quoteState])
  function updateAmount(event: ChangeEvent<HTMLInputElement>) { const amount = event.target.value; const parsedAmount = parseTradeAmount(amount); const quoteState = parsedAmount && parsedAmount.isFinite() && parsedAmount.gt(0) ? 'fresh' : 'idle'; setDraft((current) => ({ ...current, amount, quoteState })); setExecutionState('idle'); setNotice('') }
  function requestQuote() { const amount = parseTradeAmount(draft.amount); if (!amount || !amount.isFinite() || amount.lte(0)) { setNotice('Enter an amount to request a fixture quote.'); setDraft((current) => ({ ...current, quoteState: 'error' })); return }; setNotice(''); setDraft((current) => ({ ...current, quoteState: 'loading' })); window.setTimeout(() => setDraft((current) => current.quoteState === 'loading' ? { ...current, quoteState: 'fresh' } : current), 420) }
  function reviewTrade() { if (draft.quoteState !== 'fresh') { requestQuote(); return }; setExecutionState('review'); setNotice('') }
  const amount = parseTradeAmount(draft.amount)
  const receiveAmount = amount && amount.gt(0) ? amount.times(2588).toDecimalPlaces(0, Decimal.ROUND_DOWN).toLocaleString() : '—'
  const quoteStatus: Record<TradeQuoteState, string> = { idle: 'Enter an amount to get a quote', loading: 'Refreshing fixture quote…', fresh: 'Quote valid for 18s', expired: 'Quote expired · refresh to continue', error: 'Quote unavailable for this amount' }

  return <aside className="trade-panel" aria-label="Trade panel">
    <div className="trade-panel__header"><div><span className="panel-eyebrow">Trade workspace</span><h2>{token.symbol}</h2></div><span className="trade-panel__fixture"><span className="status-dot status-dot--blue" aria-hidden="true" />Mock only</span></div>
    <div className="trade-preset-row"><div className="trade-layout-tabs" role="tablist" aria-label="Trade preset"><button className="trade-layout-tab trade-layout-tab--active" type="button" role="tab" aria-selected="true">P1</button><button className="trade-layout-tab" type="button" role="tab" aria-selected="false">P2</button><button className="trade-layout-tab" type="button" role="tab" aria-selected="false">P3</button></div><button className="wallet-compact" type="button"><WalletCards size={14} aria-hidden="true" /> 1 <ChevronDown size={13} aria-hidden="true" /></button></div>
    <div className="trade-side-tabs" role="tablist" aria-label="Trade side">{(['Buy', 'Sell', 'Auto'] as const).map((side) => <button className={draft.side === side ? 'trade-side-tab trade-side-tab--active' : 'trade-side-tab'} key={side} type="button" role="tab" aria-selected={draft.side === side} onClick={() => { setDraft((current) => ({ ...current, side, quoteState: 'idle' })); setExecutionState('idle'); setNotice('') }}>{side}</button>)}</div>
    <div className="trade-mode-tabs" role="tablist" aria-label="Trade mode">{(['Market', 'Limit', 'DCA', 'Advanced'] as TradePanelMode[]).map((item) => <button className={mode === item ? 'trade-mode-tab trade-mode-tab--active' : 'trade-mode-tab'} key={item} type="button" role="tab" aria-selected={mode === item} onClick={() => setMode(item)}>{item}{item === 'Limit' && <Info size={11} aria-hidden="true" />}</button>)}<span className="trade-balance">0 {draft.asset}</span></div>
    <div className="trade-field"><div className="trade-field__label"><label htmlFor="trade-amount">Amount</label><span>{draft.asset}</span></div><div className="amount-input"><input id="trade-amount" inputMode="decimal" placeholder="0.00" value={draft.amount} onChange={updateAmount} aria-describedby="trade-balance" /><span>{draft.asset}</span><ChevronDown size={14} aria-hidden="true" /></div><div className="trade-presets" role="group" aria-label="Trade amount presets">{presets.map((preset) => <button type="button" key={preset} onClick={() => { setDraft((current) => ({ ...current, amount: preset, quoteState: 'fresh' })); setExecutionState('idle'); setNotice('') }}>{preset}</button>)}</div><span id="trade-balance" className="trade-helper">1 {draft.asset} ≈ 2,588 {token.symbol}</span></div>
    <div className="trade-safety-row"><span className="trade-safety-row__pending"><Clock3 size={13} aria-hidden="true" /> Safety checks pending</span><span>Impact &lt;0.1% quote estimate</span></div>
    <div className="quote-breakdown"><div><span>You receive</span><strong>~{receiveAmount} {token.symbol}</strong></div><div><span>Rate</span><span>1 {draft.asset} = 2,588 {token.symbol}</span></div><div><span>Price impact</span><span className="value-up">&lt;0.1%</span></div><div><span>Network fee</span><span>~$0.42</span></div></div>
    <div className={`quote-status quote-status--${draft.quoteState}`}><span className={`status-dot ${draft.quoteState === 'fresh' ? 'status-dot--green' : draft.quoteState === 'error' ? 'status-dot--red' : 'status-dot--blue'}`} aria-hidden="true" />{quoteStatus[draft.quoteState]}{draft.quoteState === 'expired' && <button type="button" onClick={requestQuote}>Refresh</button>}</div>
    <label className="strategy-toggle"><input type="checkbox" /><span>Advanced trading strategy</span><ChevronDown size={13} aria-hidden="true" /></label>
    {notice && <p className="trade-notice" role="alert">{notice}</p>}
    {executionState === 'review' ? <div className="trade-review" role="status"><div className="trade-review__title"><Check size={15} />Review ready</div><p>{draft.side} {draft.amount} {draft.asset} for approximately ~{receiveAmount} {token.symbol}. Signing is disabled in this foundation slice.</p><button className="button button--secondary" type="button" onClick={() => setExecutionState('idle')}>Edit draft</button></div> : <button className="button button--primary trade-review-button" type="button" onClick={reviewTrade}><Zap size={15} aria-hidden="true" />{draft.side === 'Sell' ? 'Review sell' : 'Review trade'}</button>}
    <div className="trade-execution-note"><WalletCards size={14} aria-hidden="true" /><span>Connect a wallet in the execution phase to sign.</span></div>
    <div className="trade-panel__footer"><span><Eye size={13} aria-hidden="true" /> Slippage 0.5%</span><span><Settings2 size={13} aria-hidden="true" /> Priority standard</span></div>
  </aside>
}

function parseTradeAmount(value: string): Decimal | undefined {
  if (value.trim() === '') return undefined
  try {
    return parseOptionalDecimal(value)
  } catch {
    return undefined
  }
}

function TokenStatePanel({ state, onBack }: { state: Exclude<TokenWorkspaceState, 'ready'>; onBack: () => void }) {
  const content = { loading: { icon: <RefreshCw size={20} />, title: 'Loading token workspace', body: 'Keeping the chart, market tabs and trade panel stable while the snapshot arrives.' }, missing: { icon: <CircleAlert size={20} />, title: 'Token not found', body: 'This token is no longer available in the current market snapshot.' }, stale: { icon: <Clock3 size={20} />, title: 'Market data is stale', body: 'The last snapshot is older than the trade freshness window. Quotes are paused until it refreshes.' }, error: { icon: <CircleAlert size={20} />, title: 'Token workspace unavailable', body: 'The market snapshot could not be loaded. Return to Discover and try again.' } }[state]
  return <main className="token-page token-page--state" aria-label="Token workspace state"><button className="back-button" type="button" onClick={onBack}><ArrowLeft size={15} /> Back to Discover</button><div className={`token-state-panel token-state-panel--${state}`} role={state === 'missing' ? 'alert' : 'status'}><span className="token-state-panel__icon">{content.icon}</span><h1>{content.title}</h1><p>{content.body}</p>{state !== 'loading' && <button className="button button--secondary" type="button" onClick={onBack}>Return to Discover</button>}</div></main>
}
