import type { Meta, StoryObj } from '@storybook/react-vite'
import { withStream } from '../test/streamStory'
import { GlobalHeader } from './GlobalHeader'

const meta = {
  title: 'Shell/GlobalHeader',
  component: GlobalHeader,
  parameters: { layout: 'fullscreen' },
  decorators: [withStream('live', 'open')],
  args: {
    activeRoute: 'Discover',
    onNavigate: () => undefined,
  },
} satisfies Meta<typeof GlobalHeader>

export default meta
type Story = StoryObj<typeof meta>

export const Discover: Story = {}

export const Portfolio: Story = {
  args: { activeRoute: 'Portfolio' },
}

export const Reconnecting: Story = {
  decorators: [withStream('live', 'reconnecting')],
}

export const Fixtures: Story = {
  decorators: [withStream('fixtures', 'open')],
}
