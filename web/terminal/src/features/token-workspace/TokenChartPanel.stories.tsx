import type { Meta, StoryObj } from '@storybook/react-vite'
import { TokenChartPanel } from './TokenPage'
import { tokenWorkspace } from '../../mocks/tokenWorkspaceFixtures'

const meta = {
  title: 'Token Workspace/Chart',
  component: TokenChartPanel,
  parameters: { layout: 'fullscreen' },
  args: {
    token: tokenWorkspace,
    chartData: tokenWorkspace.chart,
    updatedLabel: 'Updated 12s ago',
    activeInterval: '5m',
    displayedInterval: '5m',
    state: 'ready',
    connection: { state: 'connected', attempt: 0, label: 'Fixture stream connected' },
    onIntervalChange: () => undefined,
  },
} satisfies Meta<typeof TokenChartPanel>

export default meta
type Story = StoryObj<typeof meta>

export const Live: Story = {}
export const Loading: Story = { args: { chartData: { candles: [], volume: [] }, state: 'loading' } }
export const Empty: Story = { args: { chartData: { candles: [], volume: [] }, state: 'empty' } }
export const Stale: Story = { args: { state: 'stale', connection: { state: 'stale', attempt: 0, label: 'Fixture stream stale' } } }
// The workspace shows candles kept from before a drop as stale while it reconnects.
export const Reconnecting: Story = { args: { state: 'stale', connection: { state: 'reconnecting', attempt: 1, label: 'Fixture stream reconnecting' } } }
export const Unavailable: Story = { args: { chartData: { candles: [], volume: [] }, state: 'error', connection: { state: 'error', attempt: 1, label: 'Fixture stream unavailable' } } }
export const Error: Story = { args: { state: 'error' } }
