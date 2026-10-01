// Where the terminal's data comes from (#63). One build-time env variable picks the source; the
// REST client and the stream manager are the same code for all three:
//   fixtures  MSW serves the contract fixtures in the browser, REST and WebSocket alike. Offline.
//   replay    a replay server (#88) playing back a recorded session through the real API.
//   live      the API server (#78) following the chain.
// replay and live differ only in their URL and in what the header calls them.

export type DataSourceKind = 'fixtures' | 'replay' | 'live'

export type DataSource = {
  kind: DataSourceKind
  /** REST base, without the `/v1` prefix, e.g. `http://127.0.0.1:8080`. */
  apiUrl: string
  /** The WebSocket stream, e.g. `ws://127.0.0.1:8080/v1/stream`. */
  wsUrl: string
}

/** The env variables the source reads; Vite inlines them at build time. */
export type DataSourceEnv = {
  VITE_DATA_SOURCE?: string
  VITE_API_URL?: string
  VITE_WS_URL?: string
}

// Fixture mode never touches the network: MSW intercepts these URLs in the page.
const FIXTURE_API_URL = 'http://fixtures.omnimarket.invalid'

export class DataSourceError extends Error {}

export function readDataSource(env: DataSourceEnv): DataSource {
  const kind = env.VITE_DATA_SOURCE?.trim() || 'fixtures'
  if (kind !== 'fixtures' && kind !== 'replay' && kind !== 'live') {
    throw new DataSourceError(`VITE_DATA_SOURCE must be fixtures, replay or live, not "${kind}"`)
  }

  const configured = env.VITE_API_URL?.trim()
  if (kind !== 'fixtures' && !configured) {
    throw new DataSourceError(`VITE_DATA_SOURCE=${kind} needs VITE_API_URL, e.g. http://127.0.0.1:8080`)
  }
  const apiUrl = trimSlash(kind === 'fixtures' ? (configured || FIXTURE_API_URL) : configured!)
  const wsUrl = env.VITE_WS_URL?.trim() || streamUrlFor(apiUrl)
  return { kind, apiUrl, wsUrl }
}

/** `http(s)://host` → `ws(s)://host/v1/stream`. */
export function streamUrlFor(apiUrl: string): string {
  const url = new URL(apiUrl)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  url.pathname = `${trimSlash(url.pathname)}/v1/stream`
  return url.toString()
}

function trimSlash(value: string): string {
  return value.replace(/\/+$/, '')
}
