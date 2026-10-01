import { act, cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, expect, it } from 'vitest'
import App from './App'
import { createApiServices } from './app/services'
import { readDataSource } from './api/source'
import { fakeSockets } from './test/fakeSocket'

afterEach(cleanup)

it('moves the active route and updates the workspace context', async () => {
  const user = userEvent.setup()
  const sockets = fakeSockets()

  render(<App services={createApiServices(readDataSource({}), { createSocket: sockets.create })} />)
  await user.click(screen.getByRole('button', { name: 'Portfolio' }))

  expect(screen.getByRole('button', { name: 'Portfolio' })).toHaveAttribute(
    'aria-current',
    'page',
  )
  expect(screen.getByRole('main')).toHaveTextContent('Portfolio workspace')
})

it('shows the market-data connection in the header and subscribes the engine status', () => {
  const sockets = fakeSockets()
  render(<App services={createApiServices(readDataSource({}), { createSocket: sockets.create })} />)

  expect(screen.getByRole('status', { name: /^Market data: Connecting\./ })).toBeVisible()
  act(() => sockets.latest().acceptConnection())
  expect(sockets.latest().subscriptions()).toEqual([['status', true]])
})
