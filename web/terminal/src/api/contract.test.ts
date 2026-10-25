import { describe, expect, it } from 'vitest'
import { fromJson, toJson, ScalarType, type DescField, type DescMessage, type JsonValue } from '@bufbuild/protobuf'
import { file_omnimarket_api_v1_automation } from './generated/omnimarket/api/v1/automation_pb'
import { file_omnimarket_api_v1_common } from './generated/omnimarket/api/v1/common_pb'
import { CandleInterval, CandleSeriesSchema, file_omnimarket_api_v1_market, type CandleSeries } from './generated/omnimarket/api/v1/market_pb'
import { file_omnimarket_api_v1_stream } from './generated/omnimarket/api/v1/stream_pb'
import { file_omnimarket_api_v1_trading } from './generated/omnimarket/api/v1/trading_pb'
import { LineageSchema } from './generated/omnimarket/lineage/v1/lineage_pb'
import { apiFixturesJson, candleSeriesFixturesJson } from '../mocks/fixtures/api'
import { decimalField } from '../test/wire'

// The API contract v0 (#75, D91): proto/omnimarket/api/v1, as generated here.
const apiFiles = [
  file_omnimarket_api_v1_common,
  file_omnimarket_api_v1_market,
  file_omnimarket_api_v1_trading,
  file_omnimarket_api_v1_automation,
  file_omnimarket_api_v1_stream,
]
const apiMessages: DescMessage[] = apiFiles.flatMap((file) => file.messages)
const fixtureMessages: DescMessage[] = [...apiMessages, LineageSchema]

// Messages that aren't records, so carry no lineage (D91): requests (they carry a
// client_request_id the server derives their lineage from), the WebSocket envelope around
// records, and parts that only appear inside a record.
const notRecords = new Set([
  'QuoteRequest',
  'TradeRequest',
  'PlaceOrderRequest',
  'UpdateOrderRequest',
  'CancelOrderRequest',
  'ClientMessage',
  'Subscribe',
  'Unsubscribe',
  'ServerMessage',
  'Snapshot',
  'Delta',
  'Heartbeat',
  'StreamError',
  'TokenRef',
  'RouteLeg',
  'Fee',
  'WindowStats',
  'PoolSummary',
  'SafetySummary',
  'Slippage',
  'TradeStepTiming',
  'Balance',
  'OrderLevel',
])

const decimalValue = /^-?(0|[1-9]\d*)(\.\d+)?$/
// Fields holding addresses or hashes: lowercase 0x hex.
const hexField = /^(address|pool|token|token_in|token_out|spend_token|sender|recipient|wallet_address|tx_hash|block_hash|head_block_hash|intent_hash|firing_id|last_firing_id)$/
const hexValue = /^0x[0-9a-f]+$/

function allMessages(roots: DescMessage[]): DescMessage[] {
  const seen = new Map<string, DescMessage>()
  const visit = (desc: DescMessage) => {
    if (seen.has(desc.typeName)) return
    seen.set(desc.typeName, desc)
    desc.nestedMessages.forEach(visit)
    for (const field of desc.fields) {
      if (field.message) visit(field.message)
    }
  }
  roots.forEach(visit)
  return [...seen.values()]
}

function isStringField(field: DescField): boolean {
  return (field.fieldKind === 'scalar' || field.fieldKind === 'list') && field.scalar === ScalarType.STRING
}

type Problem = string

// Walks a fixture alongside its schema, checking each decimal and hex string.
function checkValues(desc: DescMessage, json: JsonValue, path: string, problems: Problem[]) {
  if (json === null || typeof json !== 'object' || Array.isArray(json)) return
  for (const field of desc.fields) {
    const value = json[field.jsonName]
    if (value === undefined) continue
    const values = Array.isArray(value) ? value : [value]
    for (const item of values) {
      const at = `${path}.${field.jsonName}`
      if (field.message) {
        checkValues(field.message, item, at, problems)
      } else if (typeof item === 'string' && decimalField.test(field.name) && !decimalValue.test(item)) {
        problems.push(`${at} = "${item}" isn't a decimal string`)
      } else if (typeof item === 'string' && hexField.test(field.name) && !hexValue.test(item)) {
        problems.push(`${at} = "${item}" isn't lowercase 0x hex`)
      }
    }
  }
}

