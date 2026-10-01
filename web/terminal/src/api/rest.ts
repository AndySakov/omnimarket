// The REST client (#63, docs/spec/frontend.md "REST (v0)"). Responses are proto3 JSON and parse
// with the generated schemas, so every field arrives typed: amounts stay exact decimal strings and
// 64-bit integers become bigint. Unknown fields are ignored, so the server can add fields first.

import {
  create,
  fromJson,
  toJson,
  type DescMessage,
  type JsonValue,
  type MessageInitShape,
  type MessageShape,
} from '@bufbuild/protobuf'
import {
  CandleSeriesSchema,
  DiscoveryFeedSchema,
  EngineStatusSchema,
  TokenSnapshotSchema,
  TradeListSchema,
  type CandleSeries,
  type DiscoveryFeed,
  type EngineStatus,
  type TokenSnapshot,
  type TradeList,
} from './generated/omnimarket/api/v1/market_pb'
import { QuoteRequestSchema, QuoteSchema, type Quote } from './generated/omnimarket/api/v1/trading_pb'
import type { CandleIntervalLabel } from './stream/topics'

/** A non-2xx response, or a body that isn't the message the endpoint promises. */
export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly path: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

export type ApiClientOptions = {
  /** The API's base URL, without `/v1`. */
  baseUrl: string
  fetch?: typeof globalThis.fetch
  /** The session's bearer token, read at each request; account endpoints need it. */
  getSessionToken?: () => string | undefined
}

export type DiscoveryQuery = { list?: 'new' | 'trending'; minDepthUsd?: string; maxAgeMs?: bigint }
export type TradesQuery = { chainId: bigint; token: string; limit?: number }
export type CandlesQuery = {
  chainId: bigint
  token: string
  interval: CandleIntervalLabel
  fromMs?: bigint
  toMs?: bigint
}

export type ApiClient = ReturnType<typeof createApiClient>

export function createApiClient(options: ApiClientOptions) {
  const baseUrl = options.baseUrl.replace(/\/+$/, '')
  const doFetch = options.fetch ?? ((input, init) => globalThis.fetch(input, init))
  const getSessionToken = options.getSessionToken ?? (() => undefined)

  async function request<Out extends DescMessage>(
    schema: Out,
    path: string,
    init: { method?: string; body?: JsonValue; signal?: AbortSignal } = {},
  ): Promise<MessageShape<Out>> {
    const headers: Record<string, string> = { Accept: 'application/json' }
    if (init.body !== undefined) headers['Content-Type'] = 'application/json'
    const token = getSessionToken()
    if (token) headers.Authorization = `Bearer ${token}`

    const response = await doFetch(`${baseUrl}${path}`, {
      method: init.method ?? 'GET',
      headers,
      body: init.body === undefined ? undefined : JSON.stringify(init.body),
      signal: init.signal,
    })
    if (!response.ok) {
      const detail = await response.text().catch(() => '')
      throw new ApiError(`${response.status} from ${path}${detail ? `: ${detail}` : ''}`, response.status, path)
    }
    let json: JsonValue
    try {
      json = (await response.json()) as JsonValue
      return fromJson(schema, json, { ignoreUnknownFields: true })
    } catch (error) {
      throw new ApiError(`${path} did not return a ${schema.typeName}: ${String(error)}`, response.status, path)
    }
  }

  return {
    getStatus(signal?: AbortSignal): Promise<EngineStatus> {
      return request(EngineStatusSchema, '/v1/status', { signal })
    },

    getDiscovery(query: DiscoveryQuery = {}, signal?: AbortSignal): Promise<DiscoveryFeed> {
      return request(DiscoveryFeedSchema, `/v1/discovery${queryString({
        list: query.list,
        min_depth_usd: query.minDepthUsd,
        max_age_ms: query.maxAgeMs,
      })}`, { signal })
    },

    getToken(chainId: bigint, address: string, signal?: AbortSignal): Promise<TokenSnapshot> {
      return request(TokenSnapshotSchema, `/v1/tokens/${chainId}/${address.toLowerCase()}`, { signal })
    },

    getTrades(query: TradesQuery, signal?: AbortSignal): Promise<TradeList> {
      return request(TradeListSchema, `/v1/trades${queryString({
        chain_id: query.chainId,
        token: query.token.toLowerCase(),
        limit: query.limit,
      })}`, { signal })
    },

    getCandles(query: CandlesQuery, signal?: AbortSignal): Promise<CandleSeries> {
      return request(CandleSeriesSchema, `/v1/candles${queryString({
        chain_id: query.chainId,
        token: query.token.toLowerCase(),
        interval: query.interval,
        from_ms: query.fromMs,
        to_ms: query.toMs,
      })}`, { signal })
    },

    /** A command: the request is built from the generated type, so it can't drift from the contract. */
    requestQuote(input: MessageInitShape<typeof QuoteRequestSchema>): Promise<Quote> {
      const body = toJson(QuoteRequestSchema, create(QuoteRequestSchema, input))
      return request(QuoteSchema, '/v1/quotes', { method: 'POST', body })
    },
  }
}

function queryString(params: Record<string, string | number | bigint | undefined>): string {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== '') search.set(key, String(value))
  }
  const text = search.toString()
  return text ? `?${text}` : ''
}
