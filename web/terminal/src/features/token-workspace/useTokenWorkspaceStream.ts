import { useMemo } from 'react'
import Decimal from 'decimal.js'
import {
  SafetyVerdict as ProtoSafetyVerdict,
  Trade_Side,
  type Candle,
  type TokenSnapshot,
  type Trade,
} from '../../api/generated/omnimarket/api/v1/market_pb'
import { useCandlesStream, useConnection, useStreamState, useTokenStream, useTradesStream } from '../../api/stream/hooks'
import type { DataState } from '../../api/stream/dataState'
import { formatAge, formatCount, formatPercentChange, formatUsdCompact, formatUsdPrice } from '../../shared/format'
import { parseOptionalDecimal } from '../../shared/decimal'
import {
  nativeAssetForChain,
  type TokenChartData,
  type TokenChartInterval,
  type TokenMarketSnapshot,
  type TokenSafetyCheck,
  type TokenWorkspaceFixture,
  type TokenWorkspaceStreamState,
  type TokenWorkspaceStreamStatus,
} from '../../domains/market/tokenWorkspace'
import { getTokenRouteAddress, getTokenWorkspaceSnapshot } from './tokenWorkspaceData'

type UseTokenWorkspaceStreamOptions = {
  tokenId?: string
  address?: string
  interval: TokenChartInterval
}

export type TokenWorkspaceStreamRegions = {
  header: TokenWorkspaceStreamState
  chart: TokenWorkspaceStreamState
  trades: TokenWorkspaceStreamState
}

export function useTokenWorkspaceStream({ tokenId, address: requestedAddress, interval }: UseTokenWorkspaceStreamOptions) {
  const workspace = useMemo(() => getTokenWorkspaceSnapshot(tokenId), [tokenId])
  const address = requestedAddress ?? getTokenRouteAddress(tokenId ?? workspace.token.id)
  const candleInterval = toCandleInterval(interval)
  const tokenStream = useTokenStream(address)
  const tradesStream = useTradesStream(address)
  const candlesStream = useCandlesStream(address, candleInterval)
  const { state: connectionState, source } = useConnection()
  const serverTimeMs = useStreamState((state) => state.serverTimeMs)

  const connection = useMemo<TokenWorkspaceStreamStatus>(() => {
    const sourceLabel = source === 'fixtures' ? 'fixture' : source
    switch (connectionState) {
      case 'open':
        return { state: 'connected', attempt: 0, label: `${sourceLabel[0]?.toUpperCase() ?? ''}${sourceLabel.slice(1)} stream connected` }
      case 'stale':
        return { state: 'stale', attempt: 0, label: `${sourceLabel[0]?.toUpperCase() ?? ''}${sourceLabel.slice(1)} stream stale` }
      case 'reconnecting':
        return { state: 'reconnecting', attempt: 1, label: `${sourceLabel[0]?.toUpperCase() ?? ''}${sourceLabel.slice(1)} stream reconnecting` }
      case 'unavailable':
        return { state: 'error', attempt: 1, label: `${sourceLabel[0]?.toUpperCase() ?? ''}${sourceLabel.slice(1)} stream unavailable` }
      default:
        return { state: 'connecting', attempt: 0, label: `Connecting to ${sourceLabel} stream` }
    }
  }, [connectionState, source])

  const snapshot = useMemo<TokenMarketSnapshot>(() => {
    const token = tokenStream.data ? tokenSnapshotToFixture(workspace.token, tokenStream.data) : workspace.token
    const trades = tradesStream.data ? mapTrades(tradesStream.data, token, serverTimeMs) : token.trades
    const chart = candlesStream.data ? mapCandles(candlesStream.data) : { candles: [], volume: [] }
    const sequence = tokenStream.data?.blockNumber ?? 0n
    const state = toRegionState(tokenStream.state, Boolean(tokenStream.data))
    return {
      interval,
      token: { ...token, trades },
      chart,
      trades,
      sequence: Number(sequence),
      state: state === 'loading' ? 'ready' : state,
      updatedLabel: updatedLabel(tokenStream.data?.blockTimeMs, serverTimeMs),
    }
  }, [candlesStream.data, interval, serverTimeMs, tokenStream.data, tokenStream.state, tradesStream.data, workspace.token])

  const regions = useMemo<TokenWorkspaceStreamRegions>(() => ({
    header: toRegionState(tokenStream.state, Boolean(tokenStream.data)),
    chart: toRegionState(candlesStream.state, Boolean(candlesStream.data?.length)),
    trades: toRegionState(tradesStream.state, Boolean(tradesStream.data?.length)),
  }), [candlesStream.data, candlesStream.state, tokenStream.data, tokenStream.state, tradesStream.data, tradesStream.state])

  return {
    snapshot,
    state: regions.header,
    regions,
    connection,
  }
}

