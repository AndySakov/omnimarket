// The guest path end to end in the app (#66): REST from the fixture handlers, the stream from a
// fake socket the test plays.
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { setupServer } from 'msw/node'
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { create } from '@bufbuild/protobuf'
import App from '../../App'
import { AccountSchema, Account_Kind } from '../../api/generated/omnimarket/api/v1/trading_pb'
import { AccountSnapshotSchema, type ClientMessage } from '../../api/generated/omnimarket/api/v1/stream_pb'
import { SessionStore, type SessionStorageLike } from '../../api/session'
import { readDataSource } from '../../api/source'
import { createApiServices } from '../../app/services'
import { fixtureHandlers } from '../../mocks/api/handlers'
import { apiFixture } from '../../mocks/fixtures/api'
import { fakeSockets } from '../../test/fakeSocket'
import { AccountMenuView } from './WalletMenu'

const source = readDataSource({})
const server = setupServer(...fixtureHandlers(source))

beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(() => {
  cleanup()
  server.resetHandlers()
})
afterAll(() => server.close())

function memoryStorage(): SessionStorageLike {
  const items = new Map<string, string>()
  return {
    getItem: (key) => items.get(key) ?? null,
    setItem: (key, value) => void items.set(key, value),
    removeItem: (key) => void items.delete(key),
  }
}

function renderApp(storage: SessionStorageLike = memoryStorage()) {
  const sockets = fakeSockets()
  const services = createApiServices(source, { createSocket: sockets.create }, new SessionStore({ storage }))
  const view = render(<App services={services} />)
  act(() => sockets.latest().acceptConnection())
  return { sockets, services, view }
}

const accountTrigger = (total: string) =>
  screen.findByRole('button', { name: new RegExp(`^Open account menu: guest, shadow balance \\${total}$`) })

function accountSubscribes(sent: ClientMessage[]) {
  return sent.flatMap((m) => (m.kind.case === 'subscribe' && m.kind.value.topic === 'account' ? [m.kind.value.sessionToken] : []))
}

