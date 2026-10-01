// Snapshot-then-delta merging and sequencing (#63, stream.proto).
import { create, type MessageInitShape } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'
import {
  CandleSchema,
  CandleSeriesSchema,
  DiscoveryFeedSchema,
  EngineStatusSchema,
  TokenSnapshotSchema,
  TokenTickSchema,
  TradeListSchema,
  TradeSchema,
} from '../generated/omnimarket/api/v1/market_pb'
import { PositionSchema, TradeStatusSchema } from '../generated/omnimarket/api/v1/trading_pb'
import { OrderStatusSchema, OrderStatus_State } from '../generated/omnimarket/api/v1/automation_pb'
import {
  AccountSnapshotSchema,
  ServerMessageSchema,
  StreamError_Code,
} from '../generated/omnimarket/api/v1/stream_pb'
import { apiFixture } from '../../mocks/fixtures/api'
import {
  applyServerMessage,
  forgetTopic,
  initialStreamState,
  markAllResubscribing,
  markSubscribed,
  MAX_TRADES_PER_TOPIC,
  type StreamState,
} from './state'
import { candlesTopic, tokenTopic, tradesTopic } from './topics'

const NOVA = '0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567'
const OTHER = '0x00000000000000000000000000000000000000aa'
const novaTopic = tokenTopic(NOVA)

type Kind = MessageInitShape<typeof ServerMessageSchema>['kind']
type SnapshotPayload = NonNullable<Extract<Kind, { case: 'snapshot' }>['value']['payload']>
type DeltaPayload = NonNullable<Extract<Kind, { case: 'delta' }>['value']['payload']>

function apply(state: StreamState, kind: Kind) {
  return applyServerMessage(state, create(ServerMessageSchema, { kind }))
}

function snapshot(state: StreamState, topic: string, seq: bigint, payload: SnapshotPayload): StreamState {
  return apply(state, { case: 'snapshot', value: { topic, seq, payload } }).state
}

function delta(state: StreamState, topic: string, seq: bigint, payload: DeltaPayload) {
  return apply(state, { case: 'delta', value: { topic, seq, payload } })
}

function tick(price: string, address = NOVA) {
  return create(TokenTickSchema, {
    chainId: 8453n,
    token: address,
    displayPriceUsd: price,
    displayPriceQuote: '0.000005',
    marketCapUsd: '1',
    depthUsd: '2',
    blockNumber: 36120451n,
    blockTimeMs: 1790000002000n,
  })
}

function tokenSnapshot(address = NOVA) {
  const fixture = apiFixture(TokenSnapshotSchema)
  return { ...fixture, token: fixture.token && { ...fixture.token, address } }
}

/** The token topic subscribed and its snapshot (seq 10) applied. */
function liveToken(): StreamState {
  const subscribed = markSubscribed(initialStreamState(), novaTopic)
  return snapshot(subscribed, novaTopic, 10n, { case: 'token', value: tokenSnapshot() })
}

