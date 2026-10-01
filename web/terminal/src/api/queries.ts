// TanStack Query access to the REST client (#63): snapshots a screen loads once, and commands.
// Streamed data comes from the stream hooks instead (stream/react.tsx).

import { createContext, useContext } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import type { MessageInitShape } from '@bufbuild/protobuf'
import type { QuoteRequestSchema } from './generated/omnimarket/api/v1/trading_pb'
import type { ApiClient, CandlesQuery, DiscoveryQuery, TradesQuery } from './rest'

export const ApiClientContext = createContext<ApiClient | null>(null)

export function useApiClient(): ApiClient {
  const client = useContext(ApiClientContext)
  if (!client) throw new Error('API hooks need an ApiClientContext provider')
  return client
}

/** Query keys, one factory per endpoint, so invalidation can target an endpoint or one entry. */
export const apiKeys = {
  status: () => ['status'] as const,
  discovery: (query: DiscoveryQuery) => ['discovery', query] as const,
  token: (chainId: bigint, address: string) => ['token', chainId.toString(), address.toLowerCase()] as const,
  trades: (query: TradesQuery) => ['trades', query.chainId.toString(), query.token.toLowerCase(), query.limit] as const,
  candles: (query: CandlesQuery) =>
    [
      'candles',
      query.chainId.toString(),
      query.token.toLowerCase(),
      query.interval,
      query.fromMs?.toString(),
      query.toMs?.toString(),
    ] as const,
}

export function useStatusQuery() {
  const client = useApiClient()
  return useQuery({ queryKey: apiKeys.status(), queryFn: ({ signal }) => client.getStatus(signal) })
}

export function useDiscoveryQuery(query: DiscoveryQuery = {}) {
  const client = useApiClient()
  return useQuery({ queryKey: apiKeys.discovery(query), queryFn: ({ signal }) => client.getDiscovery(query, signal) })
}

export function useTokenQuery(chainId: bigint, address: string) {
  const client = useApiClient()
  return useQuery({
    queryKey: apiKeys.token(chainId, address),
    queryFn: ({ signal }) => client.getToken(chainId, address, signal),
  })
}

export function useTradesQuery(query: TradesQuery) {
  const client = useApiClient()
  return useQuery({ queryKey: apiKeys.trades(query), queryFn: ({ signal }) => client.getTrades(query, signal) })
}

export function useCandlesQuery(query: CandlesQuery) {
  const client = useApiClient()
  return useQuery({ queryKey: apiKeys.candles(query), queryFn: ({ signal }) => client.getCandles(query, signal) })
}

export function useQuoteMutation() {
  const client = useApiClient()
  return useMutation({
    mutationFn: (input: MessageInitShape<typeof QuoteRequestSchema>) => client.requestQuote(input),
  })
}
