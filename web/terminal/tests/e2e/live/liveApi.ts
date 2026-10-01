// The live-source build's API address (playwright.config.ts builds it with this). Nothing listens
// there: tests play the API server with page.routeWebSocket.
export const LIVE_API_URL = 'http://127.0.0.1:4175'
export const LIVE_STREAM_URL = 'ws://127.0.0.1:4175/v1/stream'