describe('snapshot then delta', () => {
  it('applies the snapshot, then merges a tick into the token without touching other fields', () => {
    const live = liveToken()
    expect(live.topics[novaTopic]).toEqual({ phase: 'live', seq: 10n })
    expect(live.tokens[novaTopic]?.displayPriceUsd).toBe('0.0124')

    const { state, resubscribe } = delta(live, novaTopic, 11n, { case: 'tokenTick', value: tick('0.01252') })
    expect(resubscribe).toEqual([])
    const token = state.tokens[novaTopic]!
    expect(token.displayPriceUsd).toBe('0.01252')
    expect(token.blockNumber).toBe(36120451n)
    // Fields the tick doesn't carry keep the snapshot's values.
    expect(token.pools).toBe(live.tokens[novaTopic]!.pools)
    expect(token.safety).toBe(live.tokens[novaTopic]!.safety)
    expect(state.topics[novaTopic]).toEqual({ phase: 'live', seq: 11n })
  })

  it('keeps amounts as the exact strings the server sent', () => {
    const price = '0.000000000000000000123456789012345678901'
    const { state } = delta(liveToken(), novaTopic, 11n, { case: 'tokenTick', value: tick(price) })
    expect(state.tokens[novaTopic]?.displayPriceUsd).toBe(price)
  })

  it('drops a delta that arrives before its topic\'s snapshot', () => {
    const subscribed = markSubscribed(initialStreamState(), novaTopic)
    const { state, resubscribe } = delta(subscribed, novaTopic, 1n, { case: 'tokenTick', value: tick('1') })
    expect(state).toBe(subscribed)
    expect(resubscribe).toEqual([])
  })

  it('ignores a snapshot or delta for a topic that isn\'t subscribed', () => {
    const empty = initialStreamState()
    expect(snapshot(empty, novaTopic, 1n, { case: 'token', value: tokenSnapshot() })).toBe(empty)
    expect(delta(empty, novaTopic, 1n, { case: 'tokenTick', value: tick('1') }).state).toBe(empty)
  })

  it('a new snapshot replaces the topic\'s state and restarts its seq', () => {
    const ticked = delta(liveToken(), novaTopic, 11n, { case: 'tokenTick', value: tick('9') }).state
    const resubscribed = snapshot(markSubscribed(ticked, novaTopic), novaTopic, 3n, { case: 'token', value: tokenSnapshot() })
    expect(resubscribed.tokens[novaTopic]?.displayPriceUsd).toBe('0.0124')
    expect(resubscribed.topics[novaTopic]).toEqual({ phase: 'live', seq: 3n })
    expect(delta(resubscribed, novaTopic, 4n, { case: 'tokenTick', value: tick('2') }).state.tokens[novaTopic]?.displayPriceUsd).toBe('2')
  })
})

describe('out-of-order deltas', () => {
  it('drops a duplicate or older delta', () => {
    const at11 = delta(liveToken(), novaTopic, 11n, { case: 'tokenTick', value: tick('11') }).state
    for (const seq of [11n, 10n, 5n]) {
      const { state, resubscribe } = delta(at11, novaTopic, seq, { case: 'tokenTick', value: tick('old') })
      expect(state).toBe(at11)
      expect(resubscribe).toEqual([])
    }
    expect(at11.tokens[novaTopic]?.displayPriceUsd).toBe('11')
  })

  it('on a gap, discards the topic\'s state and asks for a resubscribe', () => {
    const { state, resubscribe } = delta(liveToken(), novaTopic, 12n, { case: 'tokenTick', value: tick('12') })
    expect(resubscribe).toEqual([novaTopic])
    expect(state.tokens[novaTopic]).toBeUndefined()
    expect(state.topics[novaTopic]).toEqual({ phase: 'loading', seq: 0n })
  })

  it('after a gap, drops the late delta that filled it until the new snapshot', () => {
    const gapped = delta(liveToken(), novaTopic, 12n, { case: 'tokenTick', value: tick('12') }).state
    const late = delta(gapped, novaTopic, 11n, { case: 'tokenTick', value: tick('11') })
    expect(late.state).toBe(gapped)
    expect(late.resubscribe).toEqual([])
  })

  it('a gap on one topic leaves other topics alone', () => {
    const otherTopic = tokenTopic(OTHER)
    let state = liveToken()
    state = snapshot(markSubscribed(state, otherTopic), otherTopic, 1n, { case: 'token', value: tokenSnapshot(OTHER) })
    const other = state.tokens[otherTopic]
    const gapped = delta(state, novaTopic, 99n, { case: 'tokenTick', value: tick('x') }).state
    expect(gapped.tokens[otherTopic]).toBe(other)
    expect(gapped.topics[otherTopic]).toEqual({ phase: 'live', seq: 1n })
  })
})

