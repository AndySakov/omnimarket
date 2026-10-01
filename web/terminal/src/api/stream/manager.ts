// The terminal's one WebSocket connection (#63, stream.proto). Components subscribe to the topics
// they show and unsubscribe when they unmount; the manager reference-counts them, so a topic is on
// the wire only while something on screen needs it. Messages go through `applyServerMessage` into
// one Zustand store.
//
// Failure handling:
// - A drop reconnects with exponential backoff and resubscribes every topic still wanted. Data
//   stays on screen, labelled reconnecting, until each topic's new snapshot replaces it.
// - Three missed heartbeats (one a second) mark the connection stale. Ten seconds of silence
//   counts as a dead connection: it is closed and reconnected.
// - A seq gap on one topic resubscribes that topic alone.

import { fromJsonString, create, toJsonString } from '@bufbuild/protobuf'
import { createStore, type StoreApi } from 'zustand/vanilla'
import {
  ClientMessageSchema,
  ServerMessageSchema,
  type ServerMessage,
} from '../generated/omnimarket/api/v1/stream_pb'
import {
  applyServerMessage,
  forgetTopic,
  initialStreamState,
  markAllResubscribing,
  markSubscribed,
  type StreamState,
} from './state'
import { ACCOUNT_TOPIC } from './topics'

/** The part of the browser's WebSocket the manager uses, so tests can hand it a fake. */
export type SocketLike = {
  readonly readyState: number
  send(data: string): void
  close(code?: number, reason?: string): void
  onopen: ((event: Event) => void) | null
  onclose: ((event: CloseEvent) => void) | null
  onerror: ((event: Event) => void) | null
  onmessage: ((event: MessageEvent) => void) | null
}

export type StreamManagerOptions = {
  url: string
  createSocket?: (url: string) => SocketLike
  /** The bearer token for the `account` topic, read at each subscribe. */
  getSessionToken?: () => string | undefined
  /** Backoff before the first retry; doubles per failed attempt up to `maxBackoffMs`. */
  initialBackoffMs?: number
  maxBackoffMs?: number
  /** Failed attempts in a row before the connection shows as unavailable. */
  unavailableAfterAttempts?: number
  /** Silence before the data is stale: three missed one-second heartbeats. */
  staleAfterMs?: number
  /** Silence before the connection is presumed dead and replaced. */
  deadAfterMs?: number
  /** Called with messages that don't parse; the connection carries on. */
  onProtocolError?: (error: unknown) => void
}

const OPEN = 1

export class StreamManager {
  readonly store: StoreApi<StreamState>

  private readonly url: string
  private readonly createSocket: (url: string) => SocketLike
  private readonly getSessionToken: () => string | undefined
  private readonly initialBackoffMs: number
  private readonly maxBackoffMs: number
  private readonly unavailableAfterAttempts: number
  private readonly staleAfterMs: number
  private readonly deadAfterMs: number
  private readonly onProtocolError: (error: unknown) => void

  private socket: SocketLike | undefined
  private running = false
  /** Failed attempts since the last successful open. */
  private failures = 0
  private retryTimer: ReturnType<typeof setTimeout> | undefined
  private staleTimer: ReturnType<typeof setTimeout> | undefined
  private deadTimer: ReturnType<typeof setTimeout> | undefined
  /** How many mounted components want each topic. */
  private readonly wanted = new Map<string, number>()

  constructor(options: StreamManagerOptions) {
    this.url = options.url
    this.createSocket = options.createSocket ?? ((url) => new WebSocket(url))
    this.getSessionToken = options.getSessionToken ?? (() => undefined)
    this.initialBackoffMs = options.initialBackoffMs ?? 500
    this.maxBackoffMs = options.maxBackoffMs ?? 10_000
    this.unavailableAfterAttempts = options.unavailableAfterAttempts ?? 5
    this.staleAfterMs = options.staleAfterMs ?? 3_000
    this.deadAfterMs = options.deadAfterMs ?? 10_000
    this.onProtocolError = options.onProtocolError ?? ((error) => console.warn('stream: bad message', error))
    this.store = createStore<StreamState>(() => initialStreamState())
  }

  /** Opens the connection. Safe to call again after `stop`. */
  start(): void {
    if (this.running) return
    this.running = true
    this.failures = 0
    this.setConnection('connecting')
    this.open()
  }

  /** Closes the connection and stops retrying. Subscriptions are kept for the next `start`. */
  stop(): void {
    this.running = false
    this.clearTimers()
    const socket = this.socket
    this.socket = undefined
    if (socket) {
      detach(socket)
      socket.close(1000, 'client stopped')
    }
    this.setConnection('idle')
  }

  /**
   * Wants a topic until the returned function is called. The first caller subscribes on the wire;
   * the last one to leave unsubscribes and drops the topic's data.
   */
  subscribe(topic: string): () => void {
    const count = this.wanted.get(topic) ?? 0
    this.wanted.set(topic, count + 1)
    if (count === 0) {
      this.store.setState((state) => markSubscribed(state, topic))
      this.sendSubscribe(topic)
    }
    let released = false
    return () => {
      if (released) return
      released = true
      this.release(topic)
    }
  }

