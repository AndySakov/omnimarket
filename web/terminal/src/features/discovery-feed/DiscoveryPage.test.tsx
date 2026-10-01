// Live Discover (#65): the table on the real stream manager, with a fake socket playing the server
// and the fixture source's discovery feed as its data.
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { MessageInitShape } from '@bufbuild/protobuf'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { DeltaSchema } from '../../api/generated/omnimarket/api/v1/stream_pb'
import { StreamError_Code } from '../../api/generated/omnimarket/api/v1/stream_pb'
import { StreamManager } from '../../api/stream/manager'
import { StreamProvider } from '../../api/stream/StreamProvider'
import { FixtureDiscovery } from '../../mocks/api/discoveryFeed'
import { fakeSockets } from '../../test/fakeSocket'
import { DiscoveryPage } from './DiscoveryPage'
import { INSERT_HIGHLIGHT_MS, PRICE_FLASH_MS } from './useLiveDiscovery'

const HEAD_MS = 1_790_000_000_000n
const HEAD_BLOCK = 36_120_450n

type DeltaPayload = NonNullable<MessageInitShape<typeof DeltaSchema>['payload']>

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

function setup() {
  const sockets = fakeSockets()
  const manager = new StreamManager({ url: 'ws://test.invalid/v1/stream', createSocket: sockets.create })
  render(
    <StreamProvider manager={manager} source="fixtures">
      <DiscoveryPage />
    </StreamProvider>,
  )
  act(() => {
    manager.start()
    sockets.latest().acceptConnection()
  })
  const socket = sockets.latest()
  const feed = new FixtureDiscovery(HEAD_MS, HEAD_BLOCK)
  let seq = 1n
  return {
    manager,
    socket,
    feed,
    snapshot: () =>
      act(() => socket.push({ case: 'snapshot', value: { topic: 'discovery', seq: 1n, payload: { case: 'discovery', value: feed.snapshot() } } })),
    heartbeat: (serverTimeMs: bigint) =>
      act(() => socket.push({ case: 'heartbeat', value: { serverTimeMs, headBlockNumber: HEAD_BLOCK } })),
    deltas: (payloads: DeltaPayload[]) =>
      act(() => {
        for (const payload of payloads) {
          seq += 1n
          socket.push({ case: 'delta', value: { topic: 'discovery', seq, payload } })
        }
      }),
  }
}

/** The token names in the table, top to bottom. */
function names(): string[] {
  return [...document.querySelectorAll('tbody .token-name')].map((el) => el.textContent ?? '')
}

function rowOf(name: string): HTMLElement {
  return screen.getByRole('button', { name }).closest('tr') as HTMLElement
}

function table(): HTMLElement {
  return screen.getByRole('region', { name: /token stream$/ })
}

