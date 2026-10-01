import { useEffect, useState } from 'react'
import type { EngineStatus } from '../../api/generated/omnimarket/api/v1/market_pb'
import type { DataSourceKind } from '../../api/source'
import type { DataState } from '../../api/stream/dataState'
import { useConnection, useEngineStatusStream } from '../../api/stream/hooks'
import { engineStatusState, HEAD_STALE_AFTER_MS } from './engineStatus'

/**
 * The stream's engine status plus a client-side check that the head is still moving, ticking once
 * a second so the bar goes stale on its own when blocks stop arriving.
 */
export function useEngineStatus(staleAfterMs: number = HEAD_STALE_AFTER_MS): {
  status: EngineStatus | undefined
  state: DataState
  source: DataSourceKind
} {
  const { source } = useConnection()
  const { data: status, state: streamState } = useEngineStatusStream()
  const head = status?.headBlockNumber
  const [headSeenAtMs, setHeadSeenAtMs] = useState<number | undefined>(undefined)
  const [nowMs, setNowMs] = useState(() => Date.now())

  useEffect(() => {
    if (head === undefined) return
    const now = Date.now()
    setHeadSeenAtMs(now)
    setNowMs(now)
  }, [head])

  useEffect(() => {
    const timer = setInterval(() => setNowMs(Date.now()), 1_000)
    return () => clearInterval(timer)
  }, [])

  return { status, state: engineStatusState(streamState, headSeenAtMs, nowMs, staleAfterMs), source }
}
