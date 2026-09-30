import type { Meta, StoryObj } from '@storybook/react-vite'
import { GlobalHeader } from './GlobalHeader'

const meta = {
  title: 'Shell/GlobalHeader',
  component: GlobalHeader,
  parameters: { layout: 'fullscreen' },
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