// Every fixture file, wherever it sits under the API fixtures.
const fixtureFiles = Object.fromEntries(
  Object.entries(import.meta.glob<JsonValue>('../mocks/fixtures/api/**/*.json', { eager: true, import: 'default' })).map(
    ([path, json]) => [path.slice('../mocks/fixtures/api/'.length), json],
  ),
)

// The message a fixture file holds: `v1/<Message>.json` by name, `candles/<interval>.json` a series.
function schemaFor(file: string): DescMessage | undefined {
  const message = /^v1\/(\w+)\.json$/.exec(file)
  if (message) return fixtureMessages.find((desc) => desc.name === message[1])
  if (/^candles\/\w+\.json$/.test(file)) return CandleSeriesSchema
  return undefined
}

// Walks a fixture alongside its schema, checking that each record in it carries a lineage ID.
function checkLineage(desc: DescMessage, json: JsonValue, path: string, problems: Problem[]) {
  if (json === null || typeof json !== 'object' || Array.isArray(json)) return
  if (!notRecords.has(desc.name) && desc.field.lineage?.message?.typeName === LineageSchema.typeName) {
    const id = (json.lineage as { id?: string } | undefined)?.id
    if (id === undefined || atob(id).length !== 16) problems.push(`${path} has no 16-byte lineage ID`)
  }
  for (const field of desc.fields) {
    const value = json[field.jsonName]
    if (!field.message || value === undefined) continue
    const values = Array.isArray(value) ? value : [value]
    values.forEach((item, i) => checkLineage(field.message!, item, Array.isArray(value) ? `${path}.${field.jsonName}[${i}]` : `${path}.${field.jsonName}`, problems))
  }
}

const intervalMs: Partial<Record<CandleInterval, bigint>> = {
  [CandleInterval.CANDLE_INTERVAL_1S]: 1_000n,
  [CandleInterval.CANDLE_INTERVAL_1M]: 60_000n,
  [CandleInterval.CANDLE_INTERVAL_5M]: 300_000n,
  [CandleInterval.CANDLE_INTERVAL_15M]: 900_000n,
  [CandleInterval.CANDLE_INTERVAL_1H]: 3_600_000n,
  [CandleInterval.CANDLE_INTERVAL_4H]: 14_400_000n,
  [CandleInterval.CANDLE_INTERVAL_1D]: 86_400_000n,
}

// Compares two decimal strings exactly: negative, zero or positive as a is below, at or above b.
function compareDecimals(a: string, b: string): number {
  const [aInt, aFrac = ''] = a.split('.')
  const [bInt, bFrac = ''] = b.split('.')
  const digits = Math.max(aFrac.length, bFrac.length)
  const x = BigInt(aInt + aFrac.padEnd(digits, '0'))
  const y = BigInt(bInt + bFrac.padEnd(digits, '0'))
  return x < y ? -1 : x > y ? 1 : 0
}

const toHex = (bytes: Uint8Array | undefined) => [...(bytes ?? [])].map((b) => b.toString(16).padStart(2, '0')).join('')

