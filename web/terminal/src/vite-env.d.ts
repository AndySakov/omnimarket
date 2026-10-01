/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** fixtures (the default), replay or live; see src/api/source.ts. */
  readonly VITE_DATA_SOURCE?: string
  /** The API's base URL; required for replay and live. */
  readonly VITE_API_URL?: string
  /** The stream's URL; defaults to VITE_API_URL with ws(s) and `/v1/stream`. */
  readonly VITE_WS_URL?: string
}