describe('WalletMenu', () => {
  it('takes a guest from landing to a funded shadow balance in one click, and subscribes the account', async () => {
    const user = userEvent.setup()
    const { sockets } = renderApp()

    await user.click(screen.getByRole('button', { name: 'Continue as guest' }))

    // The session's own copy of the account shows at once: 0.9 ETH, 1,000 USDC and the NOVA holding.
    expect(await accountTrigger('$3,496.40')).toHaveTextContent('Guest')
    expect(accountSubscribes(sockets.latest().sent)).toEqual(['demo-session-token-fixture'])
  })

  it('labels every balance as shadow in the header and in the panel', async () => {
    const user = userEvent.setup()
    renderApp()
    await user.click(screen.getByRole('button', { name: 'Continue as guest' }))
    const trigger = await accountTrigger('$3,496.40')
    expect(trigger).toHaveTextContent('$3,496.40 shadow')

    await user.click(trigger)
    const panel = await screen.findByRole('dialog', { name: 'Account' })
    expect(panel).toHaveTextContent('Shadow balance. Simulated funds for the demo')
    const table = within(panel).getByRole('table')
    expect(within(table).getAllByRole('columnheader').map((h) => h.textContent)).toEqual([
      'Token',
      'Amount (shadow)',
      'Value (shadow)',
    ])
    expect(within(table).getByRole('rowheader', { name: 'Total (shadow)' })).toBeVisible()
    expect(within(table).getAllByRole('row').map((row) => row.textContent)).toEqual([
      'TokenAmount (shadow)Value (shadow)',
      'ETH0.9$2,250.00',
      'USDC1,000$1,000.00',
      'NOVA19,871.2034$246.40',
      'Total (shadow)$3,496.40',
    ])
  })

  it('moves the balances when the account stream sends a new account, as after a trade', async () => {
    const user = userEvent.setup()
    const { sockets } = renderApp()
    await user.click(screen.getByRole('button', { name: 'Continue as guest' }))
    await accountTrigger('$3,496.40')

    const socket = sockets.latest()
    act(() =>
      socket.push({ case: 'snapshot', value: { topic: 'account', seq: 1n, payload: { case: 'account', value: apiFixture(AccountSnapshotSchema) } } }),
    )
    // A shadow buy: 0.1 ETH out, more NOVA in.
    const before = apiFixture(AccountSchema)
    const after = create(AccountSchema, {
      ...before,
      balances: [
        { ...before.balances[0], amount: '0.8', valueUsd: '2000' },
        before.balances[1],
        { ...before.balances[2], amount: '39742.4068', valueUsd: '492.80584432' },
      ],
    })
    act(() => socket.push({ case: 'delta', value: { topic: 'account', seq: 2n, payload: { case: 'account', value: after } } }))

    const trigger = await accountTrigger('$3,492.81')
    await user.click(trigger)
    const panel = await screen.findByRole('dialog', { name: 'Account' })
    const rows = within(within(panel).getByRole('table')).getAllByRole('row').map((row) => row.textContent)
    expect(rows).toContain('ETH0.8$2,000.00')
    expect(rows).toContain('NOVA39,742.4068$492.81')
    expect(within(panel).getByRole('status', { name: /^Shadow balances: Live\./ })).toBeVisible()
  })

  it('signing out unsubscribes the account stream, drops its data and forgets the session', async () => {
    const user = userEvent.setup()
    const { sockets, services } = renderApp()
    await user.click(screen.getByRole('button', { name: 'Continue as guest' }))
    const socket = sockets.latest()
    act(() =>
      socket.push({ case: 'snapshot', value: { topic: 'account', seq: 1n, payload: { case: 'account', value: apiFixture(AccountSnapshotSchema) } } }),
    )
    expect(services.streams.store.getState().account?.account?.accountId).toBe('acct_guest_7f3a')

    await user.click(await accountTrigger('$3,496.40'))
    await user.click(await screen.findByRole('button', { name: 'Sign out' }))

    expect(screen.getByRole('button', { name: 'Continue as guest' })).toBeVisible()
    expect(socket.subscriptions().filter(([topic]) => topic === 'account')).toEqual([['account', true], ['account', false]])
    expect(services.streams.store.getState().account).toBeUndefined()
    expect(services.session.token()).toBeUndefined()
  })

  it('keeps the session across a reload', async () => {
    const user = userEvent.setup()
    const storage = memoryStorage()
    const first = renderApp(storage)
    await user.click(screen.getByRole('button', { name: 'Continue as guest' }))
    await accountTrigger('$3,496.40')
    first.view.unmount()

    const { sockets } = renderApp(storage)
    expect(await accountTrigger('$3,496.40')).toBeVisible()
    expect(screen.queryByRole('button', { name: 'Continue as guest' })).toBeNull()
    expect(accountSubscribes(sockets.latest().sent)).toEqual(['demo-session-token-fixture'])
  })

  it('says so when the guest account cannot start, and lets the visitor try again', async () => {
    const user = userEvent.setup()
    let fail = true
    server.use(
      http.post(`${source.apiUrl}/v1/session`, () =>
        new HttpResponse('down', { status: fail ? 503 : 500 }),
      ),
    )
    renderApp()
    await user.click(screen.getByRole('button', { name: 'Continue as guest' }))
    expect(await screen.findByRole('alert')).toHaveTextContent("Couldn't start a guest account. Try again.")

    fail = false
    server.resetHandlers()
    await user.click(screen.getByRole('button', { name: 'Continue as guest' }))
    expect(await accountTrigger('$3,496.40')).toBeVisible()
  })
})

describe('AccountMenuView', () => {
  const privy = create(AccountSchema, {
    ...apiFixture(AccountSchema),
    kind: Account_Kind.PRIVY,
    walletAddress: '0x3a7e000000000000000000000000000000009c21',
  })

  it('shows a Privy account by its wallet address, and copies it', async () => {
    const user = userEvent.setup()
    const onCopyAddress = vi.fn(() => Promise.resolve())
    render(<AccountMenuView account={privy} dataState="live" onSignOut={() => undefined} onCopyAddress={onCopyAddress} />)

    const trigger = screen.getByRole('button', { name: /^Open account menu: wallet 0x3a7e0+9c21, shadow balance \$3,496\.40$/ })
    expect(trigger).toHaveTextContent('0x3a7e…9c21')
    await user.click(trigger)
    await user.click(await screen.findByRole('button', { name: 'Copy address' }))
    expect(onCopyAddress).toHaveBeenCalledWith('0x3a7e000000000000000000000000000000009c21')
    await waitFor(() => expect(screen.getByText('Address copied')).toBeInTheDocument())
  })

  it('shows the balance region\'s data state, so old balances never pass as current', async () => {
    const user = userEvent.setup()
    render(<AccountMenuView account={privy} dataState="stale" onSignOut={() => undefined} />)
    await user.click(screen.getByRole('button', { name: /^Open account menu/ }))
    expect(await screen.findByRole('status', { name: /^Shadow balances: Stale\./ })).toBeVisible()
  })

  it('shows a dash, not $0.00, before anything about the account is known', () => {
    render(<AccountMenuView account={undefined} dataState="loading" onSignOut={() => undefined} />)
    expect(screen.getByRole('button', { name: 'Open account menu: guest, shadow balance —' })).toBeVisible()
  })
})