// A series' own rules: OHLC bounds in each view, candles on the interval's grid and consecutive,
// only the last open, and the series caused by exactly its candles.
function checkSeries(series: CandleSeries, path: string, problems: Problem[]) {
  const step = intervalMs[series.interval]
  if (step === undefined) problems.push(`${path} has no known interval`)
  series.candles.forEach((candle, i) => {
    const at = `${path}.candles[${i}]`
    const views = [
      ['USD', candle.openUsd, candle.highUsd, candle.lowUsd, candle.closeUsd],
      ['quote', candle.openQuote, candle.highQuote, candle.lowQuote, candle.closeQuote],
    ] as const
    for (const [view, open, high, low, close] of views) {
      if (!decimalValue.test(open) || !decimalValue.test(high) || !decimalValue.test(low) || !decimalValue.test(close)) continue
      if (compareDecimals(high, open) < 0 || compareDecimals(high, close) < 0) problems.push(`${at}: the ${view} high is below its open or close`)
      if (compareDecimals(low, open) > 0 || compareDecimals(low, close) > 0) problems.push(`${at}: the ${view} low is above its open or close`)
    }
    if (step !== undefined && candle.openTimeMs % step !== 0n) problems.push(`${at} opens off the interval's grid`)
    const previous = series.candles[i - 1]
    if (step !== undefined && previous && candle.openTimeMs - previous.openTimeMs !== step) problems.push(`${at} isn't one interval after the candle before it`)
    if (i < series.candles.length - 1 && !candle.closed) problems.push(`${at} is open but isn't the last candle`)
  })
  const causes = series.lineage?.causedBy.map(toHex) ?? []
  const candles = series.candles.map((candle) => toHex(candle.lineage?.id))
  if (causes.join() !== candles.join()) problems.push(`${path}'s lineage isn't caused by exactly its candles`)
}

// Everything checked of one fixture file. A file no schema is mapped for fails, so a new
// directory can't skip the checks.
function fixtureProblems(file: string, json: JsonValue): Problem[] {
  const desc = schemaFor(file)
  if (!desc) return [`${file} has no message mapped to it in schemaFor`]
  let message
  try {
    // fromJson rejects unknown fields and wrong types, numbers for decimal strings among them.
    message = fromJson(desc, json)
  } catch (error) {
    return [`${file} doesn't parse as ${desc.name}: ${(error as Error).message}`]
  }
  const problems: Problem[] = []
  // The round trip rejects non-canonical forms.
  if (JSON.stringify(toJson(desc, message)) !== JSON.stringify(json)) problems.push(`${file} isn't canonical proto3 JSON for ${desc.name}`)
  checkValues(desc, json, file, problems)
  checkLineage(desc, json, file, problems)
  if (desc.typeName === CandleSeriesSchema.typeName) checkSeries(message as CandleSeries, file, problems)
  return problems
}

