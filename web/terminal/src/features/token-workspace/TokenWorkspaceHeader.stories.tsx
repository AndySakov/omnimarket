import type { Meta, StoryObj } from '@storybook/react-vite'
import { TokenWorkspaceHeader } from './TokenPage'
import { tokenWorkspace } from '../../mocks/tokenWorkspaceFixtures'

const meta = {
  title: 'Token Workspace/Header',
  component: TokenWorkspaceHeader,
  parameters: { layout: 'fullscreen' },
  args: {
    token: tokenWorkspace,
    onBack: () => undefined,
    updatedLabel: 'Updated just now',
    connection: { state: 'connected', attempt: 0, label: 'Fixture stream connected' },
  },
} satisfies Meta<typeof TokenWorkspaceHeader>

export default meta
type Story = StoryObj<typeof meta>

export const Live: Story = {}

export const Reconnecting: Story = {
  args: { connection: { state: 'reconnecting', attempt: 1, label: 'Stream interrupted · reconnecting' } },
}

export const Unavailable: Story = {
  args: { connection: { state: 'error', attempt: 2, label: 'Stream unavailable' } },
}
