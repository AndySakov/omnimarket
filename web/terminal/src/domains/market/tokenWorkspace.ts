import type { Chain, DiscoveryToken, SafetyState } from './token'

export type TokenWorkspaceState = 'ready' | 'loading' | 'missing' | 'stale'

export type TokenMarketTab = 'Trades' | 'Positions' | 'Orders' | 'Holders' | 'Top Traders' | 'Dev Token'

export type TradePanelMode = 'Market' | 'Limit' | 'DCA' | 'Advanced'

export type TradeQuoteState = 'idle' | 'loading' | 'fresh' | 'expired' | 'error'

export type TradeExecutionState = 'idle' | 'review' | 'signing-disabled' | 'pending' | 'failed'

export type PricePoint = {
  time: string
  value: number
}

export type TokenCandle = {
  time: string
  open: number
  high: number
  low: number
  close: number
}

export type TokenVolumePoint = {
  time: string
  value: number
  tone: 'up' | 'down'
}

export type TokenChartData = {
  candles: TokenCandle[]
  volume: TokenVolumePoint[]
}

export type TokenActivity = {
  id: string
  side: 'Buy' | 'Sell'
  amount: string
  value: string
  wallet: string
  time: string
}

export type TokenTradeRow = TokenActivity & {
  marketCap: string
  gas: string
  trader: string
  tracking: string
}

export type TokenPosition = {
  id: string
  wallet: string
  side: 'Long' | 'Short'
  size: string
  entry: string
  pnl: string
}

export type TokenOrder = {
  id: string
  type: 'Limit' | 'Stop'
  side: 'Buy' | 'Sell'
  amount: string
  trigger: string
  status: 'Open' | 'Filled' | 'Cancelled'
}

export type TokenHolder = {
  id: string
  wallet: string
  share: string
  balance: string
  label: string
}

export type TopTrader = {
  id: string
  wallet: string
  volume: string
  realizedPnl: string
  winRate: string
}

export type TokenWorkspaceFixture = DiscoveryToken & {
  address: string
  price: string
  priceChange: string
  fdv: string
  volume24h: string
  pair: string
  pricePoints: PricePoint[]
  chart: TokenChartData
  activity: TokenActivity[]
  trades: TokenTradeRow[]
  positions: TokenPosition[]
  orders: TokenOrder[]
  holderRows: TokenHolder[]
  topTraders: TopTrader[]
  developerToken: {
    wallet: string
    balance: string
    share: string
    lastAction: string
  }
  pool: string
  poolShare: string
  topHolders: string
  safetyNote: string
}

export type TradeDraft = {
  side: 'Buy' | 'Sell' | 'Auto'
  amount: string
  asset: 'ETH' | 'SOL' | 'BNB'
  quoteState: TradeQuoteState
}

export type SafetyTone = SafetyState | 'neutral'

export function nativeAssetForChain(chain: Chain): TradeDraft['asset'] {
  if (chain === 'Solana') return 'SOL'
  if (chain === 'BNB') return 'BNB'
  return 'ETH'
}
