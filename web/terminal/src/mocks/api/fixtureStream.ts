// The fixture source's WebSocket server (#63): answers subscribes with the contract fixtures as
// snapshots, then moves token prices and the discovery feed with deterministic ticks, so the
// terminal looks alive offline.
// Pure: `receive` and `tick` return the messages to send; the MSW handler owns the timers.

import { create, type MessageInitShape } from '@bufbuild/protobuf'
import Decimal from 'decimal.js'
import {
  CandleSeriesSchema,
  EngineStatusSchema,
  TokenSnapshotSchema,
  TradeListSchema,
  type TokenSnapshot,
} from '../../api/generated/omnimarket/api/v1/market_pb'
import {
  AccountSnapshotSchema,
  ServerMessageSchema,
  StreamError_Code,
  type ClientMessage,
  type ServerMessage,
} from '../../api/generated/omnimarket/api/v1/stream_pb'
import { topicKind } from '../../api/stream/topics'
import { apiFixture } from '../fixtures/api'
import { FixtureDiscovery } from './discoveryFeed'

// Relative price moves, applied in turn on each tick. They sum to zero, so prices wander but don't drift.
const TICK_STEPS = ['0.004', '-0.002', '0.003', '-0.005', '0.001', '-0.001'].map((step) => new Decimal(step))

type TopicCursor = { seq: bigint; ticks: number; base?: TokenSnapshot; discovery?: FixtureDiscovery }

export class FixtureStream {
  private readonly topics = new Map<string, TopicCursor>()
  private heartbeats = 0n

  /** The replies to one client message. */
  receive(message: ClientMessage): ServerMessage[] {
    switch (message.kind.case) {
      case 'subscribe': {
        const { topic, sessionToken } = message.kind.value
        return this.subscribe(topic, sessionToken)
      }
      case 'unsubscribe':
        this.topics.delete(message.kind.value.topic)
        return []
      case undefined:
        return []
    }
  }

  /** One second of stream: a heartbeat, a price tick on each subscribed token topic, and the discovery feed's changes. */
  tick(): ServerMessage[] {
    this.heartbeats += 1n
    const status = apiFixture(EngineStatusSchema)
    const serverTimeMs = status.headBlockTimeMs + this.heartbeats * 1000n
    const out: ServerMessage[] = [
      server({
        case: 'heartbeat',
        value: {
          serverTimeMs,
          headBlockNumber: status.headBlockNumber + this.heartbeats / 2n,
        },
      }),
    ]
    for (const [topic, cursor] of this.topics) {
      if (cursor.discovery) {
        cursor.ticks += 1
        for (const payload of cursor.discovery.tick(cursor.ticks, serverTimeMs)) {
          cursor.seq += 1n
          out.push(server({ case: 'delta', value: { topic, seq: cursor.seq, payload } }))
        }
        continue
      }
      if (!cursor.base) continue
      cursor.seq += 1n
      cursor.ticks += 1
      out.push(server({
        case: 'delta',
        value: { topic, seq: cursor.seq, payload: { case: 'tokenTick', value: tickFor(cursor.base, cursor.ticks) } },
      }))
    }
    return out
  }

  private subscribe(topic: string, sessionToken: string): ServerMessage[] {
    const kind = topicKind(topic)
    if (!kind) return [streamError(StreamError_Code.UNKNOWN_TOPIC, topic, `no such topic: ${topic}`)]
    if (kind === 'account' && !sessionToken) {
      return [streamError(StreamError_Code.UNAUTHENTICATED, topic, 'the account topic needs a session token')]
    }
    const address = topic.split(':')[1] ?? ''
    const cursor: TopicCursor = { seq: 1n, ticks: 0 }
    let payload: MessageInitShape<typeof ServerMessageSchema>['kind'] & { case: 'snapshot' }
    switch (kind) {
      case 'status':
        payload = snapshot(topic, { case: 'status', value: apiFixture(EngineStatusSchema) })
        break
      case 'discovery': {
        const status = apiFixture(EngineStatusSchema)
        // The feed counts its ages from the heartbeat's clock, which starts at the head block.
        cursor.discovery = new FixtureDiscovery(status.headBlockTimeMs + this.heartbeats * 1000n, status.headBlockNumber)
        payload = snapshot(topic, { case: 'discovery', value: cursor.discovery.snapshot() })
        break
      }
      case 'token': {
        const fixture = apiFixture(TokenSnapshotSchema)
        // One fixture token stands in for any address the UI asks about.
        cursor.base = { ...fixture, token: fixture.token && { ...fixture.token, address } }
        payload = snapshot(topic, { case: 'token', value: cursor.base })
        break
      }
      case 'trades':
        payload = snapshot(topic, { case: 'trades', value: apiFixture(TradeListSchema) })
        break
      case 'candles':
        payload = snapshot(topic, { case: 'candles', value: apiFixture(CandleSeriesSchema) })
        break
      case 'account':
        payload = snapshot(topic, { case: 'account', value: apiFixture(AccountSnapshotSchema) })
        break
    }
    this.topics.set(topic, cursor)
    return [create(ServerMessageSchema, { kind: payload })]
  }
}

function tickFor(base: TokenSnapshot, ticks: number) {
  let factor = new Decimal(1)
  for (let i = 0; i < ticks; i += 1) factor = factor.times(new Decimal(1).plus(TICK_STEPS[i % TICK_STEPS.length]))
  const scale = (value: string) => (value ? new Decimal(value).times(factor).toSignificantDigits(8).toFixed() : value)
  return {
    lineage: base.lineage,
    chainId: base.token?.chainId ?? 0n,
    token: base.token?.address ?? '',
    displayPriceUsd: scale(base.displayPriceUsd),
    displayPriceQuote: scale(base.displayPriceQuote),
    marketCapUsd: scale(base.marketCapUsd),
    depthUsd: base.depthUsd,
    thin: base.thin,
    blockNumber: base.blockNumber + BigInt(ticks),
    blockTimeMs: base.blockTimeMs + BigInt(ticks) * 2000n,
  }
}

function snapshot(
  topic: string,
  payload: NonNullable<MessageInitShape<typeof ServerMessageSchema>['kind'] & { case: 'snapshot' }>['value']['payload'],
) {
  return { case: 'snapshot' as const, value: { topic, seq: 1n, payload } }
}

function server(kind: MessageInitShape<typeof ServerMessageSchema>['kind']): ServerMessage {
  return create(ServerMessageSchema, { kind })
}

function streamError(code: StreamError_Code, topic: string, message: string): ServerMessage {
  return server({ case: 'error', value: { code, topic, message } })
}
