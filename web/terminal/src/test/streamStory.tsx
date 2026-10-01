// Storybook decorator: gives a story a stream in a fixed connection state, without a socket.
import type { Decorator } from '@storybook/react-vite'
import type { DataSourceKind } from '../api/source'
import { StreamManager } from '../api/stream/manager'
import { StreamProvider } from '../api/stream/StreamProvider'
import type { ConnectionState } from '../api/stream/state'

export function withStream(source: DataSourceKind, connection: ConnectionState): Decorator {
  const manager = new StreamManager({ url: 'ws://storybook.invalid/v1/stream' })
  manager.store.setState({ connection })
  return (Story) => (
    <StreamProvider manager={manager} source={source}>
      <Story />
    </StreamProvider>
  )
}