describe('resubscribe after a reconnect', () => {
  it('keeps the data on screen, marked resubscribing, until the new snapshot', () => {
    const live = liveToken()
    const resubscribing = markAllResubscribing(live)
    expect(resubscribing.topics[novaTopic]).toEqual({ phase: 'resubscribing', seq: 0n })
    expect(resubscribing.tokens[novaTopic]).toBe(live.tokens[novaTopic])
    // Deltas from the old connection's numbering don't apply.
    expect(delta(resubscribing, novaTopic, 11n, { case: 'tokenTick', value: tick('1') }).state).toBe(resubscribing)
    const back = snapshot(resubscribing, novaTopic, 1n, { case: 'token', value: tokenSnapshot() })
    expect(back.topics[novaTopic]).toEqual({ phase: 'live', seq: 1n })
  })

  it('forgetting a topic drops its subscription and data', () => {
    const forgotten = forgetTopic(liveToken(), novaTopic)
    expect(forgotten.topics[novaTopic]).toBeUndefined()
    expect(forgotten.tokens[novaTopic]).toBeUndefined()
  })
})

describe('per-topic merging', () => {
  it('appends new trades to the front, skips a repeat, and caps the list', () => {
    const topic = tradesTopic(NOVA)
    const list = apiFixture(TradeListSchema)
    let state = snapshot(markSubscribed(initialStreamState(), topic), topic, 1n, { case: 'trades', value: list })
    const fresh = create(TradeSchema, { ...list.trades[0], logIndex: 999n, blockHash: '0xfeed' })
    state = delta(state, topic, 2n, { case: 'trade', value: fresh }).state
    expect(state.trades[topic]?.[0]?.logIndex).toBe(999n)
    expect(state.trades[topic]).toHaveLength(list.trades.length + 1)

    state = delta(state, topic, 3n, { case: 'trade', value: fresh }).state
    expect(state.trades[topic]).toHaveLength(list.trades.length + 1)

    for (let i = 0; i < MAX_TRADES_PER_TOPIC; i += 1) {
      state = delta(state, topic, 4n + BigInt(i), {
        case: 'trade',
        value: create(TradeSchema, { blockHash: '0xabc', logIndex: BigInt(i) }),
      }).state
    }
    expect(state.trades[topic]).toHaveLength(MAX_TRADES_PER_TOPIC)
  })

  it('upserts candles by open time and keeps them oldest first', () => {
    const topic = candlesTopic(NOVA, '1m')
    const series = apiFixture(CandleSeriesSchema)
    let state = snapshot(markSubscribed(initialStreamState(), topic), topic, 1n, { case: 'candles', value: series })
    const last = series.candles.at(-1)!
    state = delta(state, topic, 2n, { case: 'candle', value: create(CandleSchema, { ...last, closeUsd: '7', closed: true }) }).state
    expect(state.candles[topic]).toHaveLength(series.candles.length)
    expect(state.candles[topic]?.at(-1)?.closeUsd).toBe('7')

    const next = create(CandleSchema, { ...last, openTimeMs: last.openTimeMs + 60_000n, closed: false })
    const early = create(CandleSchema, { ...last, openTimeMs: 1n })
    state = delta(state, topic, 3n, { case: 'candle', value: next }).state
    state = delta(state, topic, 4n, { case: 'candle', value: early }).state
    const times = state.candles[topic]!.map((c) => c.openTimeMs)
    expect(times).toEqual([...times].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)))
    expect(times[0]).toBe(1n)
    expect(times.at(-1)).toBe(next.openTimeMs)
  })

  it('upserts and removes discovery rows by token', () => {
    const feed = apiFixture(DiscoveryFeedSchema)
    let state = snapshot(markSubscribed(initialStreamState(), 'discovery'), 'discovery', 1n, { case: 'discovery', value: feed })
    const first = feed.rows[0]!
    const key = first.token!.address
    expect(state.discovery?.order[0]).toBe(key)

    const untouched = feed.rows[1] ? state.discovery!.rows[feed.rows[1].token!.address] : undefined
    state = delta(state, 'discovery', 2n, { case: 'discoveryRow', value: { ...first, displayPriceUsd: '5' } }).state
    expect(state.discovery?.rows[key]?.displayPriceUsd).toBe('5')
    expect(state.discovery?.order).toHaveLength(feed.rows.length)
    if (untouched) expect(state.discovery!.rows[feed.rows[1]!.token!.address]).toBe(untouched)

    state = delta(state, 'discovery', 3n, {
      case: 'discoveryRowRemoved',
      value: { chainId: 8453n, token: key },
    }).state
    expect(state.discovery?.rows[key]).toBeUndefined()
    expect(state.discovery?.order).not.toContain(key)
  })

  it('keeps the server clock from the latest heartbeat, and nothing else', () => {
    const before = liveToken()
    const { state } = apply(before, { case: 'heartbeat', value: { serverTimeMs: 1790000001000n, headBlockNumber: 1n } })
    expect(state.serverTimeMs).toBe(1790000001000n)
    expect(state.tokens).toBe(before.tokens)
    expect(state.topics).toBe(before.topics)
  })

  it('replaces engine status with each delta', () => {
    const status = apiFixture(EngineStatusSchema)
    let state = snapshot(markSubscribed(initialStreamState(), 'status'), 'status', 1n, { case: 'status', value: status })
    state = delta(state, 'status', 2n, { case: 'status', value: { ...status, headBlockNumber: 1n } }).state
    expect(state.status?.headBlockNumber).toBe(1n)
  })

  it('upserts account records by key and applies an order status to its order', () => {
    const account = apiFixture(AccountSnapshotSchema)
    let state = snapshot(markSubscribed(initialStreamState(), 'account'), 'account', 1n, { case: 'account', value: account })
    const order = account.orders[0]!
    const position = account.positions[0]!

    state = delta(state, 'account', 2n, {
      case: 'orderStatus',
      value: create(OrderStatusSchema, { orderId: order.orderId, state: OrderStatus_State.FILLED }),
    }).state
    expect(state.account?.orders[order.orderId]?.status?.state).toBe(OrderStatus_State.FILLED)

    state = delta(state, 'account', 3n, {
      case: 'position',
      value: create(PositionSchema, { ...position, amount: '1.5' }),
    }).state
    expect(state.account?.positions[position.token!.address]?.amount).toBe('1.5')

    state = delta(state, 'account', 4n, {
      case: 'tradeStatus',
      value: create(TradeStatusSchema, { tradeId: 'trade_new' }),
    }).state
    expect(state.account?.trades.trade_new).toBeDefined()
    expect(Object.keys(state.account!.positions)).toHaveLength(account.positions.length)
  })
})

