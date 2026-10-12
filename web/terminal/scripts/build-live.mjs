import { spawnSync } from 'node:child_process'
import process from 'node:process'
import { fileURLToPath, URL } from 'node:url'

const viteBin = fileURLToPath(new URL('../node_modules/vite/bin/vite.js', import.meta.url))
const result = spawnSync(process.execPath, [viteBin, 'build', '--outDir', 'dist-live'], {
  env: {
    ...process.env,
    VITE_DATA_SOURCE: 'live',
    VITE_API_URL: process.env.VITE_API_URL ?? 'http://127.0.0.1:4175',
  },
  stdio: 'inherit',
})

if (result.error) globalThis.console.error(result.error)
process.exit(result.status ?? 1)
