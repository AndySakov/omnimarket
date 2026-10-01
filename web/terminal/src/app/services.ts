// The terminal's connection to its data (#63): one REST client, one stream manager and one query
// cache, all pointed at the source the env chose (api/source.ts).

import { QueryClient } from '@tanstack/react-query'
import { createApiClient, type ApiClient } from '../api/rest'
import type { DataSource } from '../api/source'
import { StreamManager, type StreamManagerOptions } from '../api/stream/manager'

export type ApiServices = {
  source: DataSource
  client: ApiClient
  streams: StreamManager
  queryClient: QueryClient
}

export function createApiServices(
  source: DataSource,
  streamOptions: Partial<StreamManagerOptions> = {},
): ApiServices {
  return {
    source,
    client: createApiClient({ baseUrl: source.apiUrl }),
    streams: new StreamManager({ url: source.wsUrl, ...streamOptions }),
    queryClient: new QueryClient({
      defaultOptions: {
        // Streams keep market data current; a snapshot query refetches only when asked to.
        queries: { staleTime: 30_000, refetchOnWindowFocus: false, retry: 2 },
      },
    }),
  }
}
