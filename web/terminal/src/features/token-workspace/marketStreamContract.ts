import type {
  TokenChartInterval,
  TokenMarketSnapshot,
  TokenWorkspaceStreamStatus,
} from '../../domains/market/tokenWorkspace'

export type MarketStreamRequest = {
  tokenId: string
  interval: TokenChartInterval
}

type MarketStreamEventBase = {
  tokenId: string
  sequence: number
  serverTime: string
}

export type MarketStreamEvent =
  | (MarketStreamEventBase & { type: 'snapshot'; snapshot: TokenMarketSnapshot })
  | (MarketStreamEventBase & { type: 'update'; snapshot: TokenMarketSnapshot })
  | (MarketStreamEventBase & { type: 'heartbeat' })
  | (MarketStreamEventBase & {
    type: 'error'
    error: {
      code: 'STREAM_UNAVAILABLE' | 'INVALID_SUBSCRIPTION' | 'UNKNOWN'
      message: string
      retryable: boolean
    }
  })

export type MarketStreamHandlers = {
  onEvent: (event: MarketStreamEvent) => void
  onStatus: (status: TokenWorkspaceStreamStatus) => void
}

export type MarketStreamClient = {
  subscribe: (request: MarketStreamRequest, handlers: MarketStreamHandlers) => () => void
}

/**
 * Applies transport events to the view model while rejecting out-of-order
 * market updates. Heartbeats keep the existing snapshot; errors preserve the
 * last values so the UI can explain what happened without flashing blank data.
 */
export function applyMarketStreamEvent(current: TokenMarketSnapshot, event: MarketStreamEvent): TokenMarketSnapshot {
  if (event.sequence < current.sequence && event.type !== 'heartbeat') return current

  if (event.type === 'snapshot' || event.type === 'update') return event.snapshot
  if (event.type === 'heartbeat') return current

  return {
    ...current,
    state: 'error',
    updatedLabel: event.error.retryable ? 'Stream reconnecting' : 'Stream unavailable',
  }
}
