import { useEffect, useMemo, useState } from 'react'
import type {
  TokenChartInterval,
  TokenMarketSnapshot,
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

  useEffect(() => {
    setState('loading')
    const unsubscribe = createTokenWorkspaceStream(workspace.token, interval, (nextSnapshot) => {
      setSnapshot(nextSnapshot)
      setState(nextSnapshot.state)
    })

    return unsubscribe
  }, [interval, workspace.token])

  return { snapshot, state }
}
