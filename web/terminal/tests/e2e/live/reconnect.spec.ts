// The live-source build against a stand-in API server (#63): the WebSocket is played by
// page.routeWebSocket, so the test can kill it and watch the terminal recover.
import { expect, test, type Page, type WebSocketRoute } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { LIVE_STREAM_URL } from './liveApi'

const statusFixture = JSON.parse(
  readFileSync(new URL('../../../src/mocks/fixtures/api/v1/EngineStatus.json', import.meta.url), 'utf8'),
) as Record<string, unknown>

type Connection = { route: WebSocketRoute; topics: string[]; timer: ReturnType<typeof setInterval> }

/** Plays the stream server: a status snapshot per subscribe, and a heartbeat every 500ms. */
async function playStreamServer(page: Page, mode: 'MODE_LIVE' | 'MODE_REPLAY' = 'MODE_LIVE') {
  const connections: Connection[] = []
  await page.routeWebSocket(LIVE_STREAM_URL, (route) => {
    const connection: Connection = {
      route,
      topics: [],
      timer: setInterval(() => route.send(JSON.stringify({ heartbeat: { serverTimeMs: '1', headBlockNumber: '1' } })), 500),
    }
    connections.push(connection)
    route.onMessage((data) => {
      const message = JSON.parse(String(data)) as { subscribe?: { topic: string } }
      if (!message.subscribe) return
      connection.topics.push(message.subscribe.topic)
      if (message.subscribe.topic === 'status') {
        route.send(JSON.stringify({ snapshot: { topic: 'status', seq: '1', status: { ...statusFixture, mode } } }))
      }
    })
    route.onClose(() => clearInterval(connection.timer))
  })
  return connections
}

const marketData = (page: Page, label: string) =>
  page.getByRole('status', { name: new RegExp(`^Market data: ${label}\\.`) })

test('switching to the live source is an env change: the same app streams from the API', async ({ page }) => {
  const connections = await playStreamServer(page)
  await page.goto('/')
  await expect(marketData(page, 'Live')).toBeVisible()
  expect(connections).toHaveLength(1)
  // The header's status and Discover's feed.
  expect([...connections[0]!.topics].sort()).toEqual(['discovery', 'status'])
})

test('killing the WebSocket server shows Reconnecting, then recovers and resubscribes without a reload', async ({ page }) => {
  const connections = await playStreamServer(page)
  await page.goto('/')
  await expect(marketData(page, 'Live')).toBeVisible()
  const navigations: string[] = []
  page.on('framenavigated', (frame) => navigations.push(frame.url()))

  clearInterval(connections[0]!.timer)
  await connections[0]!.route.close({ code: 1011, reason: 'server killed' })

  await expect(marketData(page, 'Reconnecting')).toBeVisible()
  await expect(marketData(page, 'Live')).toBeVisible({ timeout: 10_000 })
  expect(connections).toHaveLength(2)
  expect(connections[1]!.topics).toEqual(connections[0]!.topics)
  expect(navigations).toEqual([])
})

test('the header says Replay when the engine reports a replay', async ({ page }) => {
  await playStreamServer(page, 'MODE_REPLAY')
  await page.goto('/')
  await expect(marketData(page, 'Replay')).toBeVisible()
})

test('missed heartbeats show Stale', async ({ page }) => {
  const connections = await playStreamServer(page)
  await page.goto('/')
  await expect(marketData(page, 'Live')).toBeVisible()
  clearInterval(connections[0]!.timer)
  await expect(marketData(page, 'Stale')).toBeVisible({ timeout: 6_000 })
})
