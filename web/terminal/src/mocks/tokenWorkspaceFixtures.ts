import { discoveryTokens } from './discoveryFixtures'
import type { TokenWorkspaceFixture } from '../domains/market/tokenWorkspace'

const pricePoints = [
  ['09:00', 0.42], ['09:15', 0.45], ['09:30', 0.44], ['09:45', 0.51],
  ['10:00', 0.49], ['10:15', 0.58], ['10:30', 0.62], ['10:45', 0.59],
  ['11:00', 0.68], ['11:15', 0.72], ['11:30', 0.69], ['11:45', 0.78],
  ['12:00', 0.84], ['12:15', 0.81], ['12:30', 0.91], ['12:45', 0.96],
] as const

const candles = [
  ['09:00', 0.42, 0.45, 0.40, 0.44], ['09:15', 0.44, 0.47, 0.42, 0.46],
  ['09:30', 0.46, 0.48, 0.43, 0.44], ['09:45', 0.44, 0.53, 0.43, 0.51],
  ['10:00', 0.51, 0.54, 0.47, 0.49], ['10:15', 0.49, 0.61, 0.48, 0.58],
  ['10:30', 0.58, 0.65, 0.56, 0.62], ['10:45', 0.62, 0.64, 0.57, 0.59],
  ['11:00', 0.59, 0.71, 0.57, 0.68], ['11:15', 0.68, 0.75, 0.65, 0.72],
  ['11:30', 0.72, 0.74, 0.66, 0.69], ['11:45', 0.69, 0.81, 0.68, 0.78],
  ['12:00', 0.78, 0.87, 0.76, 0.84], ['12:15', 0.84, 0.86, 0.78, 0.81],
  ['12:30', 0.81, 0.94, 0.79, 0.91], ['12:45', 0.91, 0.99, 0.88, 0.96],
] as const

const chart = {
  candles: candles.map(([time, open, high, low, close]) => ({ time, open, high, low, close })),
  volume: candles.map(([time, open, , , close], index) => ({
    time,
    value: 160 + index * 23,
    tone: close >= open ? 'up' as const : 'down' as const,
  })),
}

export const tokenWorkspace: TokenWorkspaceFixture = {
  ...discoveryTokens[0],
  address: '0x7b3f…8a42',
  price: '$0.96',
  priceChange: '+28.6%',
  fdv: '$14.8M',
  volume24h: '$8.2M',
  pair: 'NOVA / ETH',
  pricePoints: pricePoints.map(([time, value]) => ({ time, value })),
  chart,
  activity: [
    { id: 'trade-1', side: 'Buy', amount: '18,420 NOVA', value: '$17.7K', wallet: '0x3a7e…9c21', time: '12s ago' },
    { id: 'trade-2', side: 'Sell', amount: '4,120 NOVA', value: '$3.9K', wallet: '0xf11d…048a', time: '38s ago' },
    { id: 'trade-3', side: 'Buy', amount: '7,800 NOVA', value: '$7.5K', wallet: '0x8b29…e812', time: '1m ago' },
    { id: 'trade-4', side: 'Buy', amount: '2,410 NOVA', value: '$2.3K', wallet: '0x54ac…14d0', time: '2m ago' },
  ],
  trades: [
    { id: 'trade-1', side: 'Buy', amount: '18.4K', value: '$17.7K', wallet: '0x3a7e…9c21', time: '12s ago', marketCap: '$14.8M', gas: '$0.42', trader: '0x3a7e…9c21', tracking: 'Tracked' },
    { id: 'trade-2', side: 'Sell', amount: '4.1K', value: '$3.9K', wallet: '0xf11d…048a', time: '38s ago', marketCap: '$14.6M', gas: '$0.36', trader: '0xf11d…048a', tracking: 'Untracked' },
    { id: 'trade-3', side: 'Buy', amount: '7.8K', value: '$7.5K', wallet: '0x8b29…e812', time: '1m ago', marketCap: '$14.4M', gas: '$0.31', trader: '0x8b29…e812', tracking: 'Tracked' },
    { id: 'trade-4', side: 'Buy', amount: '2.4K', value: '$2.3K', wallet: '0x54ac…14d0', time: '2m ago', marketCap: '$14.1M', gas: '$0.28', trader: '0x54ac…14d0', tracking: 'Tracked' },
  ],
  positions: [
    { id: 'position-1', wallet: '0x3a7e…9c21', side: 'Long', size: '18.4K NOVA', entry: '$0.77', pnl: '+$3.5K' },
    { id: 'position-2', wallet: '0x8b29…e812', side: 'Long', size: '7.8K NOVA', entry: '$0.82', pnl: '+$1.1K' },
  ],
  orders: [
    { id: 'order-1', type: 'Limit', side: 'Buy', amount: '5.0K NOVA', trigger: '$0.88', status: 'Open' },
    { id: 'order-2', type: 'Limit', side: 'Sell', amount: '2.0K NOVA', trigger: '$1.02', status: 'Open' },
  ],
  holderRows: [
    { id: 'holder-1', wallet: '0x3a7e…9c21', share: '12.4%', balance: '1.84M NOVA', label: 'Top holder' },
    { id: 'holder-2', wallet: '0xf11d…048a', share: '5.8%', balance: '861K NOVA', label: 'Whale' },
    { id: 'holder-3', wallet: '0x8b29…e812', share: '4.2%', balance: '624K NOVA', label: 'Tracked' },
  ],
  topTraders: [
    { id: 'trader-1', wallet: '0x3a7e…9c21', volume: '$84.2K', realizedPnl: '+$16.8K', winRate: '78%' },
    { id: 'trader-2', wallet: '0x8b29…e812', volume: '$51.4K', realizedPnl: '+$8.2K', winRate: '71%' },
    { id: 'trader-3', wallet: '0xf11d…048a', volume: '$39.7K', realizedPnl: '-$2.4K', winRate: '46%' },
  ],
  developerToken: {
    wallet: '0x7c10…11b8',
    balance: '412K NOVA',
    share: '2.76%',
    lastAction: 'Added liquidity 4h ago',
  },
  pool: 'NOVA / ETH · Aerodrome',
  poolShare: '98.2% locked',
  topHolders: '8.4K holders',
  safetyNote: 'Sell simulation passed 2 minutes ago. No mint authority detected; ownership is renounced.',
}
