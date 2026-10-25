import type { Meta, StoryObj } from '@storybook/react-vite'
import { Dialog } from 'radix-ui'
import { EngineStatusSchema } from '../../api/generated/omnimarket/api/v1/market_pb'
import type { DataState } from '../../api/stream/dataState'
import { apiFixture } from '../../mocks/fixtures/api'
import { EngineStatusDetails } from './EngineStatusBar'
import { HowItWorksContent } from './HowItWorks'

const status = apiFixture(EngineStatusSchema)

/** The view as it opens over the terminal, with the engine's status as the stream gives it. */
function HowItWorksOpen({ source, withStatus, state }: { source: 'fixtures' | 'replay' | 'live'; withStatus: boolean; state: DataState }) {
  return (
    <Dialog.Root open>
      <Dialog.Content className="how-it-works">
        <HowItWorksContent source={source}>
          {withStatus ? (
            <EngineStatusDetails status={status} state={state} source={source} />
          ) : (
            <p>{state === 'loading' ? 'Waiting for the engine’s first status.' : 'The engine’s status is unavailable right now.'}</p>
          )}
        </HowItWorksContent>
      </Dialog.Content>
    </Dialog.Root>
  )
}

const meta = {
  title: 'Shell/HowItWorks',
  component: HowItWorksOpen,
  parameters: { layout: 'fullscreen' },
  args: { source: 'live', withStatus: true, state: 'live' },
} satisfies Meta<typeof HowItWorksOpen>

export default meta
type Story = StoryObj<typeof meta>

export const Live: Story = {}
export const Fixtures: Story = { args: { source: 'fixtures' } }
export const Loading: Story = { args: { withStatus: false, state: 'loading' } }
export const Stale: Story = { args: { state: 'stale' } }
export const Reconnecting: Story = { args: { state: 'reconnecting' } }
export const Unavailable: Story = { args: { withStatus: false, state: 'unavailable' } }
