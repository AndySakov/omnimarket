export type Chain = 'Base' | 'BNB' | 'Solana' | 'Ethereum'

export type SafetyState = 'passed' | 'review' | 'blocked'

export type SafetyEvidence = {
  liquidity: string
  taxes: string
  ownership: string
  state: SafetyState
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
}
