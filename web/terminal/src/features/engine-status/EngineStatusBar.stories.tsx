import type { Meta, StoryObj } from '@storybook/react-vite'
import { EngineStatus_Mode, EngineStatusSchema } from '../../api/generated/omnimarket/api/v1/market_pb'
import { apiFixture } from '../../mocks/fixtures/api'
import { EngineStatusBarView } from './EngineStatusBar'

const status = apiFixture(EngineStatusSchema)

const meta = {
  title: 'Shell/EngineStatusBar',
  component: EngineStatusBarView,
  parameters: { layout: 'centered' },
  args: { status, state: 'live', source: 'live' },
} satisfies Meta<typeof EngineStatusBarView>

export default meta
type Story = StoryObj<typeof meta>

export const Live: Story = {}
export const Replay: Story = { args: { status: { ...status, mode: EngineStatus_Mode.REPLAY }, source: 'replay' } }
export const Fixtures: Story = { args: { source: 'fixtures' } }
export const Loading: Story = { args: { status: undefined, state: 'loading' } }
export const Stale: Story = { args: { state: 'stale' } }
export const Reconnecting: Story = { args: { state: 'reconnecting' } }
export const Unavailable: Story = { args: { status: undefined, state: 'unavailable' } }