function toCandleInterval(interval: TokenChartInterval): '1s' | '1m' | '5m' | '1h' {
  return interval === '1s' || interval === '1m' || interval === '5m' || interval === '1h' ? interval : '5m'
}

function toRegionState(state: DataState, hasData: boolean): TokenWorkspaceStreamState {
  if (state === 'unavailable') return 'error'
  if (state === 'loading') return 'loading'
  if (state === 'reconnecting') return hasData ? 'stale' : 'loading'
  if (state === 'stale') return 'stale'
  return hasData ? 'ready' : 'empty'
}

function updatedLabel(blockTimeMs: bigint | undefined, serverTimeMs: bigint | undefined): string {
  if (blockTimeMs === undefined || serverTimeMs === undefined) return 'Awaiting heartbeat'
  return `Updated ${formatAge(serverTimeMs - blockTimeMs)} ago`
}

function tokenSnapshotToFixture(base: TokenWorkspaceFixture, snapshot: TokenSnapshot): TokenWorkspaceFixture {
  const stats = snapshot.stats1h ?? snapshot.stats5m
  const tokenRef = snapshot.token
  const price = parseOptionalDecimal(snapshot.displayPriceUsd)
  const marketCap = parseOptionalDecimal(snapshot.marketCapUsd)
  const depth = parseOptionalDecimal(snapshot.depthUsd)
  const trackedPriceChange = parseOptionalDecimal(snapshot.statsTracked?.priceChangePct ?? '')
  const priceChange = trackedPriceChange ? formatPercentChange(trackedPriceChange) : '—'
  const safetyEvidence = safetyFromSnapshot(snapshot)

  return {
    ...base,
    name: tokenRef?.name ?? base.name,
    symbol: tokenRef?.symbol ?? base.symbol,
    address: tokenRef?.address ?? base.address,
    chain: tokenRef?.chainId === 8453n ? 'Base' : base.chain,
    price: price ? formatUsdPrice(price) : '—',
    priceEth: snapshot.displayPriceQuote ? `${snapshot.displayPriceQuote} ${snapshot.quoteToken?.symbol ?? nativeAssetForChain(base.chain)}` : '—',
    marketCap: marketCap ? formatUsdCompact(marketCap) : '—',
    depth24h: depth ? formatUsdCompact(depth) : '—',
    txns24h: stats ? formatCount(stats.buys + stats.sells) : '—',
    statsWindow: snapshot.stats1h ? '1h' : snapshot.stats5m ? '5m' : undefined,
    priceChange,
    fdv: marketCap ? formatUsdCompact(marketCap) : '—',
    totalSupply: formatTokenAmount(snapshot.totalSupply),
    thin: snapshot.thin,
    volume24h: stats?.volumeUsd ? formatUsdCompact(new Decimal(stats.volumeUsd)) : '—',
    pools: snapshot.pools.map((pool) => ({
      venue: pool.venue,
      feeTier: `${new Decimal(pool.poolFeeBps).div(100).toFixed(2)}%`,
      address: shortenHash(pool.address),
      explorerUrl: `https://basescan.org/address/${pool.address}`,
    })),
    safetyEvidence,
    safetyNote: safetyEvidence.sellable.value,
  }
}

