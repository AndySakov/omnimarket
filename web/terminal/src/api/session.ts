// The demo session (#66, D101): the `Session` from `POST /v1/session`, kept in localStorage so it
// survives a reload. Its token is the bearer for account requests and the `account` topic. One
// store per page, owned by the API services; the REST client and the stream manager read the token
// from it at each request, so signing in or out needs no rewiring.

import { fromJsonString, toJsonString } from '@bufbuild/protobuf'
import { createStore, type StoreApi } from 'zustand/vanilla'
import { SessionSchema, type Session } from './generated/omnimarket/api/v1/trading_pb'

export const SESSION_STORAGE_KEY = 'omnimarket.session.v1'

/** The part of `localStorage` the store uses, so tests can hand it a map. */
export type SessionStorageLike = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

export type SessionState = { session: Session | undefined }

export type SessionStoreOptions = {
  /** Where the session persists; `undefined` keeps it in memory only. */
  storage?: SessionStorageLike
  /** The wall clock in Unix ms, used only to drop an expired session. */
  now?: () => number
}

export class SessionStore {
  readonly store: StoreApi<SessionState>

  private readonly storage: SessionStorageLike | undefined
  private readonly now: () => number

  constructor(options: SessionStoreOptions = {}) {
    this.storage = options.storage
    this.now = options.now ?? (() => Date.now())
    this.store = createStore<SessionState>(() => ({ session: this.load() }))
  }

  /** The current session, or undefined when signed out or once it has expired. */
  current(): Session | undefined {
    const { session } = this.store.getState()
    if (session && this.expired(session)) {
      this.clear()
      return undefined
    }
    return session
  }

  /** The bearer token for account requests, if signed in. */
  token(): string | undefined {
    return this.current()?.sessionToken || undefined
  }

  /** Keeps a new session, replacing any earlier one. */
  set(session: Session): void {
    this.write(toJsonString(SessionSchema, session))
    this.store.setState({ session })
  }

  /** Signs out: forgets the session here and in storage. */
  clear(): void {
    this.remove()
    if (this.store.getState().session !== undefined) this.store.setState({ session: undefined })
  }

  private load(): Session | undefined {
    const text = this.read()
    if (!text) return undefined
    try {
      const session = fromJsonString(SessionSchema, text, { ignoreUnknownFields: true })
      if (session.sessionToken && !this.expired(session)) return session
    } catch {
      // A session written by an older build, or edited by hand: start signed out.
    }
    this.remove()
    return undefined
  }

  private expired(session: Session): boolean {
    // An unset expiry (0) never expires; the server decides.
    return session.expiresAtMs !== 0n && session.expiresAtMs <= BigInt(this.now())
  }

  // Storage can throw (private windows, blocked site data). The session then lives in memory only.
  private read(): string | null {
    try {
      return this.storage?.getItem(SESSION_STORAGE_KEY) ?? null
    } catch {
      return null
    }
  }

  private write(text: string): void {
    try {
      this.storage?.setItem(SESSION_STORAGE_KEY, text)
    } catch {
      // Memory only.
    }
  }

  private remove(): void {
    try {
      this.storage?.removeItem(SESSION_STORAGE_KEY)
    } catch {
      // Nothing persisted.
    }
  }
}

/** The browser's localStorage, or undefined where reading it throws. */
export function browserSessionStorage(): SessionStorageLike | undefined {
  try {
    return globalThis.localStorage ?? undefined
  } catch {
    return undefined
  }
}
