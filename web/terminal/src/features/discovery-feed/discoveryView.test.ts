// The Discover table's order (#65): lists, filters, sorting, and holding still under the pointer.
import { create, type MessageInitShape } from '@bufbuild/protobuf'
import Decimal from 'decimal.js'
import { describe, expect, it } from 'vitest'
import {
  DiscoveryList,
  DiscoveryRowSchema,
  SafetyVerdict,
  type DiscoveryRow,
} from '../../api/generated/omnimarket/api/v1/market_pb'
import { compareRows, holdOrder, insertedRows, priceMoves, selectRows, type DiscoverySort } from './discoveryView'

const NOW = 1_790_000_000_000n

// The plain-object form of a row's init, so a test can override any field.
type RowInit = Exclude<MessageInitShape<typeof DiscoveryRowSchema>, DiscoveryRow>

function row(address: string, init: RowInit = {}): DiscoveryRow {
  return create(DiscoveryRowSchema, {
    token: { chainId: 8453n, address, symbol: address.slice(-3), name: address },
    poolCreatedBlock: 100n,
    poolCreatedAtMs: NOW - 60_000n,
    displayPriceUsd: '1',
    marketCapUsd: '1000',
    depthUsd: '500',
    stats5m: { volumeUsd: '10', buys: 1n, sells: 1n, priceChangePct: '0' },
    lists: [DiscoveryList.NEW, DiscoveryList.TRENDING],
    ...init,
  })
}

function byKey(...rows: DiscoveryRow[]): Record<string, DiscoveryRow> {
  return Object.fromEntries(rows.map((r) => [r.token!.address.toLowerCase(), r]))
}

const feed: DiscoverySort = { key: 'feed', direction: 'asc' }

describe('lists', () => {
  it('New lists pools by creation, newest first', () => {
    const rows = byKey(
      row('0xa', { poolCreatedAtMs: NOW - 300_000n, poolCreatedBlock: 1n }),
      row('0xb', { poolCreatedAtMs: NOW - 10_000n, poolCreatedBlock: 3n }),
      row('0xc', { poolCreatedAtMs: NOW - 60_000n, poolCreatedBlock: 2n }),
    )
    expect(selectRows(rows, 'New', {}, feed, NOW)).toEqual(['0xb', '0xc', '0xa'])
  })

  it('Trending keeps the feed rank, and only rows in the Trending list', () => {
    const rows = byKey(
      row('0xa', { rank: 2 }),
      row('0xb', { rank: 1 }),
      row('0xc', { rank: 3, lists: [DiscoveryList.NEW] }),
      row('0xd', { rank: 0 }),
    )
    expect(selectRows(rows, 'Trending', {}, feed, NOW)).toEqual(['0xb', '0xa', '0xd'])
  })
})

describe('sorting', () => {
  const rows = byKey(
    row('0xa', { depthUsd: '900.5', marketCapUsd: '20', stats5m: { volumeUsd: '5', buys: 10n, sells: 0n } }),
    row('0xb', { depthUsd: '1000', marketCapUsd: '9', stats5m: { volumeUsd: '50', buys: 1n, sells: 1n } }),
    row('0xc', { depthUsd: '', marketCapUsd: '100', stats5m: { volumeUsd: '0.5', buys: 3n, sells: 3n } }),
  )

  it.each([
    ['liquidity', 'desc', ['0xb', '0xa', '0xc']],
    ['liquidity', 'asc', ['0xa', '0xb', '0xc']],
    ['marketCap', 'desc', ['0xc', '0xa', '0xb']],
    ['volume', 'desc', ['0xb', '0xa', '0xc']],
    ['txns', 'desc', ['0xa', '0xc', '0xb']],
    ['txns', 'asc', ['0xb', '0xc', '0xa']],
  ] as const)('by %s %s', (key, direction, expected) => {
    expect(selectRows(rows, 'New', {}, { key, direction }, NOW)).toEqual(expected)
  })

  it('by age, youngest first when ascending', () => {
    const aged = byKey(row('0xa', { poolCreatedAtMs: NOW - 5n }), row('0xb', { poolCreatedAtMs: NOW - 50n }))
    expect(selectRows(aged, 'New', {}, { key: 'age', direction: 'asc' }, NOW)).toEqual(['0xa', '0xb'])
    expect(selectRows(aged, 'New', {}, { key: 'age', direction: 'desc' }, NOW)).toEqual(['0xb', '0xa'])
  })

  it('compares exact decimals, not floats', () => {
    const close = byKey(
      row('0xa', { depthUsd: '0.30000000000000000001' }),
      row('0xb', { depthUsd: '0.3' }),
    )
    expect(selectRows(close, 'New', {}, { key: 'liquidity', direction: 'desc' }, NOW)).toEqual(['0xa', '0xb'])
  })

  it('is stable: ties fall back to the newer pool, then the address, whatever the arrival order', () => {
    const tied = [
      row('0xb', { poolCreatedBlock: 5n }),
      row('0xc', { poolCreatedBlock: 7n }),
      row('0xa', { poolCreatedBlock: 5n }),
    ]
    const sort: DiscoverySort = { key: 'volume', direction: 'desc' }
    const forward = selectRows(byKey(...tied), 'New', {}, sort, NOW)
    const backward = selectRows(byKey(...tied.slice().reverse()), 'New', {}, sort, NOW)
    expect(forward).toEqual(['0xc', '0xa', '0xb'])
    expect(backward).toEqual(forward)
    expect(compareRows(tied[0]!, tied[0]!, 'New', sort)).toBe(0)
  })
})

