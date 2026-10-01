import { describe, expect, it } from 'vitest'
import { fromJson, toJson, ScalarType, type DescField, type DescMessage, type JsonValue } from '@bufbuild/protobuf'
import { file_omnimarket_api_v1_automation } from './generated/omnimarket/api/v1/automation_pb'
import { file_omnimarket_api_v1_common } from './generated/omnimarket/api/v1/common_pb'
import { file_omnimarket_api_v1_market } from './generated/omnimarket/api/v1/market_pb'
import { file_omnimarket_api_v1_stream } from './generated/omnimarket/api/v1/stream_pb'
import { file_omnimarket_api_v1_trading } from './generated/omnimarket/api/v1/trading_pb'
import { LineageSchema } from './generated/omnimarket/lineage/v1/lineage_pb'
import { apiFixturesJson } from '../mocks/fixtures/api'

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

// Fields holding amounts, prices, USD values or percentages: decimal strings on the wire.
const decimalField = /(^|_)(usd|pct|amount|price|supply|native|bought|returned|spent|quote)$|^amount_|_amount_|^pct_/
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

describe('API contract v0', () => {
  it('has a fixture for every message, and no fixture without a message', () => {
    const names = fixtureMessages.map((desc) => desc.name).sort()
    expect(Object.keys(apiFixturesJson).sort()).toEqual(names)
  })

  it.each(fixtureMessages.map((desc) => [desc.name, desc] as const))(
    'the %s fixture is canonical proto3 JSON for its message',
    (_name, desc) => {
      const json = apiFixturesJson[desc.name]
      // fromJson rejects unknown fields and wrong types; the round trip rejects non-canonical forms.
      expect(toJson(desc, fromJson(desc, json))).toEqual(json)
    },
  )

  it('carries lineage on every record', () => {
    const missing = apiMessages
      .filter((desc) => !notRecords.has(desc.name))
      .filter((desc) => desc.field.lineage?.message?.typeName !== LineageSchema.typeName)
      .map((desc) => desc.name)
    expect(missing).toEqual([])

    for (const desc of apiMessages.filter((d) => !notRecords.has(d.name))) {
      const fixture = fromJson(desc, apiFixturesJson[desc.name]) as unknown as { lineage?: { id: Uint8Array } }
      expect(fixture.lineage?.id.length, `${desc.name} fixture's lineage ID`).toBe(16)
    }
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

  it('writes decimals and hex in the agreed forms in every fixture', () => {
    const problems: Problem[] = []
    for (const desc of fixtureMessages) checkValues(desc, apiFixturesJson[desc.name], desc.name, problems)
    expect(problems).toEqual([])
  })
})
