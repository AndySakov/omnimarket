// Storybook decorator: gives a story a stream in a fixed connection state, without a socket, and
// the REST client and demo session the header's account menu needs (signed out unless given one).
import type { Decorator } from '@storybook/react-vite'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { Session } from '../api/generated/omnimarket/api/v1/trading_pb'
import { ApiClientContext } from '../api/queries'
import { createApiClient } from '../api/rest'
import { SessionStore } from '../api/session'
import type { DataSourceKind } from '../api/source'
import { StreamManager } from '../api/stream/manager'
import { StreamProvider } from '../api/stream/StreamProvider'
import type { ConnectionState } from '../api/stream/state'
import { SessionContext } from '../api/useSession'

export function withStream(source: DataSourceKind, connection: ConnectionState, session?: Session): Decorator {
  const manager = new StreamManager({ url: 'ws://storybook.invalid/v1/stream' })
  manager.store.setState({ connection })
  const sessions = new SessionStore({ now: () => 0 })
  if (session) sessions.set(session)
  const client = createApiClient({ baseUrl: 'http://storybook.invalid' })
  const queryClient = new QueryClient()
  return (Story) => (
    <QueryClientProvider client={queryClient}>
      <ApiClientContext.Provider value={client}>
        <SessionContext.Provider value={sessions}>
          <StreamProvider manager={manager} source={source}>
            <Story />
          </StreamProvider>
        </SessionContext.Provider>
      </ApiClientContext.Provider>
    </QueryClientProvider>
  )
}