describe('filters', () => {
  const rows = byKey(
    row('0xa', { depthUsd: '99999.99', poolCreatedAtMs: NOW - 30_000n }),
    row('0xb', { depthUsd: '100000', poolCreatedAtMs: NOW - 3_600_000n, safety: { verdict: SafetyVerdict.PASSED } }),
    row('0xc', { depthUsd: '250000', poolCreatedAtMs: NOW - 120_000n, safety: { verdict: SafetyVerdict.WARNING } }),
    row('0xd', { token: { chainId: 56n, address: '0xd' } }),
  )
  const age = { key: 'age', direction: 'asc' } as const

  it('keeps one chain', () => {
    expect(selectRows(rows, 'New', { chainId: 8453n }, age, NOW)).not.toContain('0xd')
    expect(selectRows(rows, 'New', { chainId: 56n }, age, NOW)).toEqual(['0xd'])
  })

  it('drops rows below the minimum liquidity, exactly at the boundary', () => {
    expect(selectRows(rows, 'New', { chainId: 8453n, minLiquidityUsd: new Decimal('100000') }, age, NOW)).toEqual(['0xc', '0xb'])
  })

  it('drops rows older than the maximum age, and ignores the filter until the clock is known', () => {
    expect(selectRows(rows, 'New', { chainId: 8453n, maxAgeMs: 300_000n }, age, NOW)).toEqual(['0xa', '0xc'])
    expect(selectRows(rows, 'New', { chainId: 8453n, maxAgeMs: 300_000n }, age, undefined)).toHaveLength(3)
  })

  it('keeps only passed safety checks when asked; an unchecked token is not passed', () => {
    expect(selectRows(rows, 'New', { chainId: 8453n, safetyPassedOnly: true }, age, NOW)).toEqual(['0xb'])
  })
})

describe('holding still under the pointer', () => {
  it('keeps what was shown; new rows wait and departed rows stay', () => {
    const held = holdOrder(['0xa', '0xb', '0xc'], ['0xd', '0xc', '0xa'])
    expect(held.order).toEqual(['0xa', '0xb', '0xc'])
    expect(held.waiting).toEqual(['0xd'])
    expect(held.departed).toEqual(['0xb'])
  })

  it('a re-rank alone moves nothing', () => {
    const held = holdOrder(['0xa', '0xb'], ['0xb', '0xa'])
    expect(held).toEqual({ order: ['0xa', '0xb'], waiting: [], departed: [] })
  })
})

describe('row motion', () => {
  it('finds price moves up and down, and ignores unchanged rows', () => {
    const before = byKey(row('0xa', { displayPriceUsd: '1.0' }), row('0xb', { displayPriceUsd: '2' }), row('0xc'))
    const after = byKey(
      row('0xa', { displayPriceUsd: '1.01' }),
      row('0xb', { displayPriceUsd: '1.99' }),
      row('0xc'),
      row('0xd'),
    )
    expect(priceMoves(before, after)).toEqual({ '0xa': 'up', '0xb': 'down' })
  })

  it('treats a reformatted but equal price as no move', () => {
    expect(priceMoves(byKey(row('0xa', { displayPriceUsd: '1.0' })), byKey(row('0xa', { displayPriceUsd: '1' })))).toEqual({})
  })

  it('finds inserted rows', () => {
    expect(insertedRows(byKey(row('0xa')), byKey(row('0xa'), row('0xb')))).toEqual(['0xb'])
  })
})
