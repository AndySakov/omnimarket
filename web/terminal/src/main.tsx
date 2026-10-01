import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { createApiServices } from './app/services'
import { readDataSource } from './api/source'
import './styles.css'
import './features/discovery-feed/discovery.css'

async function main() {
  const source = readDataSource(import.meta.env)
  if (source.kind === 'fixtures') {
    // Loaded on demand, so a replay or live build never ships the mocks' code path into use.
    const { startFixtureWorker } = await import('./mocks/api/browser')
    await startFixtureWorker(source)
  }

  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App services={createApiServices(source)} />
    </StrictMode>,
  )
}

void main()