  private release(topic: string): void {
    const count = this.wanted.get(topic) ?? 0
    if (count > 1) {
      this.wanted.set(topic, count - 1)
      return
    }
    this.wanted.delete(topic)
    this.send(toJsonString(ClientMessageSchema, create(ClientMessageSchema, {
      kind: { case: 'unsubscribe', value: { topic } },
    })))
    this.store.setState((state) => forgetTopic(state, topic))
  }

  private open(): void {
    let socket: SocketLike
    try {
      socket = this.createSocket(this.url)
    } catch (error) {
      // A URL the browser refuses counts as a failed attempt, so it escalates like any other.
      this.onProtocolError(error)
      this.recordFailure()
      return
    }
    this.socket = socket
    socket.onopen = () => this.handleOpen(socket)
    socket.onmessage = (event) => this.handleMessage(socket, event)
    socket.onclose = () => this.handleClose(socket)
    // An error is always followed by a close, which does the work.
    socket.onerror = () => undefined
  }

  private handleOpen(socket: SocketLike): void {
    if (socket !== this.socket) return
    this.setConnection('open')
    this.store.setState(markAllResubscribing)
    for (const topic of this.wanted.keys()) this.sendSubscribe(topic)
    this.armLivenessTimers()
  }

  private handleMessage(socket: SocketLike, event: MessageEvent): void {
    if (socket !== this.socket || typeof event.data !== 'string') return
    let message: ServerMessage
    try {
      message = fromJsonString(ServerMessageSchema, event.data, { ignoreUnknownFields: true })
    } catch (error) {
      this.onProtocolError(error)
      return
    }
    // The server answered, so the connection works: the next drop starts the backoff afresh.
    // Not on open, or a server that accepts and closes at once would never show unavailable.
    this.failures = 0
    if (message.kind.case === 'heartbeat') {
      if (this.store.getState().connection === 'stale') this.setConnection('open')
      this.armLivenessTimers()
      this.store.setState({ serverTimeMs: message.kind.value.serverTimeMs })
      return
    }
    const { state, resubscribe } = applyServerMessage(this.store.getState(), message)
    this.store.setState(state, true)
    for (const topic of resubscribe) {
      this.send(toJsonString(ClientMessageSchema, create(ClientMessageSchema, {
        kind: { case: 'unsubscribe', value: { topic } },
      })))
      this.sendSubscribe(topic)
    }
  }

  private handleClose(socket: SocketLike): void {
    if (socket !== this.socket) return
    detach(socket)
    this.socket = undefined
    this.clearLivenessTimers()
    if (!this.running) return
    this.recordFailure()
  }

  private recordFailure(): void {
    this.failures += 1
    this.setConnection(this.failures >= this.unavailableAfterAttempts ? 'unavailable' : 'reconnecting')
    this.scheduleRetry()
  }

  private scheduleRetry(): void {
    if (!this.running) return
    // No jitter: one terminal per browser tab talks to the server, so retries don't stampede.
    const delay = Math.min(this.initialBackoffMs * 2 ** Math.max(this.failures - 1, 0), this.maxBackoffMs)
    this.retryTimer = setTimeout(() => {
      this.retryTimer = undefined
      if (this.running) this.open()
    }, delay)
  }

  private armLivenessTimers(): void {
    this.clearLivenessTimers()
    this.staleTimer = setTimeout(() => {
      if (this.store.getState().connection === 'open') this.setConnection('stale')
    }, this.staleAfterMs)
    this.deadTimer = setTimeout(() => {
      const socket = this.socket
      if (!socket) return
      // Treat the silent socket as closed: forget it, then reconnect as after a drop.
      this.handleClose(socket)
      socket.close(4000, 'no heartbeat')
    }, this.deadAfterMs)
  }

  private sendSubscribe(topic: string): void {
    const sessionToken = topic === ACCOUNT_TOPIC ? (this.getSessionToken() ?? '') : ''
    this.send(toJsonString(ClientMessageSchema, create(ClientMessageSchema, {
      kind: { case: 'subscribe', value: { topic, sessionToken } },
    })))
  }

  /** Sends when connected; otherwise the next open resubscribes from `wanted`. */
  private send(data: string): void {
    if (this.socket && this.socket.readyState === OPEN) this.socket.send(data)
  }

  private setConnection(connection: StreamState['connection']): void {
    if (this.store.getState().connection !== connection) this.store.setState({ connection })
  }

  private clearLivenessTimers(): void {
    clearTimeout(this.staleTimer)
    clearTimeout(this.deadTimer)
    this.staleTimer = undefined
    this.deadTimer = undefined
  }

  private clearTimers(): void {
    this.clearLivenessTimers()
    clearTimeout(this.retryTimer)
    this.retryTimer = undefined
  }
}

function detach(socket: SocketLike): void {
  socket.onopen = null
  socket.onmessage = null
  socket.onclose = null
  socket.onerror = null
}
