// A WebSocket stand-in for stream manager tests: records what the client sends and lets the test
// play the server.

import { create, fromJsonString, toJsonString, type MessageInitShape } from '@bufbuild/protobuf'
import {
  ClientMessageSchema,
  ServerMessageSchema,
  type ClientMessage,
} from '../api/generated/omnimarket/api/v1/stream_pb'
import type { SocketLike } from '../api/stream/manager'

export class FakeSocket implements SocketLike {
  readyState = 0
  readonly sent: ClientMessage[] = []
  closed = false
  onopen: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null

  constructor(readonly url: string) {}

  send(data: string): void {
    this.sent.push(fromJsonString(ClientMessageSchema, data))
  }

  close(): void {
    this.closed = true
    this.readyState = 3
  }

  // Server side.
  acceptConnection(): void {
    this.readyState = 1
    this.onopen?.(new Event('open'))
  }

  push(kind: MessageInitShape<typeof ServerMessageSchema>['kind']): void {
    const data = toJsonString(ServerMessageSchema, create(ServerMessageSchema, { kind }))
    this.onmessage?.(new MessageEvent('message', { data }))
  }

  pushRaw(data: string): void {
    this.onmessage?.(new MessageEvent('message', { data }))
  }

  drop(): void {
    this.readyState = 3
    this.onclose?.(new CloseEvent('close', { code: 1006 }))
  }

  /** The topics subscribed (true) and unsubscribed (false), in order. */
  subscriptions(): Array<[string, boolean]> {
    return this.sent.flatMap((message): Array<[string, boolean]> => {
      if (message.kind.case === 'subscribe') return [[message.kind.value.topic, true]]
      if (message.kind.case === 'unsubscribe') return [[message.kind.value.topic, false]]
      return []
    })
  }
}

/** A socket factory that remembers every socket it made, newest last. */
export function fakeSockets() {
  const sockets: FakeSocket[] = []
  return {
    sockets,
    create: (url: string) => {
      const socket = new FakeSocket(url)
      sockets.push(socket)
      return socket
    },
    latest: () => {
      const socket = sockets.at(-1)
      if (!socket) throw new Error('no socket created yet')
      return socket
    },
  }
}
