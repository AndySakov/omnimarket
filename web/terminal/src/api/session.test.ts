import { create, toJsonString } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'
import { SessionSchema } from './generated/omnimarket/api/v1/trading_pb'
import { SESSION_STORAGE_KEY, SessionStore, type SessionStorageLike } from './session'

function memoryStorage(initial: Record<string, string> = {}): SessionStorageLike & { items: Map<string, string> } {
  const items = new Map(Object.entries(initial))
  return {
    items,
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => void items.set(key, value),
    removeItem: (key) => void items.delete(key),
  }
}

const session = (token: string, expiresAtMs: bigint) =>
  create(SessionSchema, { sessionToken: token, expiresAtMs, account: { accountId: 'acct_1' } })

describe('SessionStore', () => {
  it('starts signed out with nothing stored', () => {
    const store = new SessionStore({ storage: memoryStorage(), now: () => 1000 })
    expect(store.current()).toBeUndefined()
    expect(store.token()).toBeUndefined()
  })

  it('persists a session, so a new store on the same storage (a reload) is still signed in', () => {
    const storage = memoryStorage()
    new SessionStore({ storage, now: () => 1000 }).set(session('tok_1', 5000n))
    const reloaded = new SessionStore({ storage, now: () => 2000 })
    expect(reloaded.token()).toBe('tok_1')
    expect(reloaded.current()?.account?.accountId).toBe('acct_1')
  })

  it('signing out forgets the session in memory and in storage', () => {
    const storage = memoryStorage()
    const store = new SessionStore({ storage, now: () => 1000 })
    store.set(session('tok_1', 5000n))
    store.clear()
    expect(store.token()).toBeUndefined()
    expect(store.store.getState().session).toBeUndefined()
    expect(storage.items.has(SESSION_STORAGE_KEY)).toBe(false)
  })

  it('drops an expired session, on load and once it expires while open', () => {
    const storage = memoryStorage({ [SESSION_STORAGE_KEY]: toJsonString(SessionSchema, session('old', 1000n)) })
    expect(new SessionStore({ storage, now: () => 1000 }).token()).toBeUndefined()
    expect(storage.items.has(SESSION_STORAGE_KEY)).toBe(false)

    let now = 1000
    const store = new SessionStore({ storage, now: () => now })
    store.set(session('tok_2', 2000n))
    now = 1999
    expect(store.token()).toBe('tok_2')
    now = 2000
    expect(store.token()).toBeUndefined()
    expect(store.store.getState().session).toBeUndefined()
  })

  it('starts signed out from a stored value it cannot parse', () => {
    const storage = memoryStorage({ [SESSION_STORAGE_KEY]: '{not json' })
    expect(new SessionStore({ storage }).current()).toBeUndefined()
    expect(storage.items.has(SESSION_STORAGE_KEY)).toBe(false)
  })

  it('keeps the session in memory when storage throws', () => {
    const throwing: SessionStorageLike = {
      getItem: () => { throw new Error('blocked') },
      setItem: () => { throw new Error('blocked') },
      removeItem: () => { throw new Error('blocked') },
    }
    const store = new SessionStore({ storage: throwing, now: () => 0 })
    store.set(session('tok_3', 0n))
    expect(store.token()).toBe('tok_3')
    store.clear()
    expect(store.token()).toBeUndefined()
  })
})
