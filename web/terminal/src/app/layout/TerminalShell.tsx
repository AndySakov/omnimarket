import type { ReactNode } from 'react'
import { EngineStatus_Mode } from '../../api/generated/omnimarket/api/v1/market_pb'
import { useConnection, useEngineStatusStream } from '../../api/stream/hooks'
import { GlobalHeader } from '../../components/GlobalHeader'

type TerminalShellProps = {
  activeRoute: string
  onNavigate: (route: string) => void
  children: ReactNode
}

export function TerminalShell({ activeRoute, onNavigate, children }: TerminalShellProps) {
  const connection = useConnection()
  // The header is always on screen, so the engine's status topic is always subscribed.
  const engineMode = useEngineStatusStream().data?.mode
  return (
    <div className="terminal-app">
      <GlobalHeader
        activeRoute={activeRoute}
        onNavigate={onNavigate}
        connection={{
          ...connection,
          engineMode:
            engineMode === EngineStatus_Mode.REPLAY ? 'replay' : engineMode === EngineStatus_Mode.LIVE ? 'live' : undefined,
        }}
      />
      {children}
    </div>
  )
}
