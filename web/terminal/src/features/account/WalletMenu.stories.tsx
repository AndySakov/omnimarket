import type { Meta, StoryObj } from '@storybook/react-vite'
import { create } from '@bufbuild/protobuf'
import { AccountSchema, Account_Kind } from '../../api/generated/omnimarket/api/v1/trading_pb'
import { apiFixture } from '../../mocks/fixtures/api'
import { AccountMenuView, GuestEntryView } from './WalletMenu'

// The account panel opens in a portal; each story renders it open so every state is visible.
const guest = apiFixture(AccountSchema)
const privy = create(AccountSchema, {
  ...guest,
  kind: Account_Kind.PRIVY,
  walletAddress: '0x3a7e000000000000000000000000000000009c21',
})

const meta = {
  title: 'Account/WalletMenu',
  component: AccountMenuView,
  parameters: { layout: 'padded' },
  decorators: [(Story) => <div style={{ display: 'flex', justifyContent: 'flex-end', minHeight: 420 }}><Story /></div>],
  args: { account: guest, dataState: 'live', onSignOut: () => undefined, defaultOpen: true },
} satisfies Meta<typeof AccountMenuView>

export default meta
type Story = StoryObj<typeof meta>

export const SignedOut: Story = { render: () => <GuestEntryView status="idle" onStart={() => undefined} /> }
export const StartingGuest: Story = { render: () => <GuestEntryView status="starting" onStart={() => undefined} /> }
export const GuestFailed: Story = { render: () => <GuestEntryView status="error" onStart={() => undefined} /> }

export const Loading: Story = { args: { account: undefined, dataState: 'loading' } }
export const GuestLive: Story = {}
export const PrivyWallet: Story = { args: { account: privy } }
export const Empty: Story = { args: { account: create(AccountSchema, { ...guest, balances: [] }) } }
export const Stale: Story = { args: { dataState: 'stale' } }
export const Reconnecting: Story = { args: { dataState: 'reconnecting' } }
export const Unavailable: Story = { args: { dataState: 'unavailable' } }
