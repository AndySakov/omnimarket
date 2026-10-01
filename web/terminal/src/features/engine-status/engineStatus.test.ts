import { create } from '@bufbuild/protobuf'
import { describe, expect, it } from 'vitest'
import {
  EngineStatus_Mode,
  EngineStatusSchema,
} from '../../api/generated/omnimarket/api/v1/market_pb'
import { apiFixture } from '../../mocks/fixtures/api'
import { engineStatusState, formatUptime, HEAD_STALE_AFTER_MS, presentEngineStatus } from './engineStatus'

describe('presentEngineStatus', () => {
  it('summarises the fixture status for the bar', () => {
    const view = presentEngineStatus(apiFixture(EngineStatusSchema), 'live')
    expect(view).toMatchObject({
      chain: 'Base',
      block: '36,120,450',
      lag: '1 block · 2.3s',
      pools: '70,086',
      shadowChecks: '5,117 / 5,120',
      shadowChecksDetail: '5,117 of 5,120 agreed with the chain; 3 disagreed.',
      mode: 'Live',
      coreInstance: 'engine-base-1',
      recording: 'on',
      uptime: '1h 30m',
      headHash: '0x8e1f6a…c6d7',
    })
    expect(view.venues).toEqual([
      { venue: 'uniswap-v2', known: '48,210', active: '1,312' },
      { venue: 'uniswap-v3', known: '21,876', active: '904' },
    ])
  })

  it('takes Live or Replay from the engine, but fixture builds always say Fixtures', () => {
    const replay = create(EngineStatusSchema, { chainId: 8453n, mode: EngineStatus_Mode.REPLAY })
    expect(presentEngineStatus(replay, 'live').mode).toBe('Replay')
    expect(presentEngineStatus(replay, 'replay').mode).toBe('Replay')
    expect(presentEngineStatus(create(EngineStatusSchema, { mode: EngineStatus_Mode.LIVE }), 'replay').mode).toBe('Live')
    expect(presentEngineStatus(replay, 'fixtures').mode).toBe('Fixtures')
  })

  it('says plainly when no shadow checks have run, and pluralises lag', () => {
    const view = presentEngineStatus(create(EngineStatusSchema, { chainId: 8453n, lagBlocks: 3n, lagMs: 6400n, recording: false }), 'live')
    expect(view.shadowChecksDetail).toBe('No shadow checks run yet.')
    expect(view.shadowChecks).toBe('0 / 0')
    expect(view.lag).toBe('3 blocks · 6.4s')
    expect(view.recording).toBe('off')
    expect(view.coreInstance).toBe('unknown')
  })

  it('names an unknown chain by its ID', () => {
    expect(presentEngineStatus(create(EngineStatusSchema, { chainId: 56n }), 'live').chain).toBe('Chain 56')
  })
})

describe('formatUptime', () => {
  it.each([
    [45_000n, '45s'],
    [60_000n, '1m'],
    [5_400_000n, '1h 30m'],
    [90_000_000n, '1d 1h'],
  ])('%s ms is %s', (ms, text) => {
    expect(formatUptime(ms)).toBe(text)
  })
})

describe('engineStatusState', () => {
  it('is live while the head keeps moving', () => {
    expect(engineStatusState('live', 1_000, 1_000 + HEAD_STALE_AFTER_MS)).toBe('live')
  })

  it('goes stale when the head stops, even though the connection is fine', () => {
    expect(engineStatusState('live', 1_000, 1_001 + HEAD_STALE_AFTER_MS)).toBe('stale')
  })

  it('passes the stream state through when the stream is not live', () => {
    for (const state of ['loading', 'stale', 'reconnecting', 'unavailable'] as const) {
      expect(engineStatusState(state, 0, 0)).toBe(state)
    }
    expect(engineStatusState('live', undefined, 99_999)).toBe('live')
  })
})
