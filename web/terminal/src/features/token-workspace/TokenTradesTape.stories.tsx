import type { Meta, StoryObj } from '@storybook/react-vite'
import { TradesPanel } from './TokenPage'
import { tokenWorkspace } from '../../mocks/tokenWorkspaceFixtures'

const meta = {
  title: 'Token Workspace/Trades Tape',
  component: TradesPanel,
  parameters: { layout: 'fullscreen' },
  args: { rows: tokenWorkspace.trades },
} satisfies Meta<typeof TradesPanel>

export default meta
type Story = StoryObj<typeof meta>

export const Live: Story = {}
export const Loading: Story = { args: { state: 'loading' } }
export const Empty: Story = { args: { rows: [], state: 'empty' } }
export const Stale: Story = { args: { state: 'stale' } }
export const Error: Story = { args: { rows: [], state: 'error' } }