describe('live Discover', () => {
  it('shows loading until the snapshot, then New lists pools newest first', () => {
    const { snapshot, heartbeat } = setup()
    expect(screen.getByText('Loading market radar')).toBeVisible()

    snapshot()
    heartbeat(HEAD_MS)
    expect(names()).toEqual([
      'Thin Token (fixture)',
      'Dusk Cat (fixture)',
      'NovaSet (fixture)',
      'Moth Protocol (fixture)',
      'Kelp Labs (fixture)',
      'Rift Finance (fixture)',
    ])
    // Ages count from the server's clock, not the browser's.
    expect(within(rowOf('NovaSet (fixture)')).getByText('7m')).toBeVisible()
    expect(within(rowOf('Thin Token (fixture)')).getByText('20s')).toBeVisible()
    expect(screen.getByRole('status', { name: /^Discovery feed: Live\./ })).toBeVisible()
  })

  it('shows exact values from the feed, formatted at the edge', () => {
    const { snapshot } = setup()
    snapshot()
    const nova = within(rowOf('NovaSet (fixture)'))
    expect(nova.getByText('$12.4M')).toBeVisible()
    expect(nova.getByText('$0.0124')).toBeVisible()
    expect(nova.getByText('$184.3K')).toBeVisible()
    expect(nova.getByText('$41.2K')).toBeVisible()
    expect(nova.getByText('309')).toBeVisible()
    expect(nova.getByText('+6.4%')).toBeVisible()
  })

  it('Trending lists only ranked rows, in rank order', async () => {
    const user = userEvent.setup()
    const { snapshot } = setup()
    snapshot()
    await user.click(screen.getByRole('tab', { name: 'Trending' }))
    expect(names()).toEqual(['NovaSet (fixture)', 'Kelp Labs (fixture)', 'Moth Protocol (fixture)', 'Rift Finance (fixture)'])
  })

  it('inserts a pool created during the session without a reload, highlighted briefly', () => {
    vi.useFakeTimers()
    const { snapshot, heartbeat, deltas, feed } = setup()
    snapshot()
    const now = HEAD_MS + 4_000n
    heartbeat(now)
    deltas(feed.tick(4, now))

    expect(names()[0]).toBe('Brine (fixture)')
    const row = rowOf('Brine (fixture)')
    expect(within(row).getByText('0s')).toBeVisible()
    expect(row.querySelector('.row-motion--inserted')).not.toBeNull()
    // Rows already on screen aren't marked new.
    expect(rowOf('NovaSet (fixture)').querySelector('.row-motion--inserted')).toBeNull()

    act(() => vi.advanceTimersByTime(INSERT_HIGHLIGHT_MS))
    expect(row.querySelector('.row-motion--inserted')).toBeNull()
  })

  it('flashes a price on change, up or down, and clears the flash', () => {
    vi.useFakeTimers()
    const { snapshot, deltas, feed } = setup()
    snapshot()
    const price = (name: string) => within(rowOf(name)).getByTestId('row-price')
    // Tick 1 moves the feed's second row (Thin Token) down 0.8%.
    deltas(feed.tick(1, HEAD_MS + 1_000n))
    expect(price('Thin Token (fixture)')).toHaveClass('price-flash--down')
    expect(price('Thin Token (fixture)')).toHaveTextContent('$0.000003095 (down)')
    expect(price('NovaSet (fixture)')).not.toHaveClass('price-flash--up', 'price-flash--down')

    // Tick 6 moves the first row (NovaSet) up 1.2%; Thin Token's flash is still running.
    act(() => vi.advanceTimersByTime(PRICE_FLASH_MS / 2))
    deltas(feed.tick(6, HEAD_MS + 6_000n))
    expect(price('NovaSet (fixture)')).toHaveClass('price-flash--up')
    expect(price('NovaSet (fixture)')).toHaveTextContent('$0.01255 (up)')

    // Each flash clears on its own clock.
    act(() => vi.advanceTimersByTime(PRICE_FLASH_MS / 2))
    expect(price('Thin Token (fixture)')).not.toHaveClass('price-flash--down')
    expect(price('Thin Token (fixture)')).toHaveTextContent(/^\$0\.000003095$/)
    expect(price('NovaSet (fixture)')).toHaveClass('price-flash--up')
    act(() => vi.advanceTimersByTime(PRICE_FLASH_MS / 2))
    expect(price('NovaSet (fixture)')).not.toHaveClass('price-flash--up')
  })

  it('sorts by the chosen column, highest first, and keeps that order stable across updates', async () => {
    const user = userEvent.setup()
    const { snapshot, deltas, feed } = setup()
    snapshot()
    await user.selectOptions(screen.getByRole('combobox', { name: 'Sort by' }), 'liquidity')
    const byDepth = ['Rift Finance (fixture)', 'Kelp Labs (fixture)', 'NovaSet (fixture)', 'Moth Protocol (fixture)', 'Dusk Cat (fixture)', 'Thin Token (fixture)']
    expect(names()).toEqual(byDepth)
    // Price moves don't change depth, so the order doesn't move.
    deltas([...feed.tick(1, HEAD_MS + 1_000n), ...feed.tick(2, HEAD_MS + 2_000n), ...feed.tick(3, HEAD_MS + 3_000n)])
    expect(names()).toEqual(byDepth)

    await user.click(screen.getByRole('button', { name: 'Liquidity: highest first' }))
    expect(names()).toEqual(byDepth.slice().reverse())

    await user.selectOptions(screen.getByRole('combobox', { name: 'Sort by' }), 'txns')
    expect(names()[0]).toBe('Kelp Labs (fixture)')
  })

  it('holds the order still while the pointer is in the table: no row jumps under it', async () => {
    const user = userEvent.setup()
    const { snapshot, heartbeat, deltas, feed } = setup()
    snapshot()
    heartbeat(HEAD_MS)
    const before = names()

    await user.hover(rowOf('NovaSet (fixture)'))
    const now = HEAD_MS + 4_000n
    deltas(feed.tick(4, now))
    // The new pool would sort first; it waits instead, and nothing above the pointer moves.
    expect(names()).toEqual(before)
    expect(screen.getByTestId('discovery-hold')).toHaveTextContent('1 new waiting')

    await user.unhover(table())
    expect(names()).toEqual(['Brine (fixture)', ...before])
    expect(screen.queryByTestId('discovery-hold')).toBeNull()
  })

  it('holds the order while keyboard focus is in the table, too', () => {
    const { snapshot, deltas, feed } = setup()
    snapshot()
    const before = names()
    act(() => screen.getByRole('button', { name: 'Kelp Labs (fixture)' }).focus())
    deltas(feed.tick(4, HEAD_MS + 4_000n))
    expect(names()).toEqual(before)
    act(() => screen.getByRole('button', { name: 'Kelp Labs (fixture)' }).blur())
    expect(names()[0]).toBe('Brine (fixture)')
  })

  it('keeps a row that leaves the feed during a hold in place, dimmed, until the hold ends', async () => {
    const user = userEvent.setup()
    const { snapshot, deltas } = setup()
    snapshot()
    const before = names()
    await user.hover(rowOf('Rift Finance (fixture)'))
    deltas([{ case: 'discoveryRowRemoved', value: { chainId: 8453n, token: '0xffffffffffffffffffffffffffffffffffffffa1' } }])
    expect(names()).toEqual(before)
    expect(within(rowOf('Moth Protocol (fixture)')).getByText('Left the feed')).toBeVisible()

    await user.unhover(table())
    expect(names()).not.toContain('Moth Protocol (fixture)')
  })

  it('filters by minimum liquidity and maximum age, and says when nothing matches', async () => {
    const user = userEvent.setup()
    const { snapshot, heartbeat } = setup()
    snapshot()
    heartbeat(HEAD_MS)
    await user.click(screen.getByRole('button', { name: /Filters/ }))
    await user.selectOptions(screen.getByRole('combobox', { name: /Min liquidity/ }), '$100K')
    expect(names()).toEqual(['NovaSet (fixture)', 'Kelp Labs (fixture)', 'Rift Finance (fixture)'])
    expect(screen.getByRole('button', { name: /Filters/ })).toHaveTextContent('1')

    await user.selectOptions(screen.getByRole('combobox', { name: /Max age/ }), '15m')
    expect(names()).toEqual(['NovaSet (fixture)'])

    await user.selectOptions(screen.getByRole('combobox', { name: /Min liquidity/ }), '$500K')
    expect(names()).toEqual([])
    expect(screen.getByText('No tokens match these filters')).toBeVisible()
  })

  it('never shows an unchecked token as safe, and screening keeps only passed checks', async () => {
    const user = userEvent.setup()
    const { snapshot } = setup()
    snapshot()
    expect(within(rowOf('Dusk Cat (fixture)')).getByText('Not checked yet')).toBeVisible()
    expect(within(rowOf('Moth Protocol (fixture)')).getByText('Red flag found')).toBeVisible()
    expect(within(rowOf('NovaSet (fixture)')).getByText('Sell check passed')).toBeVisible()

    await user.click(screen.getByRole('button', { name: 'Screening' }))
    expect(names()).toEqual(['NovaSet (fixture)', 'Kelp Labs (fixture)', 'Rift Finance (fixture)'])
  })

  it('marks the rows stale after three missed heartbeats, keeps them, and recovers', () => {
    vi.useFakeTimers()
    const { snapshot, heartbeat } = setup()
    snapshot()
    heartbeat(HEAD_MS)
    act(() => vi.advanceTimersByTime(3_000))

    expect(screen.getByText(/No heartbeat for 3 seconds: these rows may be out of date/)).toBeVisible()
    expect(screen.getByRole('status', { name: /^Discovery feed: Stale\./ })).toBeVisible()
    expect(table()).toHaveClass('discovery-table-shell--stale')
    expect(names()).toHaveLength(6)

    heartbeat(HEAD_MS + 4_000n)
    expect(screen.queryByText(/these rows may be out of date/)).toBeNull()
    expect(table()).not.toHaveClass('discovery-table-shell--stale')
  })

  it('shows the error state when the server can\'t serve the feed', () => {
    const { socket } = setup()
    act(() => socket.push({ case: 'error', value: { code: StreamError_Code.UNAVAILABLE, topic: 'discovery', message: 'down' } }))
    expect(screen.getByRole('alert')).toHaveTextContent('Market radar is unavailable')
  })

  it('shows Base as live, and BNB and MegaETH as coming, not selectable, with a reason', async () => {
    const user = userEvent.setup()
    const { snapshot } = setup()
    snapshot()
    const rail = screen.getByRole('complementary', { name: 'Filter by chain' })
    expect(within(rail).getByRole('button', { name: /^Base/ })).toHaveAttribute('aria-pressed', 'true')
    for (const chain of ['BNB', 'MegaETH']) {
      const button = within(rail).getByRole('button', { name: new RegExp(`^${chain}`) })
      expect(button).toHaveAttribute('aria-disabled', 'true')
      expect(button).toHaveAccessibleDescription(`${chain} is coming. OmniMarket serves Base first; ${chain} follows.`)
      await user.click(button)
      expect(button).not.toHaveAttribute('aria-pressed')
    }
    expect(names()).toHaveLength(6)
  })

  it('a forced state (stories) still wins over the feed', () => {
    render(
      <StreamProvider manager={new StreamManager({ url: 'ws://test.invalid/v1/stream' })} source="fixtures">
        <DiscoveryPage state="error" />
      </StreamProvider>,
    )
    expect(screen.getAllByRole('alert')[0]).toHaveTextContent('Market radar is unavailable')
    fireEvent.click(screen.getAllByRole('tab', { name: 'New' })[0]!)
  })
})
