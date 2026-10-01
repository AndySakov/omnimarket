// The header's account control (#66, D101). Signed out, it's one button: "Continue as guest" starts
// a demo account with a shadow balance. Signed in, it shows the account ("Guest", or a Privy
// wallet's address) and its shadow total, and opens onto the balances, copy address and sign out.
// Balances come from the `account` topic, so they move after a trade; until its snapshot lands, the
// session's own copy of the account stands in.

import { useEffect, useRef, useState } from 'react'
import Decimal from 'decimal.js'
import { Popover } from 'radix-ui'
import { Check, ChevronDown, Copy, LogOut, UserRound } from 'lucide-react'
import { Account_Kind, type Account, type Balance } from '../../api/generated/omnimarket/api/v1/trading_pb'
import type { DataState } from '../../api/stream/dataState'
import { useAccountStream } from '../../api/stream/hooks'
import { useSession, useSessionStore, useStartGuestSession } from '../../api/useSession'
import { parseDecimal } from '../../shared/decimal'
import { formatTokenAmount, formatUsd } from '../../shared/format'
import { DataStatus } from '../../shared/ui/DataStatus'
import './account.css'

/** Connected to the session and the stream; the header renders this. */
export function WalletMenu() {
  const session = useSession()
  // The control swaps on sign-in and sign-out; focus follows it so a keyboard user isn't dropped
  // back to the top of the page.
  const [focusNext, setFocusNext] = useState(false)
  if (!session) return <GuestEntry autoFocus={focusNext} onStarted={() => setFocusNext(true)} />
  // Keyed by token: a new session remounts the menu, so the `account` topic is resubscribed with
  // the new token, and signing out unmounts it, which unsubscribes and drops the account's data.
  return (
    <SignedInMenu
      key={session.sessionToken}
      fallback={session.account}
      autoFocus={focusNext}
      onSignedOut={() => setFocusNext(true)}
    />
  )
}

function GuestEntry({ autoFocus, onStarted }: { autoFocus: boolean; onStarted: () => void }) {
  const start = useStartGuestSession()
  return (
    <GuestEntryView
      status={start.isPending ? 'starting' : start.isError ? 'error' : 'idle'}
      onStart={() => {
        // Before the session lands, so the account trigger mounts already asked to take focus.
        onStarted()
        start.mutate()
      }}
      autoFocus={autoFocus}
    />
  )
}

function SignedInMenu({
  fallback,
  autoFocus,
  onSignedOut,
}: {
  fallback: Account | undefined
  autoFocus: boolean
  onSignedOut: () => void
}) {
  const session = useSessionStore()
  const { data, state } = useAccountStream()
  return (
    <AccountMenuView
      account={data?.account ?? fallback}
      dataState={state}
      autoFocus={autoFocus}
      onSignOut={() => {
        onSignedOut()
        session.clear()
      }}
    />
  )
}

