// Formatting exact values for display (#66). Values stay decimal.js until here, and are turned into
// text without passing through `number`, so a display never rounds through a float.

import Decimal from 'decimal.js'

/** `1234567.891` → `1,234,567.89`: fixed places, rounded half up, thousands grouped. */
export function formatDecimal(value: Decimal, places: number): string {
  const fixed = value.toFixed(places, Decimal.ROUND_HALF_UP)
  const negative = fixed.startsWith('-')
  const [whole, fraction] = (negative ? fixed.slice(1) : fixed).split('.')
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',')
  // "-0.00" reads as a loss that isn't there.
  const sign = negative && !/^[0.,]+$/.test(`${grouped}${fraction ?? ''}`) ? '−' : ''
  return `${sign}${grouped}${fraction === undefined ? '' : `.${fraction}`}`
}

/** A USD value with cents: `$3,496.40`. */
export function formatUsd(value: Decimal): string {
  const text = formatDecimal(value, 2)
  return text.startsWith('−') ? `−$${text.slice(1)}` : `$${text}`
}

/**
 * A token amount: up to four decimal places below 1,000 (`0.9`, `19,871.2034`), none above
 * 1,000,000, trailing zeros trimmed.
 */
export function formatTokenAmount(value: Decimal): string {
  const places = value.abs().gte(1_000_000) ? 0 : 4
  const text = formatDecimal(value, places)
  return places === 0 ? text : text.replace(/\.?0+$/, '')
}
