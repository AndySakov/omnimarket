import Decimal from 'decimal.js'
import { describe, expect, it } from 'vitest'
import { formatDecimal, formatTokenAmount, formatUsd } from './format'

describe('format', () => {
  it('groups thousands and rounds half up at fixed places, exactly', () => {
    expect(formatDecimal(new Decimal('1234567.891'), 2)).toBe('1,234,567.89')
    expect(formatDecimal(new Decimal('0.125'), 2)).toBe('0.13')
    expect(formatDecimal(new Decimal('999.995'), 2)).toBe('1,000.00')
    expect(formatDecimal(new Decimal('12345678901234567890.01'), 2)).toBe('12,345,678,901,234,567,890.01')
  })

  it('writes USD with cents and a real minus sign, and never "−$0.00"', () => {
    expect(formatUsd(new Decimal('3496.40292216'))).toBe('$3,496.40')
    expect(formatUsd(new Decimal('-3.59707784'))).toBe('−$3.60')
    expect(formatUsd(new Decimal('-0.001'))).toBe('$0.00')
    expect(formatUsd(new Decimal('0'))).toBe('$0.00')
  })

  it('trims token amounts to four places and drops trailing zeros', () => {
    expect(formatTokenAmount(new Decimal('0.9'))).toBe('0.9')
    expect(formatTokenAmount(new Decimal('1000'))).toBe('1,000')
    expect(formatTokenAmount(new Decimal('19871.20345'))).toBe('19,871.2035')
    expect(formatTokenAmount(new Decimal('0.00001'))).toBe('0')
    expect(formatTokenAmount(new Decimal('1234567.89'))).toBe('1,234,568')
  })
})
