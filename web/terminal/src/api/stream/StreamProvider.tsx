import { useMemo, type ReactNode } from 'react'
import type { DataSourceKind } from '../source'
import { StreamContext } from './hooks'
import type { StreamManager } from './manager'

/** Gives the stream hooks their manager. Starting and stopping the manager is the caller's job. */
export function StreamProvider({
  manager,
  source,
  children,
}: {
  manager: StreamManager
  source: DataSourceKind
  children: ReactNode
}) {
  const value = useMemo(() => ({ manager, source }), [manager, source])
  return <StreamContext.Provider value={value}>{children}</StreamContext.Provider>
}
