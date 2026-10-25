// The terminal's streamed regions (#139): each component that shows stream data, its stories and
// the states it shows. `regions.test.tsx` renders every region with hostile contract values, checks
// each has a story per state, and fails when a component under `features/` reads the stream
// without being listed here. A new region joins this list in the PR that adds it.

import { createElement, type ReactElement } from 'react'
import { DiscoveryPage } from './features/discovery-feed/DiscoveryPage'
import { EngineStatusBar } from './features/engine-status/EngineStatusBar'
import { HowItWorks } from './features/engine-status/HowItWorks'
import { TokenPage } from './features/token-workspace/TokenPage'
import { canonicalWorkspaceAddress } from './features/token-workspace/tokenWorkspaceData'

/** The data states every streamed region shows (`docs/spec/frontend.md`). */
export const streamStates = ['loading', 'live', 'stale', 'reconnecting', 'unavailable'] as const

export type RegionState = (typeof streamStates)[number] | 'empty' | 'error'

export type Region = {
  name: string
  /** The file, under `src/`, whose component reads the stream. */
  component: string
  /** Its stories, under `src/`: one export per state, named for it (`Loading`, `Live`, ...). */
  stories: string
  /** The states it shows beyond the stream's own. */
  extraStates: ReadonlyArray<'empty' | 'error'>
  /** The region on a stream, as the hostile-value test renders it. Regions on one page share one. */
  render: () => ReactElement
  /** The button that brings its numbers on screen, when they're behind one. */
  reveal?: string
}

const tokenPage = () => createElement(TokenPage, { address: canonicalWorkspaceAddress, onBack: () => undefined })

export const regions: Region[] = [
  {
    name: 'discover',
    component: 'features/discovery-feed/DiscoveryPage.tsx',
    stories: 'features/discovery-feed/DiscoveryPage.stories.tsx',
    extraStates: ['empty', 'error'],
    render: () => createElement(DiscoveryPage),
  },
  {
    name: 'engine status',
    component: 'features/engine-status/EngineStatusBar.tsx',
    stories: 'features/engine-status/EngineStatusBar.stories.tsx',
    extraStates: [],
    render: () => createElement(EngineStatusBar),
  },
  {
    name: 'how it works',
    component: 'features/engine-status/HowItWorks.tsx',
    stories: 'features/engine-status/HowItWorks.stories.tsx',
    extraStates: [],
    render: () => createElement(HowItWorks),
    reveal: 'How it works',
  },
  {
    name: 'token header',
    component: 'features/token-workspace/TokenPage.tsx',
    stories: 'features/token-workspace/TokenWorkspaceHeader.stories.tsx',
    extraStates: ['empty', 'error'],
    render: tokenPage,
  },
  {
    name: 'token chart',
    component: 'features/token-workspace/TokenPage.tsx',
    stories: 'features/token-workspace/TokenChartPanel.stories.tsx',
    extraStates: ['empty', 'error'],
    render: tokenPage,
  },
  {
    name: 'token safety',
    component: 'features/token-workspace/TokenPage.tsx',
    stories: 'features/token-workspace/TokenSafetyPanel.stories.tsx',
    extraStates: ['empty', 'error'],
    render: tokenPage,
  },
  {
    name: 'token trades',
    component: 'features/token-workspace/TokenPage.tsx',
    stories: 'features/token-workspace/TokenTradesTape.stories.tsx',
    extraStates: ['empty', 'error'],
    render: tokenPage,
  },
]
