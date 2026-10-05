import { spawn, spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const apiUrl = process.argv[2] ?? 'http://127.0.0.1:4175'
const viteCli = path.join(projectRoot, 'node_modules', 'vite', 'bin', 'vite.js')
const liveEnvironment = { ...process.env, VITE_DATA_SOURCE: 'live', VITE_API_URL: apiUrl }

const build = spawnSync(process.execPath, [viteCli, 'build', '--outDir', 'dist-live'], {
  cwd: projectRoot,
  env: liveEnvironment,
  stdio: 'inherit',
})

if (build.error) {
  console.error(`Could not start the live Vite build: ${build.error.message}`)
  process.exit(1)
}

if (build.status !== 0) {
  process.exit(build.status ?? 1)
}

const preview = spawn(process.execPath, [viteCli, 'preview', '--outDir', 'dist-live', '--host', '127.0.0.1', '--port', '4174', '--strictPort'], {
  cwd: projectRoot,
  env: process.env,
  stdio: 'inherit',
})

let shuttingDown = false

function shutdown(code) {
  if (shuttingDown) return
  shuttingDown = true
  if (!preview.killed) preview.kill()
  process.exit(code)
}

process.on('SIGINT', () => shutdown(0))
process.on('SIGTERM', () => shutdown(0))
preview.on('error', () => shutdown(1))
preview.on('exit', (code, signal) => shutdown(signal ? 1 : (code ?? 0)))
