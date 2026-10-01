import type { TokenWorkspaceStreamOptions } from './tokenWorkspaceData'
import { getTokenWorkspaceSnapshot, createTokenWorkspaceStream } from './tokenWorkspaceData'
import type { TokenMarketSnapshot } from '../../domains/market/tokenWorkspace'
import type {
  MarketStreamClient,
  MarketStreamEvent,
} from './marketStreamContract'

export type FixtureMarketStreamClientOptions = TokenWorkspaceStreamOptions & {
  heartbeatIntervalMs?: number
  errorAfterMs?: number
}

const defaultHeartbeatIntervalMs = 15_000

/**
 * Local adapter with the same subscription boundary the M2 WebSocket client
 * will use. It turns deterministic fixture snapshots into transport events and
 * emits heartbeats without changing the view-model components.
 */
export function createFixtureMarketStreamClient(options: FixtureMarketStreamClientOptions = {}): MarketStreamClient {
  return {
    subscribe: (request, handlers) => {
      const workspace = getTokenWorkspaceSnapshot(request.tokenId)
      let firstEvent = true
      let latestSequence = 0
      let heartbeatTimer: number | undefined
      let errorTimer: number | undefined

      const emitEvent = (snapshot: TokenMarketSnapshot) => {
        latestSequence = snapshot.sequence
        const event: MarketStreamEvent = {
          type: firstEvent ? 'snapshot' : 'update',
          tokenId: workspace.token.id,
          sequence: snapshot.sequence,
          serverTime: fixtureServerTime(snapshot.sequence),
          snapshot,
        }
        firstEvent = false
        handlers.onEvent(event)
        if (heartbeatTimer === undefined) {
          heartbeatTimer = window.setInterval(() => {
            handlers.onEvent({
              type: 'heartbeat',
              tokenId: workspace.token.id,
              sequence: latestSequence,
              serverTime: fixtureServerTime(latestSequence),
            })
          }, options.heartbeatIntervalMs ?? defaultHeartbeatIntervalMs)
        }
        if (options.errorAfterMs !== undefined && errorTimer === undefined) {
          errorTimer = window.setTimeout(() => {
            handlers.onEvent({
              type: 'error',
              tokenId: workspace.token.id,
              sequence: latestSequence,
              serverTime: fixtureServerTime(latestSequence),
              error: { code: 'STREAM_UNAVAILABLE', message: 'Fixture gateway unavailable', retryable: true },
            })
            handlers.onStatus({ state: 'error', attempt: 0, label: 'Stream unavailable · retrying' })
            streamUnsubscribe()
            if (heartbeatTimer !== undefined) window.clearInterval(heartbeatTimer)
          }, options.errorAfterMs)
        }
      }

      const streamUnsubscribe = createTokenWorkspaceStream(workspace.token, request.interval, emitEvent, {
        ...options,
        onStatus: handlers.onStatus,
      })

      return () => {
        streamUnsubscribe()
        if (heartbeatTimer !== undefined) window.clearInterval(heartbeatTimer)
        if (errorTimer !== undefined) window.clearTimeout(errorTimer)
      }
    },
  }
}

function fixtureServerTime(sequence: number): string {
  return new Date(Date.UTC(2026, 8, 30, 9, sequence)).toISOString()
}
