// One proto3 JSON fixture per API contract message (#75), for MSW handlers, stories and tests.
// Each file is named after its message, e.g. `v1/TokenSnapshot.json`, and parses with the
// generated schema; `src/api/contract.test.ts` checks that every message has one.
import { fromJson, type DescMessage, type JsonValue, type MessageShape } from '@bufbuild/protobuf'
import { CandleSeriesSchema } from '../../../api/generated/omnimarket/api/v1/market_pb'

const files = import.meta.glob<JsonValue>('./v1/*.json', { eager: true, import: 'default' })

/** The raw proto3 JSON fixtures, keyed by message name. */
export const apiFixturesJson: Readonly<Record<string, JsonValue>> = Object.fromEntries(
  Object.entries(files).map(([path, json]) => [path.slice('./v1/'.length, -'.json'.length), json]),
)

/** The fixture for a message, as the wire carries it: what an MSW handler responds with. */
export function apiFixtureJson(schema: DescMessage): JsonValue {
  const json = apiFixturesJson[schema.name]
  if (json === undefined) {
    throw new Error(`no API fixture for ${schema.typeName}: add src/mocks/fixtures/api/v1/${schema.name}.json`)
  }
  return json
}

const candleFiles = import.meta.glob<JsonValue>('./candles/*.json', { eager: true, import: 'default' })

/**
 * The candle series fixture for an interval, as the wire carries it: `candles/<interval>.json`
 * where there is one, otherwise the `CandleSeries` message fixture.
 */
export function candleSeriesFixtureJson(interval: string): JsonValue {
  return candleFiles[`./candles/${interval}.json`] ?? apiFixtureJson(CandleSeriesSchema)
}

/** The fixture for a message, parsed into its generated type. */
export function apiFixture<Desc extends DescMessage>(schema: Desc): MessageShape<Desc> {
  return fromJson(schema, apiFixtureJson(schema))
}
