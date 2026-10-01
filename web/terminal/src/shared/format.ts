// Formatting at the edge (#65, D91): exact values from `decimal.ts` become text here and nowhere
// earlier. Every formatter takes a Decimal or a bigint, never a JavaScript number.

import Decimal from 'decimal.js'

const COMPACT_STEPS: Array<{ at: Decimal; suffix: string }> = [
  { at: new Decimal('1e12'), suffix: 'T' },
  { at: new Decimal('1e9'), suffix: 'B' },
  { at: new Decimal('1e6'), suffix: 'M' },
  { at: new Decimal('1e3'), suffix: 'K' },
]

/** A USD amount for a dense table: "$12.4M", "$184.3K", "$412.08". */
export function formatUsdCompact(value: Decimal): string {
  const sign = value.isNegative() ? '-' : ''
  const abs = value.abs()
  for (const { at, suffix } of COMPACT_STEPS) {
    if (abs.gte(at)) return `${sign}$${trimZeros(abs.div(at).toDecimalPlaces(1, Decimal.ROUND_DOWN).toFixed(1))}${suffix}`
  }
  return `${sign}$${trimZeros(abs.toDecimalPlaces(2, Decimal.ROUND_DOWN).toFixed(2))}`
}

/** A token price in USD, to four significant digits: "$0.0124", "$0.00000312", "$1,234". */
export function formatUsdPrice(value: Decimal): string {
  const rounded = value.toSignificantDigits(4, Decimal.ROUND_HALF_UP)
  const [whole, fraction] = rounded.abs().toFixed().split('.')
  const grouped = BigInt(whole).toLocaleString('en-US')
  return `${rounded.isNegative() ? '-' : ''}$${fraction ? `${grouped}.${fraction}` : grouped}`
}

/** A percentage change with its sign: "+6.4%", "-12.5%", "0%". */
export function formatPercentChange(value: Decimal): string {
  const rounded = value.toDecimalPlaces(1, Decimal.ROUND_HALF_UP)
  const text = trimZeros(rounded.abs().toFixed(1))
  if (rounded.isZero()) return '0%'
  return `${rounded.isNegative() ? '-' : '+'}${text}%`
}

/** A count: "1,842". */
export function formatCount(value: bigint): string {
  return value.toLocaleString('en-US')
}

/** How long ago, in the largest whole unit: "42s", "7m", "3h", "2d". A future time reads "0s". */
export function formatAge(ageMs: bigint): string {
  const seconds = ageMs > 0n ? ageMs / 1000n : 0n
  if (seconds < 60n) return `${seconds}s`
  if (seconds < 3600n) return `${seconds / 60n}m`
  if (seconds < 86_400n) return `${seconds / 3600n}h`
  return `${seconds / 86_400n}d`
}

function trimZeros(text: string): string {
  return text.includes('.') ? text.replace(/\.?0+$/, '') : text
}
