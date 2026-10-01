import type { Meta, StoryObj } from '@storybook/react-vite'
import { ConnectionStatus } from './ConnectionStatus'

const meta = {
  title: 'Shell/ConnectionStatus',
  component: ConnectionStatus,
  parameters: { layout: 'centered' },
  args: { source: 'live', state: 'open' },
} satisfies Meta<typeof ConnectionStatus>

export default meta
type Story = StoryObj<typeof meta>

export const Live: Story = {}
export const Replay: Story = { args: { source: 'replay' } }
export const Fixtures: Story = { args: { source: 'fixtures' } }
export const Connecting: Story = { args: { state: 'connecting' } }
export const Stale: Story = { args: { state: 'stale' } }
export const Reconnecting: Story = { args: { state: 'reconnecting' } }
export const Unavailable: Story = { args: { state: 'unavailable' } }
