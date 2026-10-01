import type { ReactNode } from 'react'
import { useConnection } from '../../api/stream/hooks'
import { GlobalHeader } from '../../components/GlobalHeader'

type TerminalShellProps = {
  activeRoute: string
  onNavigate: (route: string) => void
  children: ReactNode
}

export function TerminalShell({ activeRoute, onNavigate, children }: TerminalShellProps) {
  const connection = useConnection()
  return (
    <div className="terminal-app">
      <GlobalHeader activeRoute={activeRoute} onNavigate={onNavigate} connection={connection} />
      {children}
    </div>
  )
}
