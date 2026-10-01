// Exact amounts (#63, D91). The API sends amounts, prices and percentages as decimal strings; they
// become decimal.js values here and are only turned into text at the edge, by a formatter.
// Never `Number(...)` an amount.

import Decimal from 'decimal.js'

// The contract's decimal strings: base 10, optional sign, no exponent.
const DECIMAL_STRING = /^-?\d+(\.\d+)?$/

export class DecimalParseError extends Error {}

/** Parses a contract decimal string exactly; rejects anything else, including exponents. */
export function parseDecimal(value: string): Decimal {
  if (!DECIMAL_STRING.test(value)) {
    throw new DecimalParseError(`not a decimal string: "${value}"`)
  }
  return new Decimal(value)
}

/** Like `parseDecimal`, but an empty string (an unset proto3 field) is `undefined`. */
export function parseOptionalDecimal(value: string): Decimal | undefined {
  return value === '' ? undefined : parseDecimal(value)
}