describe('account order status', () => {
  it('ignores a status for an order the client hasn\'t seen', () => {
    const account = apiFixture(AccountSnapshotSchema)
    const state = snapshot(markSubscribed(initialStreamState(), 'account'), 'account', 1n, { case: 'account', value: account })
    const { state: next } = delta(state, 'account', 2n, {
      case: 'orderStatus',
      value: create(OrderStatusSchema, { orderId: 'order_unknown', state: OrderStatus_State.FILLED }),
    })
    expect(next.account?.orders).toBe(state.account?.orders)
    expect(next.account?.orders.order_unknown).toBeUndefined()
    // The delta still counts: the next seq applies.
    expect(next.topics.account).toEqual({ phase: 'live', seq: 2n })
  })
})

describe('stream errors', () => {
  it('marks the topic unavailable and drops its data', () => {
    const state = apply(liveToken(), {
      case: 'error',
      value: { code: StreamError_Code.UNAVAILABLE, topic: novaTopic, message: 'engine stalled' },
    }).state
    expect(state.topics[novaTopic]).toEqual({ phase: 'unavailable', seq: 0n, error: 'engine stalled' })
    expect(state.tokens[novaTopic]).toBeUndefined()
  })

  it('leaves state alone for an error about the whole connection', () => {
    const live = liveToken()
    expect(apply(live, { case: 'error', value: { code: StreamError_Code.RATE_LIMITED, message: 'slow down' } }).state).toBe(live)
  })
})
