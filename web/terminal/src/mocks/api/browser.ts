// Starts MSW in the browser for the fixture source (#63). Loaded only when VITE_DATA_SOURCE is
// `fixtures`, so replay and live builds never intercept anything.

import { setupWorker } from 'msw/browser'
import type { DataSource } from '../../api/source'
import { fixtureHandlers } from './handlers'

export async function startFixtureWorker(source: DataSource): Promise<void> {
  const worker = setupWorker(...fixtureHandlers(source))
  await worker.start({
    // Assets, fonts and the app's own modules pass through untouched.
    onUnhandledRequest: 'bypass',
    quiet: true,
    serviceWorker: { url: `${import.meta.env.BASE_URL}mockServiceWorker.js` },
  })
}
