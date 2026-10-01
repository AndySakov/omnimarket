// A live discovery row as the Discover table shows it (#65): the contract's exact values, turned
// into text here, at the edge (D91). The table's row component takes the same `DiscoveryToken`
// shape the fixture rows use, with the live additions under `live`.

import type Decimal from 'decimal.js'
import { SafetyVerdict as ProtoVerdict, type DiscoveryRow } from '../../api/generated/omnimarket/api/v1/market_pb'
import type { Chain, DiscoveryToken, SafetyVerdict } from '../../domains/market/token'
import { parseOptionalDecimal } from '../../shared/decimal'
import {
  formatAge,
  formatCount,
  formatPercentChange,
  formatUsdCompact,
  formatUsdPrice,
} from '../../shared/format'
import { rowKey, txns, type PriceMove } from './discoveryView'

/** Chain IDs the terminal knows. v0 serves Base only (D91); the table filters to one chain first. */
const chainNames: Record<string, Chain> = {
  '8453': 'Base',
  '56': 'BNB',
}

const avatarTones: DiscoveryToken['avatarTone'][] = ['lime', 'violet', 'orange', 'blue', 'pink', 'teal']
const avatarColors: Record<DiscoveryToken['avatarTone'], string> = {
  lime: '#9fe85d',
  violet: '#a78bfa',
  orange: '#fb923c',
  blue: '#6d7cff',
  pink: '#f472b6',
  teal: '#2dd4bf',
}

const venueLabels: Record<string, string> = {
  'uniswap-v2': 'Uniswap v2',
  'uniswap-v3': 'Uniswap v3',
  'uniswap-v4': 'Uniswap v4',
  aerodrome: 'Aerodrome',
}

export type RowMotion = {
  priceMove?: { direction: PriceMove; seq: number }
  inserted: boolean
  departed: boolean
}

/**
 * The table row for a live discovery row. `nowMs` is the server's clock; until the first heartbeat
 * it's unknown and ages read "–". `prices` are the display prices seen this session, oldest first,
 * for the sparkline.
 */
export function toDiscoveryToken(
  row: DiscoveryRow,
  nowMs: bigint | undefined,
  motion: RowMotion,
  prices: readonly string[] = [],
): DiscoveryToken {
  const address = rowKey(row)
  const symbol = row.token?.symbol || '?'
  const tone = avatarTones[toneIndex(address)]
  const stats = row.stats5m
  const buys = stats?.buys ?? 0n
  const sells = stats?.sells ?? 0n
  const priceChange = parseOptionalDecimal(stats?.priceChangePct ?? '')
  const verdict = verdictOf(row.safety?.verdict)
  const checked = verdict !== 'unchecked'

  return {
    id: address,
    name: row.token?.name || symbol,
    symbol,
    chain: chainNames[(row.token?.chainId ?? 0n).toString()] ?? 'Base',
    age: nowMs === undefined ? '–' : formatAge(nowMs - row.poolCreatedAtMs),
    marketCap: usd(row.marketCapUsd),
    marketCapChange: priceChange ? formatPercentChange(priceChange) : '',
    liquidity: usd(row.depthUsd),
    liquidityChange: '',
    volume: usd(stats?.volumeUsd ?? ''),
    volumeChange: '',
    txns: formatCount(txns(row)),
    // Counts, not amounts: a number is exact well past any real 5m count.
    buys: Number(buys),
    sells: Number(sells),
    holders: '',
    tags: [venueLabels[row.venue] ?? row.venue, ...(row.marketCapIsFdv ? ['FDV'] : [])].filter(Boolean),
    sparkline: sparklineOf(prices),
    safety: {
      liquidity: row.thin ? 'Thin' : 'Depth ±2%',
      taxes: checked ? `${tax(row.safety?.buyTaxPct)}/${tax(row.safety?.sellTaxPct)} taxes` : '',
      ownership: '',
      state: verdict === 'passed' ? 'passed' : verdict === 'honeypot' ? 'blocked' : 'review',
      verdict,
    },
    avatarTone: tone,
    avatarSrc: initialsAvatar(symbol, avatarColors[tone]),
    live: {
      address,
      priceUsd: usdPrice(row.displayPriceUsd),
      priceMove: motion.priceMove,
      inserted: motion.inserted,
      departed: motion.departed,
      verdict,
      buyTax: checked ? tax(row.safety?.buyTaxPct) : undefined,
      sellTax: checked ? tax(row.safety?.sellTaxPct) : undefined,
      thin: row.thin,
    },
  }
}

function verdictOf(verdict: ProtoVerdict | undefined): SafetyVerdict {
  switch (verdict) {
    case ProtoVerdict.PASSED:
      return 'passed'
    case ProtoVerdict.WARNING:
      return 'warning'
    case ProtoVerdict.HONEYPOT:
      return 'honeypot'
    default:
      return 'unchecked'
  }
}

function usd(value: string): string {
  const parsed = parseOptionalDecimal(value)
  return parsed ? formatUsdCompact(parsed) : '–'
}

function usdPrice(value: string): string {
  const parsed = parseOptionalDecimal(value)
  return parsed ? formatUsdPrice(parsed) : '–'
}

function tax(value: string | undefined): string {
  const parsed = parseOptionalDecimal(value ?? '')
  return parsed ? `${parsed.toDecimalPlaces(2).toFixed()}%` : '0%'
}

/**
 * Sparkline points from the prices seen this session, as each price relative to the first. These
 * numbers only place points on a 100×32 drawing; the prices themselves stay exact.
 */
function sparklineOf(prices: readonly string[]): number[] {
  const parsed = prices.map((price) => parseOptionalDecimal(price)).filter((p): p is Decimal => p !== undefined)
  const first = parsed[0]
  if (!first || first.isZero()) return []
  return parsed.map((price) => price.div(first).toNumber())
}

function toneIndex(address: string): number {
  // The last hex digit picks a stable colour per token.
  const digit = parseInt(address.slice(-1), 16)
  return Number.isNaN(digit) ? 0 : digit % avatarTones.length
}

/** A local SVG with the symbol's first letters, until the API carries token images. */
function initialsAvatar(symbol: string, color: string): string {
  const letters = symbol.replace(/[^A-Za-z0-9]/g, '').slice(0, 2).toUpperCase() || '?'
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 72 72">` +
    `<rect width="72" height="72" fill="#111721"/>` +
    `<circle cx="36" cy="36" r="27" fill="${color}" fill-opacity="0.18" stroke="${color}" stroke-width="2"/>` +
    `<text x="36" y="44" text-anchor="middle" font-family="Inter, sans-serif" font-size="22" font-weight="700" fill="${color}">${letters}</text>` +
    `</svg>`
  return `data:image/svg+xml,${encodeURIComponent(svg)}`
}
