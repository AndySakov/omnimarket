import { useEffect, useMemo, useState } from 'react'
import type {
  TokenChartInterval,
  TokenMarketSnapshot,
  TokenWorkspaceStreamStatus,
  TokenWorkspaceStreamState,
} from '../../domains/market/tokenWorkspace'
import { getTokenMarketSnapshot, getTokenWorkspaceSnapshot } from './tokenWorkspaceData'
import { applyMarketStreamEvent, type MarketStreamClient } from './marketStreamContract'
import { createFixtureMarketStreamClient } from './marketStreamClient'

const fixtureMarketStreamClient = createFixtureMarketStreamClient()

type UseTokenWorkspaceStreamOptions = {
  tokenId?: string
  interval: TokenChartInterval
  client?: MarketStreamClient
}

export function useTokenWorkspaceStream({ tokenId, interval, client }: UseTokenWorkspaceStreamOptions) {
  const workspace = useMemo(() => getTokenWorkspaceSnapshot(tokenId), [tokenId])
  const streamClient = client ?? fixtureMarketStreamClient
  const [snapshot, setSnapshot] = useState<TokenMarketSnapshot>(() => getTokenMarketSnapshot(workspace.token, interval))
  const [state, setState] = useState<TokenWorkspaceStreamState>('loading')
  const [connection, setConnection] = useState<TokenWorkspaceStreamStatus>({ state: 'connecting', attempt: 0, label: 'Connecting to fixture stream' })

  useEffect(() => {
    setState('loading')
    setConnection({ state: 'connecting', attempt: 0, label: 'Connecting to fixture stream' })
    let currentSnapshot = getTokenMarketSnapshot(workspace.token, interval)
    setSnapshot(currentSnapshot)
    const unsubscribe = streamClient.subscribe({ tokenId: workspace.token.id, interval }, {
      onEvent: (event) => {
        currentSnapshot = applyMarketStreamEvent(currentSnapshot, event)
        setSnapshot(currentSnapshot)
        setState(currentSnapshot.state)
      },
      onStatus: setConnection,
    })

    return unsubscribe
  }, [interval, streamClient, workspace.token])

  return { snapshot, state, connection }
}
