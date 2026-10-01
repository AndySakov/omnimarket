import { useEffect, useMemo, useState } from 'react'
import type {
  TokenChartInterval,
  TokenMarketSnapshot,
  TokenWorkspaceStreamStatus,
  TokenWorkspaceStreamState,
} from '../../domains/market/tokenWorkspace'
import { getTokenMarketSnapshot, getTokenWorkspaceSnapshot, createTokenWorkspaceStream } from './tokenWorkspaceData'

type UseTokenWorkspaceStreamOptions = {
  tokenId?: string
  interval: TokenChartInterval
}

export function useTokenWorkspaceStream({ tokenId, interval }: UseTokenWorkspaceStreamOptions) {
  const workspace = useMemo(() => getTokenWorkspaceSnapshot(tokenId), [tokenId])
  const [snapshot, setSnapshot] = useState<TokenMarketSnapshot>(() => getTokenMarketSnapshot(workspace.token, interval))
  const [state, setState] = useState<TokenWorkspaceStreamState>('loading')
  const [connection, setConnection] = useState<TokenWorkspaceStreamStatus>({ state: 'connecting', attempt: 0, label: 'Connecting to fixture stream' })

  useEffect(() => {
    setState('loading')
    setConnection({ state: 'connecting', attempt: 0, label: 'Connecting to fixture stream' })
    const unsubscribe = createTokenWorkspaceStream(workspace.token, interval, (nextSnapshot) => {
      setSnapshot(nextSnapshot)
      setState(nextSnapshot.state)
    }, { onStatus: setConnection })

    return unsubscribe
  }, [interval, workspace.token])

  return { snapshot, state, connection }
}
