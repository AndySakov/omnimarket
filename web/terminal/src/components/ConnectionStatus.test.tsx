import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { ConnectionStatusView } from './ConnectionStatus'

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
    render(<ConnectionStatusView source={source} state={state} />)
    const status = screen.getByRole('status')
    expect(status).toHaveTextContent(label)
    expect(status).toHaveAccessibleName(new RegExp(`^Market data: ${label}\\.`))
  })
  it('takes Live or Replay from the engine\'s own mode, but never relabels fixtures', () => {
    const { rerender } = render(<ConnectionStatusView source="live" state="open" engineMode="replay" />)
    expect(screen.getByRole('status')).toHaveTextContent('Replay')
    rerender(<ConnectionStatusView source="replay" state="open" engineMode="live" />)
    expect(screen.getByRole('status')).toHaveTextContent('Live')
    rerender(<ConnectionStatusView source="fixtures" state="open" engineMode="live" />)
    expect(screen.getByRole('status')).toHaveTextContent('Fixtures')
    rerender(<ConnectionStatusView source="live" state="reconnecting" engineMode="replay" />)
    expect(screen.getByRole('status')).toHaveTextContent('Reconnecting')
  })
})
