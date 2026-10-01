// React access to the stream (#63). Each hook subscribes its topic while mounted and selects only
// its own slice of the store, so a tick on one token rerenders that token's components alone.

import { createContext, useContext, useEffect } from 'react'
import { useStore } from 'zustand'
import type { Candle, DiscoveryRow, EngineStatus, TokenSnapshot, Trade } from '../generated/omnimarket/api/v1/market_pb'
import type { DataSourceKind } from '../source'
import { dataStateOf, type DataState } from './dataState'
import type { StreamManager } from './manager'
import type { AccountState, ConnectionState, DiscoveryState, StreamState } from './state'
import {
  ACCOUNT_TOPIC,
  candlesTopic,
  DISCOVERY_TOPIC,
  STATUS_TOPIC,
  tokenTopic,
  tradesTopic,
  type CandleIntervalLabel,
} from './topics'

export type StreamContextValue = { manager: StreamManager; source: DataSourceKind }

/** Provided by `<StreamProvider>`. */
export const StreamContext = createContext<StreamContextValue | null>(null)

function useStreamContext(): StreamContextValue {
  const value = useContext(StreamContext)
  if (!value) throw new Error('stream hooks need a <StreamProvider>')
  return value
}

/** Reads a slice of the stream store; rerenders only when the slice's identity changes. */
export function useStreamState<T>(selector: (state: StreamState) => T): T {
  return useStore(useStreamContext().manager.store, selector)
}

/** Keeps `topic` subscribed while the calling component is mounted. */
export function useTopic(topic: string | undefined): void {
  const { manager } = useStreamContext()
  useEffect(() => (topic ? manager.subscribe(topic) : undefined), [manager, topic])
}

export function useDataState(topic: string | undefined): DataState {
  return useStreamState((state) => dataStateOf(state.connection, topic ? state.topics[topic] : undefined))
}

/** The connection as the header shows it, with the source it's connected to. */
export function useConnection(): { state: ConnectionState; source: DataSourceKind } {
  const { source } = useStreamContext()
  return { state: useStreamState((s) => s.connection), source }
}

export type Streamed<T> = { data: T | undefined; state: DataState }

function useStreamed<T>(topic: string | undefined, selector: (state: StreamState) => T | undefined): Streamed<T> {
  useTopic(topic)
  return { data: useStreamState(selector), state: useDataState(topic) }
}

export function useEngineStatusStream(): Streamed<EngineStatus> {
  return useStreamed(STATUS_TOPIC, (s) => s.status)
}

export function useDiscoveryStream(): Streamed<DiscoveryState> {
  return useStreamed(DISCOVERY_TOPIC, (s) => s.discovery)
}

/**
 * One discovery row, without subscribing: the table subscribes to `discovery` once, and each row
 * selects its own record so an update to one row leaves the others alone.
 */
export function useDiscoveryRow(address: string): DiscoveryRow | undefined {
  const key = address.toLowerCase()
  return useStreamState((s) => s.discovery?.rows[key])
}

export function useTokenStream(address: string): Streamed<TokenSnapshot> {
  const topic = tokenTopic(address)
  return useStreamed(topic, (s) => s.tokens[topic])
}

export function useTradesStream(address: string): Streamed<Trade[]> {
  const topic = tradesTopic(address)
  return useStreamed(topic, (s) => s.trades[topic])
}

export function useCandlesStream(address: string, interval: CandleIntervalLabel): Streamed<Candle[]> {
  const topic = candlesTopic(address, interval)
  return useStreamed(topic, (s) => s.candles[topic])
}

export function useAccountStream(): Streamed<AccountState> {
  return useStreamed(ACCOUNT_TOPIC, (s) => s.account)
}
