import { useEffect, type ReactNode } from 'react'
import { QueryClientProvider } from '@tanstack/react-query'
import { ApiClientContext } from '../api/queries'
import { StreamProvider } from '../api/stream/StreamProvider'
import type { ApiServices } from './services'

/** Provides the REST client, the query cache and the stream, and keeps the stream connected while mounted. */
export function ApiProvider({ services, children }: { services: ApiServices; children: ReactNode }) {
  const { streams } = services
  useEffect(() => {
    streams.start()
    return () => streams.stop()
  }, [streams])

  return (
    <QueryClientProvider client={services.queryClient}>
      <ApiClientContext.Provider value={services.client}>
        <StreamProvider manager={streams} source={services.source.kind}>
          {children}
        </StreamProvider>
      </ApiClientContext.Provider>
    </QueryClientProvider>
  )
}
