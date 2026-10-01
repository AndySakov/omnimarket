# Token Workspace Stream Plan

**Status:** Phase 1 implementation slice  
**Owner:** Frontend  
**Scope:** Connection-aware market data for the Token Workspace

## Goal

Keep the Token Workspace UI independent from the eventual backend transport.
The page should consume one typed market snapshot, show whether that snapshot
is fresh, and separately show whether the market stream is connected. A live
WebSocket can then replace the fixture adapter without changing the header,
chart, market tabs, or trade panel.

## Current checkpoint

The fixture adapter already provides deterministic token snapshots, interval
aggregation, trade ticks, loading, and stale-data states. The next slice adds
connection lifecycle state and a controlled reconnect path. Fixture values stay
local; this does not add wallet signing, API fetching, or backend behavior.

## Implementation plan

1. **Define the contract**
   - `TokenWorkspaceStreamStatus` carries `connecting`, `connected`,
     `reconnecting`, or `error`, plus an attempt count and user-facing label.
   - Market freshness remains part of the snapshot (`ready` or `stale`) rather
     than being confused with transport connectivity.
2. **Keep transport behind an adapter**
   - `createTokenWorkspaceStream` remains the fixture adapter.
   - Its listener and status callbacks match the shape a WebSocket adapter will
     provide later.
3. **Model recovery**
   - The fixture adapter can deterministically interrupt and reconnect when a
     test opts in.
   - Cleanup cancels load, tick, disconnect, and reconnect timers.
4. **Expose status in the UI**
   - Chart toolbar, chart footer, and token freshness metadata show the current
     connection without hiding the last known snapshot.
   - A stale snapshot is described as stale data while the stream can still be
     connected; a reconnecting stream is a separate state.
5. **Verify the boundary**
   - Unit tests cover lifecycle ordering and timer cleanup.
   - Playwright verifies the connected status in the normal workspace flow.
   - The backend integration will replace only the adapter after the M2 API
     and WebSocket contract are available.

## Deferred work

- Live WebSocket or server-sent-event transport.
- Generated API types and backend mock-server wiring.
- Authentication, wallet signing, transaction submission, and real quotes.
- Server-driven reconnect backoff and heartbeat timeout policy.

## Acceptance criteria

- The UI can distinguish data freshness from stream connectivity.
- Reconnect does not clear the last usable snapshot or create duplicate timers.
- Switching tokens or intervals cleans up the previous subscription.
- The current fixture remains deterministic for unit, E2E, and visual tests.
- No backend or wallet behavior is introduced in this slice.

## Backend-aligned bridge (current follow-up)

The backend currently publishes low-level pool updates to Kafka; it does not
yet expose the token-level API/WebSocket gateway described by M2. The frontend
therefore implements the seam that can be tested now without coupling the UI to
pool reserves or tick math:

```text
MarketStreamClient
  subscribe(request, handlers)
    → snapshot / update / heartbeat / error events
    → Token Workspace view model
```

The temporary event envelope is intentionally small and transport-neutral:

- `snapshot`: complete token workspace data for the subscription.
- `update`: the next complete view-model snapshot in the fixture bridge; the
  backend may later send a smaller delta with the same sequence and timestamp
  guarantees.
- `heartbeat`: proves the subscription is alive without replacing market data.
- `error`: carries a stable code, message, and retryable flag.

Every event carries a token ID, monotonic sequence, and server timestamp. The
fixture client emits this shape from the existing deterministic stream. A
future M2 WebSocket client will implement the same `MarketStreamClient`
interface and map generated Protobuf/API types at this boundary.
