// MSW handlers for the fixture source (#63): every v0 REST endpoint answers with its contract
// fixture, and the stream is a `FixtureStream` driven by a one-second timer per connection.
// The browser uses them through `startFixtureWorker`; tests use them with `setupServer`.

import { fromJsonString, toJsonString } from '@bufbuild/protobuf'
import { http, HttpResponse, ws, type RequestHandler, type WebSocketHandler } from 'msw'
import {
  DiscoveryFeedSchema,
  EngineStatusSchema,
  TokenSnapshotSchema,
  TradeListSchema,
} from '../../api/generated/omnimarket/api/v1/market_pb'
import {
  AccountSchema,
  PositionListSchema,
  QuoteSchema,
  SessionSchema,
  TradeHistorySchema,
  TradeReceiptSchema,
  TradeStatusSchema,
} from '../../api/generated/omnimarket/api/v1/trading_pb'
import {
  FiringExplanationSchema,
  OrderListSchema,
  OrderSchema,
} from '../../api/generated/omnimarket/api/v1/automation_pb'
import { ClientMessageSchema, ServerMessageSchema } from '../../api/generated/omnimarket/api/v1/stream_pb'
import type { DataSource } from '../../api/source'
import { apiFixtureJson, candleSeriesFixtureJson } from '../fixtures/api'
import { FixtureStream } from './fixtureStream'

export type FixtureHandlerOptions = {
  /** How often the stream sends a heartbeat and price ticks. */
  tickIntervalMs?: number
}

export function fixtureHandlers(
  source: Pick<DataSource, 'apiUrl' | 'wsUrl'>,
  options: FixtureHandlerOptions = {},
): Array<RequestHandler | WebSocketHandler> {
  const api = `${source.apiUrl}/v1`
  const json = (schema: Parameters<typeof apiFixtureJson>[0]) => () => HttpResponse.json(apiFixtureJson(schema))

  const stream = ws.link(source.wsUrl)
  const streamHandler = stream.addEventListener('connection', ({ client }) => {
    const fixtures = new FixtureStream()
    const send = (messages: ReturnType<FixtureStream['tick']>) => {
      for (const message of messages) client.send(toJsonString(ServerMessageSchema, message))
    }
    const timer = setInterval(() => send(fixtures.tick()), options.tickIntervalMs ?? 1000)
    client.addEventListener('message', (event) => {
      if (typeof event.data !== 'string') return
      send(fixtures.receive(fromJsonString(ClientMessageSchema, event.data)))
    })
    client.addEventListener('close', () => clearInterval(timer))
  })

  return [
    http.get(`${api}/status`, json(EngineStatusSchema)),
    http.get(`${api}/discovery`, json(DiscoveryFeedSchema)),
    http.get(`${api}/tokens/:chainId/:address`, json(TokenSnapshotSchema)),
    http.get(`${api}/trades`, json(TradeListSchema)),
    http.get(`${api}/candles`, ({ request }) =>
      HttpResponse.json(candleSeriesFixtureJson(new URL(request.url).searchParams.get('interval') ?? '')),
    ),
    http.post(`${api}/session`, json(SessionSchema)),
    http.get(`${api}/account`, json(AccountSchema)),
    http.get(`${api}/positions`, json(PositionListSchema)),
    http.get(`${api}/history`, json(TradeHistorySchema)),
    http.post(`${api}/quotes`, json(QuoteSchema)),
    http.post(`${api}/trades`, json(TradeStatusSchema)),
    http.get(`${api}/trades/:tradeId/receipt`, json(TradeReceiptSchema)),
    http.get(`${api}/orders`, json(OrderListSchema)),
    http.post(`${api}/orders`, json(OrderSchema)),
    http.patch(`${api}/orders/:orderId`, json(OrderSchema)),
    http.delete(`${api}/orders/:orderId`, json(OrderSchema)),
    http.get(`${api}/firings/:firingId/explanation`, json(FiringExplanationSchema)),
    http.get(`${api}/trades/:tradeId/explanation`, json(FiringExplanationSchema)),
    streamHandler,
  ]
}
