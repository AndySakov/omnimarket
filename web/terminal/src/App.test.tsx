import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { expect, it } from 'vitest'
import App from './App'

it('moves the active route and updates the workspace context', async () => {
  const user = userEvent.setup()

  render(<App />)
  await user.click(screen.getByRole('button', { name: 'Portfolio' }))

  expect(screen.getByRole('button', { name: 'Portfolio' })).toHaveAttribute(
    'aria-current',
    'page',
  )
  expect(screen.getByRole('main')).toHaveTextContent('Portfolio workspace')
})
