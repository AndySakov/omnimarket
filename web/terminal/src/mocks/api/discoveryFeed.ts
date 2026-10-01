// The fixture source's discovery feed (#65): the contract's two canonical rows plus four more, then
// a live stream of changes, so Discover moves offline the way it does on the live API. Pure and
// deterministic: tick n always produces the same deltas.

import { create, type MessageInitShape } from '@bufbuild/protobuf'
import Decimal from 'decimal.js'
import {
  DiscoveryFeedSchema,
  DiscoveryList,
  DiscoveryRowSchema,
  SafetyVerdict,
  type DiscoveryFeed,
  type DiscoveryRow,
} from '../../api/generated/omnimarket/api/v1/market_pb'
import type { DeltaSchema } from '../../api/generated/omnimarket/api/v1/stream_pb'
import { apiFixture } from '../fixtures/api'

/** A new pool every this many ticks, starting at tick `FIRST_NEW_POOL_TICK`. */
export const NEW_POOL_EVERY_TICKS = 6
export const FIRST_NEW_POOL_TICK = 4
/** New pools beyond this many are removed, oldest first, so a long session stays bounded. */
const MAX_NEW_POOLS = 12

const WETH = { chainId: 8453n, address: '0x4200000000000000000000000000000000000006', symbol: 'WETH', name: 'Wrapped Ether', decimals: 18 }

type Seed = { symbol: string; name: string; price: string; supply: string; depth: string; volume: string; buys: bigint; sells: bigint; change: string; ageMs: bigint; verdict: SafetyVerdict; trending: number }

// Four more fixture tokens, enough rows for the table to scroll at every breakpoint.
const SEEDS: Seed[] = [
  { symbol: 'KELP', name: 'Kelp Labs (fixture)', price: '0.4821', supply: '18000000', depth: '612400', volume: '92480.1', buys: 340n, sells: 211n, change: '3.2', ageMs: 3_480_000n, verdict: SafetyVerdict.PASSED, trending: 2 },
  { symbol: 'MOTH', name: 'Moth Protocol (fixture)', price: '0.000184', supply: '1000000000', depth: '48210.3', volume: '18700.55', buys: 96n, sells: 120n, change: '-4.8', ageMs: 1_260_000n, verdict: SafetyVerdict.WARNING, trending: 3 },
  { symbol: 'RIFT', name: 'Rift Finance (fixture)', price: '2.17', supply: '4200000', depth: '1204000', volume: '51230', buys: 88n, sells: 74n, change: '0.9', ageMs: 5_400_000n, verdict: SafetyVerdict.PASSED, trending: 4 },
  { symbol: 'DUSK', name: 'Dusk Cat (fixture)', price: '0.0000921', supply: '420690000000', depth: '9120.4', volume: '2210.8', buys: 31n, sells: 12n, change: '11.6', ageMs: 240_000n, verdict: SafetyVerdict.UNSPECIFIED, trending: 0 },
]

const NEW_POOL_NAMES = ['Brine', 'Cobalt', 'Ember', 'Fable', 'Grove', 'Haze', 'Iris', 'Jolt']

// Relative price moves, one row at a time; they cycle, so prices wander without drifting far.
const STEPS = ['0.012', '-0.008', '0.005', '-0.011', '0.007', '-0.004'].map((s) => new Decimal(s))

type DeltaPayload = NonNullable<MessageInitShape<typeof DeltaSchema>['payload']>

export class FixtureDiscovery {
  private readonly rows = new Map<string, DiscoveryRow>()
  private readonly newPools: string[] = []

  /** `headTimeMs` and `headBlock` are the fixture chain's head when the stream starts. */
  constructor(
    private readonly headTimeMs: bigint,
    private readonly headBlock: bigint,
  ) {
    const canonical = apiFixture(DiscoveryFeedSchema)
    for (const row of canonical.rows) this.rows.set(row.token!.address, row)
    SEEDS.forEach((seed, i) => {
      const row = seededRow(seed, i, headTimeMs, headBlock)
      this.rows.set(row.token!.address, row)
    })
  }

  snapshot(): DiscoveryFeed {
    return create(DiscoveryFeedSchema, { rows: [...this.rows.values()], blockNumber: this.headBlock })
  }

  /** The changes in second `tick` (1-based) of the stream, whose clock then reads `nowMs`. */
  tick(tick: number, nowMs: bigint): DeltaPayload[] {
    const block = this.headBlock + BigInt(tick) / 2n
    const out: DeltaPayload[] = []

    // One row's price moves each tick, in turn.
    const keys = [...this.rows.keys()]
    const key = keys[tick % keys.length]!
    const moved = movePrice(this.rows.get(key)!, STEPS[tick % STEPS.length]!, block)
    this.rows.set(key, moved)
    out.push({ case: 'discoveryRow', value: moved })

    if (tick >= FIRST_NEW_POOL_TICK && (tick - FIRST_NEW_POOL_TICK) % NEW_POOL_EVERY_TICKS === 0) {
      const n = (tick - FIRST_NEW_POOL_TICK) / NEW_POOL_EVERY_TICKS
      const row = newPoolRow(n, nowMs, block)
      this.rows.set(row.token!.address, row)
      this.newPools.push(row.token!.address)
      out.push({ case: 'discoveryRow', value: row })
      if (this.newPools.length > MAX_NEW_POOLS) {
        const oldest = this.newPools.shift()!
        this.rows.delete(oldest)
        out.push({ case: 'discoveryRowRemoved', value: { chainId: 8453n, token: oldest } })
      }
    }
    return out
  }
}

