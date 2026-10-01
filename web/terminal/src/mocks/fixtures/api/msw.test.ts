// The API fixtures work as MSW responses: served as JSON, they parse back with the generated
// schemas, as the terminal's REST client will parse the live API (#63).
import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'
import { fromJson } from '@bufbuild/protobuf'
import { TokenSnapshotSchema } from '../../../api/generated/omnimarket/api/v1/market_pb'
import { QuoteSchema } from '../../../api/generated/omnimarket/api/v1/trading_pb'
import { apiFixture, apiFixtureJson } from '.'

const api = 'http://api.test/v1'
const server = setupServer(
  http.get(`${api}/tokens/:chainId/:address`, () => HttpResponse.json(apiFixtureJson(TokenSnapshotSchema))),
  http.post(`${api}/quotes`, () => HttpResponse.json(apiFixtureJson(QuoteSchema))),
)

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterAll(() => server.close())

describe('API fixtures through MSW', () => {
  it('serves a token snapshot that parses into the generated type', async () => {
    const response = await fetch(`${api}/tokens/8453/0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567`)
    const snapshot = fromJson(TokenSnapshotSchema, await response.json())
    expect(snapshot).toEqual(apiFixture(TokenSnapshotSchema))
    expect(snapshot.displayPriceUsd).toBe('0.0124')
    expect(snapshot.blockNumber).toBe(36120450n)
  })

  it('serves a quote whose amounts stay exact strings', async () => {
    const response = await fetch(`${api}/quotes`, { method: 'POST', body: '{}' })
    const quote = fromJson(QuoteSchema, await response.json())
    expect(quote.minAmountOut).toBe('16897.862895')
    expect(quote.route).toHaveLength(1)
  })
})
