// The REST client against the fixture source's MSW handlers (#63).
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest'
import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'
import { fixtureHandlers } from '../mocks/api/handlers'
import { CandleInterval } from './generated/omnimarket/api/v1/market_pb'
import { ApiError, createApiClient } from './rest'
import { readDataSource } from './source'

const source = readDataSource({})
const server = setupServer(...fixtureHandlers(source))
const NOVA = '0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567'

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(() => server.resetHandlers())
afterAll(() => server.close())

describe('REST client', () => {
  const client = createApiClient({ baseUrl: source.apiUrl, getSessionToken: () => 'sess_1' })

  it('parses snapshots into the generated types, amounts exact and 64-bit values as bigint', async () => {
    const token = await client.getToken(8453n, NOVA)
    expect(token.displayPriceUsd).toBe('0.0124')
    expect(token.blockNumber).toBe(36120450n)

    const status = await client.getStatus()
    expect(status.headBlockNumber).toBe(36120450n)
    expect((await client.getDiscovery({ list: 'trending' })).rows.length).toBeGreaterThan(0)
    expect((await client.getTrades({ chainId: 8453n, token: NOVA, limit: 50 })).trades.length).toBeGreaterThan(0)
    expect((await client.getCandles({ chainId: 8453n, token: NOVA, interval: '1m' })).candles.length).toBeGreaterThan(0)
  })

  it('serves a candle series at each of the 15m, 4h and 1d intervals, spaced by the interval', async () => {
    const cases = [
      ['15m', CandleInterval.CANDLE_INTERVAL_15M, 900_000n],
      ['4h', CandleInterval.CANDLE_INTERVAL_4H, 14_400_000n],
      ['1d', CandleInterval.CANDLE_INTERVAL_1D, 86_400_000n],
    ] as const
    for (const [label, interval, ms] of cases) {
      const series = await client.getCandles({ chainId: 8453n, token: NOVA, interval: label })
      expect(series.interval, label).toBe(interval)
      expect(series.candles.length, label).toBe(8)
      for (const [i, candle] of series.candles.entries()) {
        expect(candle.interval, label).toBe(interval)
        expect(candle.openTimeMs % ms, label).toBe(0n)
        if (i > 0) expect(candle.openTimeMs - series.candles[i - 1]!.openTimeMs, label).toBe(ms)
      }
      // The last is still open, ending at the fixture head's time.
      expect(series.candles.map((c) => c.closed), label).toEqual([true, true, true, true, true, true, true, false])
      expect(series.candles.at(-1)!.openTimeMs, label).toBe(1_790_000_000_000n - (1_790_000_000_000n % ms))
    }
  })

  it('builds query strings and sends the session token', async () => {
    let seen: Request | undefined
    server.use(
      http.get(`${source.apiUrl}/v1/candles`, ({ request }) => {
        seen = request
        return HttpResponse.json({})
      }),
    )
    await client.getCandles({ chainId: 8453n, token: NOVA.toUpperCase().replace('0X', '0x'), interval: '5m', fromMs: 1n })
    const url = new URL(seen!.url)
    expect(Object.fromEntries(url.searchParams)).toEqual({ chain_id: '8453', token: NOVA, interval: '5m', from_ms: '1' })
    expect(seen!.headers.get('authorization')).toBe('Bearer sess_1')
  })

  it('sends a command built from the generated request type', async () => {
    let body: unknown
    server.use(
      http.post(`${source.apiUrl}/v1/quotes`, async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({ quoteId: 'q1', amountOut: '16897.862895' })
      }),
    )
    const quote = await client.requestQuote({ clientRequestId: 'r1', chainId: 8453n, tokenIn: 'a', tokenOut: 'b', amountIn: '0.5' })
    expect(body).toEqual({ clientRequestId: 'r1', chainId: '8453', tokenIn: 'a', tokenOut: 'b', amountIn: '0.5' })
    expect(quote.amountOut).toBe('16897.862895')
  })

  it('ignores fields it doesn\'t know, so the server can add them first', async () => {
    server.use(http.get(`${source.apiUrl}/v1/status`, () => HttpResponse.json({ headBlockNumber: '7', newField: 1 })))
    expect((await client.getStatus()).headBlockNumber).toBe(7n)
  })

  it('throws an ApiError on a failed response or a body of the wrong shape', async () => {
    server.use(http.get(`${source.apiUrl}/v1/status`, () => new HttpResponse('engine down', { status: 503 })))
    await expect(client.getStatus()).rejects.toMatchObject({ name: 'ApiError', status: 503 })

    server.use(http.get(`${source.apiUrl}/v1/status`, () => HttpResponse.json({ headBlockNumber: 'not a number' })))
    await expect(client.getStatus()).rejects.toBeInstanceOf(ApiError)
  })
})
