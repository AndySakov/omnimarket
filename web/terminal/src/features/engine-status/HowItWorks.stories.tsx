import type { Meta, StoryObj } from '@storybook/react-vite'
import { Dialog } from 'radix-ui'
import { EngineStatusSchema } from '../../api/generated/omnimarket/api/v1/market_pb'
import { apiFixture } from '../../mocks/fixtures/api'
import { EngineStatusDetails } from './EngineStatusBar'
import { HowItWorksContent } from './HowItWorks'

const status = apiFixture(EngineStatusSchema)

/** The view as it opens over the terminal, with the engine's status as the stream gives it. */
function HowItWorksOpen({ source, withStatus }: { source: 'fixtures' | 'replay' | 'live'; withStatus: boolean }) {
  return (
    <Dialog.Root open>
      <Dialog.Content className="how-it-works">
        <HowItWorksContent source={source}>
          {withStatus ? <EngineStatusDetails status={status} state="live" source={source} /> : <p>Waiting for the engine’s first status.</p>}
        </HowItWorksContent>
      </Dialog.Content>
    </Dialog.Root>
  )
}

const meta = {
  title: 'Shell/HowItWorks',
  component: HowItWorksOpen,
  parameters: { layout: 'fullscreen' },
  args: { source: 'live', withStatus: true },
} satisfies Meta<typeof HowItWorksOpen>

export default meta
type Story = StoryObj<typeof meta>

export const Live: Story = {}
export const Fixtures: Story = { args: { source: 'fixtures' } }
export const Loading: Story = { args: { withStatus: false } }