describe('every API fixture file', () => {
  it.each(Object.keys(fixtureFiles).sort())('%s parses with its message and passes every check', (file) => {
    expect(fixtureProblems(file, fixtureFiles[file])).toEqual([])
  })

  const series = () => structuredClone(fixtureFiles['candles/4h.json']) as { candles: Record<string, JsonValue>[] }

  it('fails a numeric amount in any directory', () => {
    const json = series()
    json.candles[0].highUsd = 0.01167
    expect(fixtureProblems('candles/4h.json', json as unknown as JsonValue)).toEqual([
      expect.stringMatching(/^candles\/4h\.json doesn't parse as CandleSeries: .*high_usd from JSON: expected string/),
    ])
  })

  it('fails a high below the close', () => {
    const json = series()
    json.candles[0].highUsd = '0.01160'
    expect(fixtureProblems('candles/4h.json', json as unknown as JsonValue)).toEqual(['candles/4h.json.candles[0]: the USD high is below its open or close'])
  })

  it('fails a low above the open', () => {
    const json = series()
    json.candles[0].lowQuote = '0.0000047'
    expect(fixtureProblems('candles/4h.json', json as unknown as JsonValue)).toEqual(['candles/4h.json.candles[0]: the quote low is above its open or close'])
  })

  it('fails candles misaligned with the interval', () => {
    const json = series()
    json.candles[1].openTimeMs = String(BigInt(json.candles[1].openTimeMs as string) + 1n)
    expect(fixtureProblems('candles/4h.json', json as unknown as JsonValue)).toEqual([
      "candles/4h.json.candles[1] opens off the interval's grid",
      "candles/4h.json.candles[1] isn't one interval after the candle before it",
      "candles/4h.json.candles[2] isn't one interval after the candle before it",
    ])
  })

  it('fails an open candle before the last', () => {
    const json = series()
    delete json.candles[0].closed
    expect(fixtureProblems('candles/4h.json', json as unknown as JsonValue)).toEqual(["candles/4h.json.candles[0] is open but isn't the last candle"])
  })

  it("fails a series whose lineage doesn't list its candles", () => {
    const json = series() as unknown as { lineage: { causedBy: string[] } }
    json.lineage.causedBy.pop()
    expect(fixtureProblems('candles/4h.json', json as unknown as JsonValue)).toEqual(["candles/4h.json's lineage isn't caused by exactly its candles"])
  })

  it('fails a candle without lineage', () => {
    const json = series()
    delete json.candles[2].lineage
    expect(fixtureProblems('candles/4h.json', json as unknown as JsonValue)).toEqual([
      'candles/4h.json.candles[2] has no 16-byte lineage ID',
      "candles/4h.json's lineage isn't caused by exactly its candles",
    ])
  })

  it('fails a file no message is mapped to, naming it', () => {
    expect(fixtureProblems('trades/recent.json', {})).toEqual(['trades/recent.json has no message mapped to it in schemaFor'])
    expect(fixtureProblems('v1/NoSuchMessage.json', {})).toEqual(['v1/NoSuchMessage.json has no message mapped to it in schemaFor'])
  })
})

describe('API contract v0', () => {
  it('has a fixture for every message, and no fixture without a message', () => {
    const names = fixtureMessages.map((desc) => desc.name).sort()
    expect(Object.keys(apiFixturesJson).sort()).toEqual(names)
  })

  it('gives every record a lineage field', () => {
    const missing = apiMessages
      .filter((desc) => !notRecords.has(desc.name))
      .filter((desc) => desc.field.lineage?.message?.typeName !== LineageSchema.typeName)
      .map((desc) => desc.name)
    expect(missing).toEqual([])
  })

  it('lists only messages that exist as non-records', () => {
    const names = new Set(apiMessages.map((desc) => desc.name))
    expect([...notRecords].filter((name) => !names.has(name))).toEqual([])
  })

  it('sends amounts as decimal strings, never numbers', () => {
    const problems: Problem[] = []
    for (const desc of allMessages(apiMessages)) {
      for (const field of desc.fields) {
        if (field.fieldKind === 'scalar' || field.fieldKind === 'list') {
          if (field.scalar === ScalarType.FLOAT || field.scalar === ScalarType.DOUBLE) {
            problems.push(`${desc.typeName}.${field.name} is a float`)
          }
        }
        if (field.fieldKind !== 'message' && decimalField.test(field.name) && !isStringField(field)) {
          problems.push(`${desc.typeName}.${field.name} isn't a string`)
        }
      }
    }
    expect(problems).toEqual([])
  })
})

describe('candle series fixtures', () => {
  const intervals = Object.keys(candleSeriesFixturesJson).sort()

  it('has one for each interval the chart needs beyond the message fixture', () => {
    expect(intervals).toEqual(['15m', '1d', '4h'])
  })

  it.each(intervals)('the %s series is at its interval', (interval) => {
    expect((candleSeriesFixturesJson[interval] as { interval?: string }).interval).toBe(`CANDLE_INTERVAL_${interval.toUpperCase()}`)
  })

  it.each(intervals)('every candle in the %s series carries its quote view', (interval) => {
    const series = fromJson(CandleSeriesSchema, candleSeriesFixturesJson[interval])
    for (const candle of series.candles) {
      const view = [candle.quoteAsset, candle.quoteSymbol, candle.openQuote, candle.highQuote, candle.lowQuote, candle.closeQuote, candle.volumeQuote]
      expect(view.every((field) => field !== ''), `candle ${candle.openTimeMs}: ${view.join(', ')}`).toBe(true)
      expect(candle.quoteTrades, `candle ${candle.openTimeMs} quote trades`).toBeGreaterThan(0n)
    }
  })
})
