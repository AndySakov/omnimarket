import type { Meta, StoryObj } from '@storybook/react-vite'
import { DataStatus } from './DataStatus'

const meta = {
  title: 'Shared/DataStatus',
  component: DataStatus,
  parameters: { layout: 'centered' },
  args: { label: 'NOVA price' },
} satisfies Meta<typeof DataStatus>

export default meta
type Story = StoryObj<typeof meta>

export const Loading: Story = { args: { state: 'loading' } }
export const Live: Story = { args: { state: 'live' } }
export const Stale: Story = { args: { state: 'stale' } }
export const Reconnecting: Story = { args: { state: 'reconnecting' } }
export const Unavailable: Story = { args: { state: 'unavailable' } }
