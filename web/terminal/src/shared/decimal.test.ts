import { describe, expect, it } from 'vitest'
import { DecimalParseError, parseDecimal, parseOptionalDecimal } from './decimal'

describe('parseDecimal', () => {
  it('keeps every digit of a contract amount', () => {
    const value = '123456789012345678901234567890.000000000000000000000000000001'
    expect(parseDecimal(value).toFixed()).toBe(value)
    expect(parseDecimal('0.1').plus(parseDecimal('0.2')).toFixed()).toBe('0.3')
    expect(parseDecimal('-3.5').toFixed()).toBe('-3.5')
  })

  it('rejects anything that isn\'t a plain decimal string', () => {
    for (const bad of ['', '1e5', '0x10', ' 1', '1.', '.5', 'NaN', 'Infinity', '+1']) {
      expect(() => parseDecimal(bad), bad).toThrow(DecimalParseError)
    }
  })

  it('reads an unset or malformed field as undefined', () => {
    for (const missing of ['', 'abc', '1e400', 'NaN', 'Infinity', '0x10']) {
      expect(parseOptionalDecimal(missing), missing).toBeUndefined()
    }
    expect(parseOptionalDecimal('2')?.toFixed()).toBe('2')
    expect(parseOptionalDecimal('-1.5')?.toFixed()).toBe('-1.5')
  })
})