/** Focuses the element once on mount when asked to. */
function useFocusOnMount<T extends HTMLElement>(enabled: boolean | undefined) {
  const ref = useRef<T>(null)
  useEffect(() => {
    if (enabled) ref.current?.focus()
    // Mount only: a later prop change must not steal focus.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return ref
}

export type GuestEntryStatus = 'idle' | 'starting' | 'error'

export function GuestEntryView({
  status,
  onStart,
  autoFocus,
}: {
  status: GuestEntryStatus
  onStart: () => void
  autoFocus?: boolean
}) {
  const ref = useFocusOnMount<HTMLButtonElement>(autoFocus)
  const detail =
    status === 'starting' ? 'Opening a demo account…' : status === 'error' ? "Couldn't start. Try again." : 'Shadow balance, no signup'
  return (
    <div className="guest-entry">
      <button
        ref={ref}
        className="wallet-menu wallet-menu--guest"
        type="button"
        onClick={onStart}
        disabled={status === 'starting'}
        aria-busy={status === 'starting'}
        aria-label="Continue as guest"
        aria-describedby="guest-entry-detail"
      >
        <span className="wallet-avatar" aria-hidden="true">
          <UserRound size={14} />
        </span>
        <span className="wallet-menu__details">
          <span className="wallet-menu__title">Continue as guest</span>
          <span
            id="guest-entry-detail"
            className={`wallet-menu__balance ${status === 'error' ? 'wallet-menu__balance--error' : ''}`}
          >
            {detail}
          </span>
        </span>
      </button>
      {status === 'error' && (
        <span className="sr-only" role="alert">
          Couldn't start a guest account. Try again.
        </span>
      )}
    </div>
  )
}

export type AccountMenuViewProps = {
  /** Undefined only before anything about the account is known. */
  account: Account | undefined
  /** The `account` topic's state; the balances say whether they're current. */
  dataState: DataState
  onSignOut: () => void
  /** Defaults to the clipboard API; stories and tests pass their own. */
  onCopyAddress?: (address: string) => Promise<void>
  /** Open on first render, for stories and visual tests. */
  defaultOpen?: boolean
  /** Focus the trigger on mount: it has just replaced the control that had focus. */
  autoFocus?: boolean
}

export function AccountMenuView({ account, dataState, onSignOut, onCopyAddress, defaultOpen, autoFocus }: AccountMenuViewProps) {
  const triggerRef = useFocusOnMount<HTMLButtonElement>(autoFocus)
  const privy = account?.kind === Account_Kind.PRIVY && account.walletAddress !== ''
  const name = privy ? shortAddress(account.walletAddress) : 'Guest'
  const balances = account?.balances ?? []
  const total = account ? shadowTotalUsd(balances) : undefined
  const totalText = total === undefined ? '—' : formatUsd(total)

  return (
    <Popover.Root defaultOpen={defaultOpen}>
      <Popover.Trigger asChild>
        <button
          ref={triggerRef}
          className="wallet-menu"
          type="button"
          aria-label={`Open account menu: ${privy ? `wallet ${account.walletAddress}` : 'guest'}, shadow balance ${totalText}`}
        >
          <span className="wallet-avatar" aria-hidden="true">
            {privy ? account.walletAddress.slice(2, 4).toUpperCase() : 'G'}
          </span>
          <span className="wallet-menu__details">
            <span className="wallet-menu__address">{name}</span>
            <span className="wallet-menu__balance">
              {totalText} <span className="shadow-tag">shadow</span>
            </span>
          </span>
          <ChevronDown size={14} aria-hidden="true" />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content className="account-panel" align="end" sideOffset={8} aria-label="Account">
          <div className="account-panel__head">
            <div>
              <div className="account-panel__eyebrow">{privy ? 'Privy wallet' : 'Guest account'}</div>
              <div className="account-panel__name">{privy ? <code>{account.walletAddress}</code> : 'Guest'}</div>
            </div>
            {privy && <CopyAddressButton address={account.walletAddress} onCopy={onCopyAddress} />}
          </div>

          <p className="account-panel__shadow" id="account-shadow-note">
            <strong>Shadow balance.</strong> Simulated funds for the demo: trades are simulated and never
            broadcast, and nothing here is real money.
          </p>

          <section className="account-panel__balances" aria-labelledby="account-balances-title">
            <div className="account-panel__balances-head">
              <h2 id="account-balances-title">Shadow balances</h2>
              <DataStatus state={dataState} label="Shadow balances" />
            </div>
            {account === undefined ? (
              <p className="account-panel__empty">Loading the account…</p>
            ) : balances.length === 0 ? (
              <p className="account-panel__empty">No shadow balance yet.</p>
            ) : (
              <table aria-describedby="account-shadow-note">
                <thead>
                  <tr>
                    <th scope="col">Token</th>
                    <th scope="col">Amount (shadow)</th>
                    <th scope="col">Value (shadow)</th>
                  </tr>
                </thead>
                <tbody>
                  {balances.map((balance) => (
                    <BalanceRow key={balance.token?.address ?? ''} balance={balance} />
                  ))}
                </tbody>
                <tfoot>
                  <tr>
                    <th scope="row" colSpan={2}>Total (shadow)</th>
                    <td>{totalText}</td>
                  </tr>
                </tfoot>
              </table>
            )}
          </section>

          <button className="account-panel__sign-out" type="button" onClick={onSignOut}>
            <LogOut size={14} aria-hidden="true" />
            Sign out
          </button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

function BalanceRow({ balance }: { balance: Balance }) {
  const amount = parseDecimal(balance.amount || '0')
  const value = balance.valueUsd ? parseDecimal(balance.valueUsd) : undefined
  return (
    <tr>
      <th scope="row">{balance.token?.symbol || shortAddress(balance.token?.address ?? '')}</th>
      <td>{formatTokenAmount(amount)}</td>
      <td>{value === undefined ? '—' : formatUsd(value)}</td>
    </tr>
  )
}

function CopyAddressButton({ address, onCopy }: { address: string; onCopy?: (address: string) => Promise<void> }) {
  const [copied, setCopied] = useState<'idle' | 'copied' | 'failed'>('idle')
  async function copy() {
    try {
      await (onCopy ?? ((text: string) => navigator.clipboard.writeText(text)))(address)
      setCopied('copied')
    } catch {
      setCopied('failed')
    }
  }
  return (
    <button className="icon-button account-panel__copy" type="button" onClick={copy} aria-label="Copy address">
      {copied === 'copied' ? <Check size={15} aria-hidden="true" /> : <Copy size={15} aria-hidden="true" />}
      <span className="sr-only" role="status">
        {copied === 'copied' ? 'Address copied' : copied === 'failed' ? "Couldn't copy the address" : ''}
      </span>
    </button>
  )
}

/** The sum of the balances' USD values; undefined if any balance has no value yet. */
function shadowTotalUsd(balances: Balance[]): Decimal | undefined {
  let total = new Decimal(0)
  for (const balance of balances) {
    if (!balance.valueUsd) return undefined
    total = total.plus(parseDecimal(balance.valueUsd))
  }
  return total
}

function shortAddress(address: string): string {
  return address.length > 10 ? `${address.slice(0, 6)}…${address.slice(-4)}` : address
}
