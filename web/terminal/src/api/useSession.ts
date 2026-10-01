// React access to the demo session (#66). The header's wallet menu starts a guest session, shows
// the account and signs out; anything that needs the account reads it through these hooks.

import { createContext, useContext } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useStore } from 'zustand'
import type { Session } from './generated/omnimarket/api/v1/trading_pb'
import { useApiClient } from './queries'
import type { SessionStore } from './session'

export const SessionContext = createContext<SessionStore | null>(null)

export function useSessionStore(): SessionStore {
  const store = useContext(SessionContext)
  if (!store) throw new Error('session hooks need a SessionContext provider')
  return store
}

/** The current session, or undefined when signed out. */
export function useSession(): Session | undefined {
  return useStore(useSessionStore().store, (state) => state.session)
}

/** Starts a guest demo account and keeps its session: one click from landing to a shadow balance. */
export function useStartGuestSession() {
  const client = useApiClient()
  const store = useSessionStore()
  return useMutation({
    mutationFn: () => client.createGuestSession(),
    onSuccess: (session) => store.set(session),
  })
}
