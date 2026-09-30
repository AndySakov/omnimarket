import { useState } from 'react'
import { GlobalHeader } from './components/GlobalHeader'

export default function App() {
  const [activeRoute, setActiveRoute] = useState('Discover')

  return (
    <div className="terminal-app">
      <GlobalHeader activeRoute={activeRoute} onNavigate={setActiveRoute} />
      <main className="shell-preview" aria-label="Terminal workspace preview">
        <div className="shell-preview__eyebrow">{activeRoute} workspace</div>
        <h1>Market context stays below the shell.</h1>
        <p>
          This foundation slice locks the global navigation, context controls, and responsive
          behavior before discovery data and trading surfaces are added.
        </p>
      </main>
    </div>
  )
}
