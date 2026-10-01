import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { ConnectionStatus } from './ConnectionStatus'

afterEach(cleanup)

describe('ConnectionStatus', () => {
  it.each([
    ['live', 'open', 'Live'],
    ['replay', 'open', 'Replay'],
    ['fixtures', 'open', 'Fixtures'],
    ['live', 'connecting', 'Connecting'],
    ['replay', 'reconnecting', 'Reconnecting'],
    ['live', 'stale', 'Stale'],
    ['live', 'unavailable', 'Unavailable'],
  ] as const)('shows %s/%s as %s, in text', (source, state, label) => {
    render(<ConnectionStatus source={source} state={state} />)
    const status = screen.getByRole('status')
    expect(status).toHaveTextContent(label)
    expect(status).toHaveAccessibleName(new RegExp(`^Market data: ${label}\\.`))
  })
})
