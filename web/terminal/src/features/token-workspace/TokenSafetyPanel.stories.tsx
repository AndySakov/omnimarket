import type { Meta, StoryObj } from '@storybook/react-vite'
import { SafetyEvidencePanel } from './TokenPage'
import { tokenWorkspace } from '../../mocks/tokenWorkspaceFixtures'

const meta = {
  title: 'Token Workspace/Safety Evidence',
  component: SafetyEvidencePanel,
  parameters: { layout: 'padded' },
  args: { evidence: tokenWorkspace.safetyEvidence },
} satisfies Meta<typeof SafetyEvidencePanel>

export default meta
type Story = StoryObj<typeof meta>

export const NotChecked: Story = {}

export const Loading: Story = { args: { state: 'loading' } }

export const Empty: Story = { args: { state: 'empty' } }

export const Stale: Story = { args: { state: 'stale' } }

export const Error: Story = { args: { state: 'error' } }

export const Passed: Story = {
  args: {
    evidence: {
      updatedLabel: 'Checked 12s ago',
      sellable: { status: 'passed', value: 'Sellable', detail: 'Simulation passed.' },
      buyTax: { status: 'passed', value: '0.00%', detail: 'No buy tax detected.' },
      sellTax: { status: 'passed', value: '0.00%', detail: 'No sell tax detected.' },
    },
  },
}

export const Warning: Story = {
  args: {
    evidence: {
      ...tokenWorkspace.safetyEvidence,
      updatedLabel: 'Checked 2m ago',
      sellable: { status: 'warning', value: 'Low liquidity', detail: 'Simulation needs review.' },
    },
  },
}

export const Failed: Story = {
  args: {
    evidence: {
      ...tokenWorkspace.safetyEvidence,
      updatedLabel: 'Checked 2m ago',
      sellable: { status: 'failed', value: 'Not sellable', detail: 'Simulation failed.' },
    },
  },
}
