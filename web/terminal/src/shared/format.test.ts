import Decimal from 'decimal.js'
import { describe, expect, it } from 'vitest'
import { formatAge, formatCount, formatPercentChange, formatUsdCompact, formatUsdPrice } from './format'

const d = (value: string) => new Decimal(value)

describe('formatUsdCompact', () => {
  it.each([
    ['12400000', '$12.4M'],
    ['184300.5', '$184.3K'],
    ['1000', '$1K'],
    ['999999', '$999.9K'],
    ['412.08', '$412.08'],
    ['96.2', '$96.2'],
    ['0', '$0'],
    ['2500000000', '$2.5B'],
    ['-1500', '-$1.5K'],
  ])('%s is %s', (value, text) => {
    expect(formatUsdCompact(d(value))).toBe(text)
  })
})

describe('formatUsdPrice', () => {
  it.each([
    ['0.0124', '$0.0124'],
    ['0.00000312', '$0.00000312'],
    ['0.000003124567', '$0.000003125'],
    ['1234.5678', '$1,235'],
    ['1.5', '$1.5'],
  ])('%s is %s', (value, text) => {
    expect(formatUsdPrice(d(value))).toBe(text)
  })
})

describe('formatPercentChange', () => {
  it.each([
    ['6.4', '+6.4%'],
    ['-12.5', '-12.5%'],
    ['412.5', '+412.5%'],
    ['0', '0%'],
    ['0.04', '0%'],
    ['3', '+3%'],
  ])('%s is %s', (value, text) => {
    expect(formatPercentChange(d(value))).toBe(text)
  })
})

describe('formatCount and formatAge', () => {
  it('groups counts', () => {
    expect(formatCount(1842n)).toBe('1,842')
  })

  it.each([
    [0n, '0s'],
    [-5000n, '0s'],
    [42_999n, '42s'],
    [60_000n, '1m'],
    [420_000n, '7m'],
    [3_600_000n, '1h'],
    [86_400_000n * 2n, '2d'],
  ])('%s ms is %s', (ms, text) => {
    expect(formatAge(ms)).toBe(text)
  })
})
