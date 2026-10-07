import { fromJson, fromJsonString, toJsonString, type JsonValue } from '@bufbuild/protobuf'
import { expect, test, type Page, type WebSocketRoute } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { ClientMessageSchema, ServerMessageSchema } from '../../../src/api/generated/omnimarket/api/v1/stream_pb'
import { LIVE_STREAM_URL } from './liveApi'

type JsonObject = { [key: string]: JsonValue }
type DiscoveryFixture = { rows: JsonObject[]; blockNumber: string }

const discoveryFixture = JSON.parse(
  readFileSync(new URL('../../../src/mocks/fixtures/api/v1/DiscoveryFeed.json', import.meta.url), 'utf8'),
) as DiscoveryFixture
const statusFixture = JSON.parse(
  readFileSync(new URL('../../../src/mocks/fixtures/api/v1/EngineStatus.json', import.meta.url), 'utf8'),
) as JsonObject

const LIVE_TOKEN = '0x1111111111111111111111111111111111111111'
const LIVE_POOL = '0x2222222222222222222222222222222222222222'
const LIVE_BLOCK_TIME = 1_790_000_004_000n

type Connection = { route: WebSocketRoute; topics: string[]; timer: ReturnType<typeof setInterval> }

function send(route: WebSocketRoute, message: JsonObject): void {
  route.send(toJsonString(ServerMessageSchema, fromJson(ServerMessageSchema, message)))
}

function liveRow(withSafety: boolean): JsonObject {
  const source = discoveryFixture.rows[0]!
  const token = source.token as JsonObject
  return {
    ...source,
    token: { ...token, address: LIVE_TOKEN, symbol: 'LIVE', name: 'Live Pool' },
    pool: LIVE_POOL,
    poolCreatedBlock: '36120452',
    poolCreatedAtMs: String(LIVE_BLOCK_TIME),
    displayPriceUsd: '0.0133',
    marketCapUsd: '13300000',
    depthUsd: '184300.5',
    stats5m: { volumeUsd: '125', buys: '1', sells: '0', priceChangePct: '1.5' },
    stats1h: { volumeUsd: '125', buys: '1', sells: '0', priceChangePct: '1.5' },
    statsTracked: { volumeUsd: '125', buys: '1', sells: '0', priceChangePct: '1.5' },
    safety: withSafety ? { verdict: 'SAFETY_VERDICT_PASSED', buyTaxPct: '0', sellTaxPct: '0', checkedAtMs: String(LIVE_BLOCK_TIME) } : {},
    rank: 1,
    blockNumber: '36120452',
  }
}

async function playLiveDiscovery(page: Page): Promise<Connection[]> {
  const connections: Connection[] = []
  await page.routeWebSocket(LIVE_STREAM_URL, (route) => {
    const timer = setInterval(() => {
      send(route, { heartbeat: { serverTimeMs: String(LIVE_BLOCK_TIME), headBlockNumber: '36120452' } })
    }, 500)
    const connection: Connection = { route, topics: [], timer }
    connections.push(connection)
    route.onClose(() => clearInterval(timer))
    route.onMessage((data) => {
      const message = fromJsonString(ClientMessageSchema, String(data))
      if (message.kind.case !== 'subscribe') return
      const { topic } = message.kind.value
      connection.topics.push(topic)
      if (topic === 'status') {
        send(route, {
          snapshot: {
            topic,
            seq: '1',
            status: { ...statusFixture, mode: 'MODE_LIVE' },
          },
        })
        return
      }
      if (topic === 'discovery') {
        send(route, {
          snapshot: {
            topic,
            seq: '1',
            discovery: discoveryFixture,
          },
        })
      }
    })
  })
  return connections
}

test('live Discover inserts a Base pool without reload and renders the API safety verdict', async ({ page }) => {
  const connections = await playLiveDiscovery(page)
  await page.goto('/')

  await expect(page.getByRole('status', { name: /^Market data: Live\./ })).toBeVisible()
  const liveRowLocator = page.getByRole('row').filter({ hasText: 'Live Pool' })
  await expect(page.getByRole('row').filter({ hasText: 'NovaSet (fixture)' })).toBeVisible()
  await expect(liveRowLocator).toHaveCount(0)
  expect(connections[0]?.topics).toEqual(['status', 'discovery'])

  const navigations: string[] = []
  page.on('framenavigated', (frame) => navigations.push(frame.url()))

  send(connections[0]!.route, {
    delta: {
      topic: 'discovery',
      seq: '2',
      discoveryRow: liveRow(false),
    },
  })

  await expect(liveRowLocator).toBeVisible()
  await expect(liveRowLocator.getByText('Not checked yet')).toBeVisible()

  send(connections[0]!.route, {
    delta: {
      topic: 'discovery',
      seq: '3',
      discoveryRow: liveRow(true),
    },
  })

  const row = page.getByRole('row').filter({ hasText: 'Live Pool' })
  await expect(row.getByText('Sell check passed')).toBeVisible()
  await expect(row.getByText('0%/0% taxes')).toBeVisible()
  await expect(row.getByText('Not checked yet')).toHaveCount(0)
  expect(navigations).toEqual([])
})
