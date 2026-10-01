// "How it works" (#71): one click from the header, on any route.
import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it } from 'vitest'
import { StreamManager } from '../../api/stream/manager'
import { StreamProvider } from '../../api/stream/StreamProvider'
import { GlobalHeader } from '../../components/GlobalHeader'
import type { DataSourceKind } from '../../api/source'

afterEach(cleanup)

function renderHeader(activeRoute: string, source: DataSourceKind = 'fixtures') {
  const manager = new StreamManager({ url: 'ws://api.test/v1/stream', createSocket: () => { throw new Error('offline') } })
  render(
    <StreamProvider manager={manager} source={source}>
      <GlobalHeader activeRoute={activeRoute} onNavigate={() => undefined} />
    </StreamProvider>,
  )
}

describe('How it works', () => {
  it.each(['Discover', 'Portfolio', 'Settings'])('opens in one click from the header on %s', async (route) => {
    renderHeader(route)
    await userEvent.click(screen.getByRole('button', { name: 'How it works' }))
    const dialog = screen.getByRole('dialog', { name: 'How it works' })
    expect(dialog).toBeVisible()
  })

  it('says what is live and what is shadow, draws the data path and links the sources', async () => {
    renderHeader('Discover')
    await userEvent.click(screen.getByRole('button', { name: 'How it works' }))
    const dialog = screen.getByRole('dialog', { name: 'How it works' })

    expect(within(dialog).getByRole('region', { name: 'Live' })).toHaveTextContent(/Base market data/)
    expect(within(dialog).getByRole('region', { name: 'Shadow' })).toHaveTextContent(/simulated.*Nothing is broadcast/)

    const stages = within(within(dialog).getByRole('list', { name: /^Data path/ })).getAllByRole('listitem')
    expect(stages.map((stage) => stage.querySelector('strong')?.textContent)).toEqual(['Chain engine', 'Kafka', 'API', 'Terminal'])

    expect(within(dialog).getByRole('link', { name: 'The project README' })).toHaveAttribute('href', 'https://github.com/AndySakov/omnimarket#readme')
    expect(within(dialog).getByRole('link', { name: 'The decision log' })).toHaveAttribute('href', 'https://github.com/AndySakov/omnimarket/blob/main/docs/spec/decisions.md')
    expect(within(dialog).getByRole('link', { name: /^#52/ })).toHaveAttribute('href', 'https://github.com/AndySakov/omnimarket/pull/52')
  })

  it('says when the terminal is showing sample data', async () => {
    renderHeader('Discover', 'fixtures')
    await userEvent.click(screen.getByRole('button', { name: 'How it works' }))
    expect(screen.getByRole('dialog')).toHaveAccessibleDescription(/built-in sample data, not the chain/)
  })

  it('closes on Escape and returns focus to its button', async () => {
    renderHeader('Discover')
    const trigger = screen.getByRole('button', { name: 'How it works' })
    await userEvent.click(trigger)
    await userEvent.keyboard('{Escape}')
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
    expect(trigger).toHaveFocus()
  })
})