function mapTrades(trades: Trade[], token: TokenWorkspaceFixture, serverTimeMs: bigint | undefined) {
  return trades.map((trade) => ({
    id: `${trade.blockHash}:${trade.logIndex.toString()}`,
    side: trade.side === Trade_Side.BUY ? 'Buy' as const : 'Sell' as const,
    amount: `${trade.tokenAmount} ${trade.token?.symbol ?? token.symbol}`,
    value: trade.valueUsd ? formatUsdCompact(new Decimal(trade.valueUsd)) : '—',
    wallet: shortenHash(trade.sender),
    time: serverTimeMs === undefined ? '—' : formatAge(serverTimeMs - trade.blockTimeMs) + ' ago',
    price: trade.priceUsd ? formatUsdPrice(new Decimal(trade.priceUsd)) : '—',
    marketCap: token.marketCap,
    gas: '—',
    trader: shortenHash(trade.sender),
    tracking: 'Untracked',
    txHash: shortenHash(trade.txHash),
    txUrl: `https://basescan.org/tx/${trade.txHash}`,
  }))
}

function mapCandles(candles: Candle[]): TokenChartData {
  const sorted = candles.slice().sort((a, b) => (a.openTimeMs < b.openTimeMs ? -1 : a.openTimeMs > b.openTimeMs ? 1 : 0))
  return {
    candles: sorted.map((candle) => ({
      time: candle.openTimeMs.toString(),
      open: Number(candle.openUsd),
      high: Number(candle.highUsd),
      low: Number(candle.lowUsd),
      close: Number(candle.closeUsd),
    })),
    volume: sorted.map((candle) => ({
      time: candle.openTimeMs.toString(),
      value: Number(candle.volumeUsd),
      tone: Number(candle.closeUsd) >= Number(candle.openUsd) ? 'up' as const : 'down' as const,
    })),
  }
}

function safetyFromSnapshot(snapshot: TokenSnapshot): TokenWorkspaceFixture['safetyEvidence'] {
  const report = snapshot.safety
  if (!report?.roundTrip) {
    return notCheckedSafety('Awaiting safety service')
  }

  const status: TokenSafetyCheck['status'] = report.verdict === ProtoSafetyVerdict.PASSED
    ? 'passed'
    : report.verdict === ProtoSafetyVerdict.HONEYPOT
      ? 'failed'
      : 'warning'
  const sellable = report.roundTrip.sellSucceeded
    ? { status, value: status === 'passed' ? 'Sell simulation passed' : 'Review sell simulation', detail: report.roundTrip.revertReason || 'Round-trip simulation completed.' }
    : { status: 'failed' as const, value: 'Sell simulation failed', detail: report.roundTrip.revertReason || 'The simulated sell did not complete.' }

  return {
    sellable,
    buyTax: { status, value: `${report.roundTrip.buyTaxPct || '0'}%`, detail: 'Measured by the latest round-trip simulation.' },
    sellTax: { status, value: `${report.roundTrip.sellTaxPct || '0'}%`, detail: 'Measured by the latest round-trip simulation.' },
    updatedLabel: 'Checked from latest safety report',
  }
}

function notCheckedSafety(updatedLabel: string): TokenWorkspaceFixture['safetyEvidence'] {
  const check: TokenSafetyCheck = { status: 'not-checked', value: 'Not checked yet', detail: 'No safety report is available for this token.' }
  return { sellable: check, buyTax: check, sellTax: check, updatedLabel }
}

function formatTokenAmount(value: string): string {
  const decimal = parseOptionalDecimal(value)
  if (!decimal) return '—'
  if (decimal.gte('1e9')) return `${decimal.div('1e9').toDecimalPlaces(1).toFixed()}B`
  if (decimal.gte('1e6')) return `${decimal.div('1e6').toDecimalPlaces(1).toFixed()}M`
  if (decimal.gte('1e3')) return `${decimal.div('1e3').toDecimalPlaces(1).toFixed()}K`
  return decimal.toFixed()
}

function shortenHash(value: string): string {
  if (value.length <= 14) return value
  return `${value.slice(0, 6)}…${value.slice(-4)}`
}
