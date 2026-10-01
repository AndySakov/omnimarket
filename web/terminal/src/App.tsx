// The terminal's composition root (#63): picks the data source from the env, starts the fixture
// worker when the source is fixtures, and provides the API to the app. Kept apart from
// TerminalApp and main.tsx so screens and routing never deal with where data comes from.
import { useEffect, useState } from 'react'
import TerminalApp from './app/TerminalApp'
import { ApiProvider } from './app/ApiProvider'
import { startApiServices, type ApiServices } from './app/services'

export default function App({ services }: { services?: ApiServices }) {
  const [started, setStarted] = useState<ApiServices | Error | undefined>(services)

  useEffect(() => {
    if (services) return
    let cancelled = false
    startApiServices(import.meta.env).then(
      (ready) => !cancelled && setStarted(ready),
      (error: unknown) => !cancelled && setStarted(error instanceof Error ? error : new Error(String(error))),
    )
    return () => {
      cancelled = true
    }
  }, [services])

  if (started === undefined) return null
  if (started instanceof Error) {
    return (
      <main className="shell-preview" role="alert">
        <div className="shell-preview__eyebrow">Configuration error</div>
        <h1>The terminal can't reach its data.</h1>
        <p>{started.message}</p>
      </main>
    )
  }
  return (
    <ApiProvider services={started}>
      <TerminalApp />
    </ApiProvider>
  )
}
