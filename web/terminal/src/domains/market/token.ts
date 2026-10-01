export type Chain = 'Base' | 'BNB' | 'MegaETH' | 'Solana' | 'Ethereum'

export type SafetyState = 'passed' | 'review' | 'blocked'

export type SafetyEvidence = {
  liquidity: string
  taxes: string
  ownership: string
  state: SafetyState
  /** Set on live rows (#65): the check's verdict, which `state` only approximates. */
  verdict?: SafetyVerdict
}

export type DiscoveryToken = {
  id: string
  name: string
  symbol: string
  chain: Chain
  age: string
  marketCap: string
  marketCapChange: string
  liquidity: string
  liquidityChange: string
  volume: string
  volumeChange: string
  txns: string
  buys: number
  sells: number
  holders: string
  tags: string[]
  sparkline: number[]
  safety: SafetyEvidence
  avatarTone: 'lime' | 'violet' | 'orange' | 'blue' | 'pink' | 'teal'
  avatarSrc: string
  provisional?: boolean
  /** Set on rows from the live discovery feed (#65); fixture rows leave it unset. */
  live?: LiveRowState
}

/** The safety verdict a live row shows (SafetySummary, D29). "unchecked" is never shown as safe. */
export type SafetyVerdict = 'unchecked' | 'passed' | 'warning' | 'honeypot'

/** What a live discovery row adds to the table row: display text, already formatted, and its motion. */
export type LiveRowState = {
  /** The token's address, lowercase: the row's key. */
  address: string
  /** Display price in USD, formatted. */
  priceUsd: string
  /** The last display-price move; `seq` changes with every move, so the flash replays each time. */
  priceMove?: { direction: 'up' | 'down'; seq: number }
  /** Arrived after the first snapshot: highlighted briefly as it inserts. */
  inserted: boolean
  /** Left the feed while the table was held still: kept in place, dimmed, until the hold ends. */
  departed: boolean
  verdict: SafetyVerdict
  /** Buy and sell tax, formatted, once a check has run. */
  buyTax?: string
  sellTax?: string
  /** No pool above the liquidity floor: priced from its deepest pool (D18, D20). */
  thin: boolean
}