function seededRow(seed: Seed, index: number, headTimeMs: bigint, headBlock: bigint): DiscoveryRow {
  const age = seed.ageMs
  const lists = [DiscoveryList.NEW, ...(seed.trending > 0 ? [DiscoveryList.TRENDING] : [])]
  return create(DiscoveryRowSchema, {
    lineage: { id: lineageId(0x40 + index) },
    token: { chainId: 8453n, address: address(0xa0 + index), symbol: seed.symbol, name: seed.name, decimals: 18 },
    pool: address(0xb0 + index),
    venue: index % 2 === 0 ? 'aerodrome' : 'uniswap-v3',
    quoteToken: WETH,
    poolCreatedBlock: headBlock - age / 2000n,
    poolCreatedAtMs: headTimeMs - age,
    displayPriceUsd: seed.price,
    marketCapUsd: new Decimal(seed.price).times(seed.supply).toFixed(),
    marketCapIsFdv: true,
    depthUsd: seed.depth,
    stats5m: { volumeUsd: seed.volume, buys: seed.buys, sells: seed.sells, priceChangePct: seed.change },
    stats1h: { volumeUsd: seed.volume, buys: seed.buys, sells: seed.sells, priceChangePct: seed.change },
    statsTracked: { volumeUsd: seed.volume, buys: seed.buys, sells: seed.sells, priceChangePct: seed.change },
    thin: false,
    safety: seed.verdict === SafetyVerdict.UNSPECIFIED
      ? {}
      : { verdict: seed.verdict, buyTaxPct: '0', sellTaxPct: seed.verdict === SafetyVerdict.WARNING ? '4.5' : '0', checkedAtMs: headTimeMs - 30_000n },
    rank: seed.trending,
    lists,
    blockNumber: headBlock,
  })
}

function newPoolRow(n: number, nowMs: bigint, block: bigint): DiscoveryRow {
  const name = NEW_POOL_NAMES[n % NEW_POOL_NAMES.length]!
  const round = Math.floor(n / NEW_POOL_NAMES.length)
  const label = round === 0 ? name : `${name} ${round + 1}`
  const price = new Decimal('0.000042').times(n + 1)
  return create(DiscoveryRowSchema, {
    lineage: { id: lineageId(0x80 + (n % 0x7f)) },
    token: { chainId: 8453n, address: address(0x1000 + n), symbol: name.toUpperCase().slice(0, 4), name: `${label} (fixture)`, decimals: 18 },
    pool: address(0x2000 + n),
    venue: 'uniswap-v2',
    quoteToken: WETH,
    poolCreatedBlock: block,
    poolCreatedAtMs: nowMs,
    displayPriceUsd: price.toFixed(),
    marketCapUsd: price.times('1000000000').toFixed(),
    marketCapIsFdv: true,
    depthUsd: '1850.5',
    stats5m: { volumeUsd: '0', buys: 0n, sells: 0n, priceChangePct: '0' },
    stats1h: { volumeUsd: '0', buys: 0n, sells: 0n, priceChangePct: '0' },
    statsTracked: { volumeUsd: '0', buys: 0n, sells: 0n, priceChangePct: '0' },
    thin: true,
    safety: {},
    lists: [DiscoveryList.NEW],
    blockNumber: block,
  })
}

/** A swap: the price moves by `step`, and the 5m window counts it. */
function movePrice(row: DiscoveryRow, step: Decimal, block: bigint): DiscoveryRow {
  const factor = new Decimal(1).plus(step)
  const scale = (value: string) => (value ? new Decimal(value).times(factor).toSignificantDigits(8).toFixed() : value)
  const stats = row.stats5m
  const buy = step.isPositive()
  return {
    ...row,
    displayPriceUsd: scale(row.displayPriceUsd),
    marketCapUsd: scale(row.marketCapUsd),
    stats5m: stats && {
      ...stats,
      volumeUsd: new Decimal(stats.volumeUsd || '0').plus('125.5').toFixed(),
      buys: stats.buys + (buy ? 1n : 0n),
      sells: stats.sells + (buy ? 0n : 1n),
    },
    blockNumber: block,
  }
}

function address(n: number): string {
  return `0x${n.toString(16).padStart(40, 'f')}`
}

function lineageId(n: number): Uint8Array {
  const id = new Uint8Array(16)
  id[0] = 0xd1
  id[15] = n
  return id
}
