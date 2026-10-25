// Hostile contract values (#139): the variants of a message a server could still send that a
// region must show without crashing or printing NaN.

import { ScalarType, type DescField, type DescMessage, type JsonObject, type JsonValue } from '@bufbuild/protobuf'
import { decimalField } from './wire'

/** Decimal strings no amount should be shown as: empty, not a number, negative, out of range, too long. */
export const HOSTILE_DECIMALS = ['', 'abc', '-1', '1e400', '1234567890123456789012345678901234567890']

export type Variant = { change: string; json: JsonObject }

function isDecimal(field: DescField): boolean {
  const scalar = field.fieldKind === 'scalar' || (field.fieldKind === 'list' && field.listKind === 'scalar') ? field.scalar : undefined
  return scalar === ScalarType.STRING && decimalField.test(field.name)
}

function asObject(json: JsonValue | undefined): JsonObject | undefined {
  return json !== null && typeof json === 'object' && !Array.isArray(json) ? json : undefined
}

/**
 * Every variant of `json`, a `desc` message in proto3 JSON, with one decimal string made hostile or
 * one sub-message unset. A repeated or map field varies its first entry.
 */
export function hostileVariants(desc: DescMessage, json: JsonObject, path: string = desc.name): Variant[] {
  const out: Variant[] = []
  for (const field of desc.fields) {
    const key = field.jsonName
    const at = `${path}.${key}`
    const value = json[key]
    if (isDecimal(field)) {
      if (field.fieldKind === 'list') {
        if (!Array.isArray(value) || value.length === 0) continue
        for (const hostile of HOSTILE_DECIMALS) out.push({ change: `${at}[0] = "${hostile}"`, json: { ...json, [key]: [hostile, ...value.slice(1)] } })
      } else {
        for (const hostile of HOSTILE_DECIMALS) out.push({ change: `${at} = "${hostile}"`, json: { ...json, [key]: hostile } })
      }
      continue
    }
    if (field.fieldKind === 'message') {
      const child = asObject(value)
      if (!child) continue
      const unset = { ...json }
      delete unset[key]
      out.push({ change: `${at} unset`, json: unset })
      for (const variant of hostileVariants(field.message, child, at)) out.push({ change: variant.change, json: { ...json, [key]: variant.json } })
    } else if (field.fieldKind === 'list' && field.listKind === 'message' && Array.isArray(value)) {
      const first = asObject(value[0])
      if (!first) continue
      for (const variant of hostileVariants(field.message, first, `${at}[0]`)) out.push({ change: variant.change, json: { ...json, [key]: [variant.json, ...value.slice(1)] } })
    } else if (field.fieldKind === 'map' && field.mapKind === 'message') {
      const entries = asObject(value)
      const firstKey = entries && Object.keys(entries)[0]
      const first = firstKey === undefined ? undefined : asObject(entries?.[firstKey])
      if (!entries || firstKey === undefined || !first) continue
      for (const variant of hostileVariants(field.message, first, `${at}["${firstKey}"]`)) out.push({ change: variant.change, json: { ...json, [key]: { ...entries, [firstKey]: variant.json } } })
    }
  }
  return out
}
