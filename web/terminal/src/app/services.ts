// The terminal's connection to its data (#63): one REST client, one stream manager and one query
// cache, all pointed at the source the env chose (api/source.ts), and the demo session whose token
// both send (#66).

import { QueryClient } from '@tanstack/react-query'
import { createApiClient, type ApiClient } from '../api/rest'
import { browserSessionStorage, SessionStore } from '../api/session'
import { readDataSource, type DataSource, type DataSourceEnv } from '../api/source'
import { StreamManager, type StreamManagerOptions } from '../api/stream/manager'

export type ApiServices = {
  source: DataSource
  client: ApiClient
  streams: StreamManager
  queryClient: QueryClient
  session: SessionStore
}

export function createApiServices(
  source: DataSource,
  streamOptions: Partial<StreamManagerOptions> = {},
  // In memory unless the caller persists it: tests and stories start signed out.
  session: SessionStore = new SessionStore(),
): ApiServices {
  const getSessionToken = () => session.token()
  return {
    source,
    session,
    client: createApiClient({ baseUrl: source.apiUrl, getSessionToken }),
    streams: new StreamManager({ url: source.wsUrl, getSessionToken, ...streamOptions }),
    queryClient: new QueryClient({
      defaultOptions: {
        // Streams keep market data current; a snapshot query refetches only when asked to.
        queries: { staleTime: 30_000, refetchOnWindowFocus: false, retry: 2 },
      },
    }),
  }
}

let starting: Promise<ApiServices> | undefined

/**
 * Reads the source from the env and, for fixtures, starts the MSW worker first. Called once per
 * page: React's StrictMode runs effects twice, and the worker must start only once.
 */
export function startApiServices(env: DataSourceEnv): Promise<ApiServices> {
  starting ??= (async () => {
    const source = readDataSource(env)
    if (source.kind === 'fixtures') {
      // Loaded on demand, so replay and live builds never start the mocks.
      const { startFixtureWorker } = await import('../mocks/api/browser')
      await startFixtureWorker(source)
    }
    return createApiServices(source, {}, new SessionStore({ storage: browserSessionStorage() }))
  })()
  return starting
}
