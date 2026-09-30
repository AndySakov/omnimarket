import type { ReactNode } from 'react'
import { GlobalHeader } from '../../components/GlobalHeader'

type TerminalShellProps = {
  activeRoute: string
  onNavigate: (route: string) => void
  children: ReactNode
}

export function TerminalShell({ activeRoute, onNavigate, children }: TerminalShellProps) {
  return (
    <div className="terminal-app">
      <GlobalHeader activeRoute={activeRoute} onNavigate={onNavigate} />
      {children}
    </div>
  )
}
