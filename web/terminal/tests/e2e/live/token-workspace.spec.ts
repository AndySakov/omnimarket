import { expect, test, type Page, type WebSocketRoute } from '@playwright/test'
import { readFileSync } from 'node:fs'
import type { CandleSeries, EngineStatus, TokenSnapshot, TradeList } from '../../../src/api/generated/omnimarket/api/v1/market_pb'
import { LIVE_STREAM_URL } from './liveApi'

const tokenSnapshot = JSON.parse(
  readFileSync(new URL('../../../src/mocks/fixtures/api/v1/TokenSnapshot.json', import.meta.url), 'utf8'),
) as unknown as TokenSnapshot
const tradeList = JSON.parse(
  readFileSync(new URL('../../../src/mocks/fixtures/api/v1/TradeList.json', import.meta.url), 'utf8'),
) as unknown as TradeList
const candleSeries = JSON.parse(
  readFileSync(new URL('../../../src/mocks/fixtures/api/v1/CandleSeries.json', import.meta.url), 'utf8'),
) as unknown as CandleSeries
const status = JSON.parse(
  readFileSync(new URL('../../../src/mocks/fixtures/api/v1/EngineStatus.json', import.meta.url), 'utf8'),
) as unknown as EngineStatus

type Connection = {
  route: WebSocketRoute
  topics: string[]
  unsubscribed: string[]
  intervalSnapshotHeld: boolean
}

async function playTokenStream(page: Page): Promise<Connection[]> {
  const connections: Connection[] = []
  await page.routeWebSocket(LIVE_STREAM_URL, (route) => {
    const connection: Connection = { route, topics: [], unsubscribed: [], intervalSnapshotHeld: false }
    connections.push(connection)
    route.onMessage((data) => {
      const message = JSON.parse(String(data)) as { subscribe?: { topic: string }; unsubscribe?: { topic: string } }
      if (message.unsubscribe) {
        connection.unsubscribed.push(message.unsubscribe.topic)
        return
      }
      const topic = message.subscribe?.topic
      if (!topic) return
      connection.topics.push(topic)
      if (topic === 'status') {
        route.send(JSON.stringify({ snapshot: { topic, seq: '1', status } }))
        return
      }
      if (topic === 'discovery') return
      if (topic.startsWith('token:')) {
        route.send(JSON.stringify({ snapshot: { topic, seq: '1', token: { ...tokenSnapshot, token: { ...tokenSnapshot.token, address: topic.slice('token:'.length) } } } }))
        return
      }
      if (topic.startsWith('trades:')) {
        route.send(JSON.stringify({ snapshot: { topic, seq: '1', trades: tradeList } }))
        return
      }
      if (topic.endsWith(':1h')) {
        connection.intervalSnapshotHeld = true
        return
      }
      if (topic.startsWith('candles:')) {
        route.send(JSON.stringify({ snapshot: { topic, seq: '1', candles: candleSeries } }))
      }
    })
  })
  return connections
}

test('token workspace subscribes to shared topics, updates from deltas, and cleans up', async ({ page }) => {
  const connections = await playTokenStream(page)
  const address = '0x9a1b2c3d4e5f60718293a4b5c6d7e8f901234567'
  await page.goto(`/base/token/${address}`)

  await expect(page.getByRole('main', { name: /token workspace/i })).toBeVisible()
  await expect(page.getByText('Live stream connected', { exact: true })).toBeVisible()
  await expect.poll(() => connections[0]?.topics.filter((topic) => topic.startsWith('token:') || topic.startsWith('trades:') || topic.startsWith('candles:')).sort()).toEqual([
    `candles:${address}:5m`,
    `token:${address}`,
    `trades:${address}`,
  ])

  const tokenTopic = `token:${address}`
  const tradeTopic = `trades:${address}`
  const candleTopic = `candles:${address}:5m`
  const socket = connections[0]!.route
  socket.send(JSON.stringify({ delta: { topic: tokenTopic, seq: '2', tokenTick: { ...tokenSnapshot, displayPriceUsd: '0.0133', displayPriceQuote: '0.00000532', marketCapUsd: '13300000', depthUsd: '184300.5', blockNumber: '36120451', blockTimeMs: '1790000002000', token: address } } }))
  socket.send(JSON.stringify({ delta: { topic: tradeTopic, seq: '2', trade: { ...tradeList.trades[0], txHash: `0x${'a'.repeat(64)}`, blockHash: `0x${'b'.repeat(64)}`, logIndex: '8', valueUsd: '125' } } }))
  socket.send(JSON.stringify({ delta: { topic: candleTopic, seq: '2', candle: { ...candleSeries.candles[2], closeUsd: '0.0133', openTimeMs: '1790000060000' } } }))

  await expect(page.getByText('$0.0133', { exact: true }).first()).toBeVisible()
  await expect(page.getByText('$125', { exact: true })).toBeVisible()
  await expect(page.getByRole('img', { name: /candlestick price chart/ })).toBeVisible()

  await page.getByRole('tab', { name: '1h', exact: true }).click()
  await expect(page.getByText('Loading 1h chart', { exact: true })).toBeVisible()
  await expect.poll(() => connections[0]?.unsubscribed).toContain(candleTopic)
  await expect.poll(() => connections[0]?.topics).toContain(`candles:${address}:1h`)

  await page.locator('.token-page').getByRole('button', { name: 'Discover' }).click()
  await expect(page.getByRole('heading', { name: 'Discover market radar' })).toBeVisible()
  await expect.poll(() => connections[0]?.unsubscribed).toEqual(expect.arrayContaining([tokenTopic, tradeTopic, candleTopic]))
})
