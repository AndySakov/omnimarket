# Security

**Status:** Draft. Decisions: D26, D42, D52, D55–D58.

## Assets, most valuable first

| Asset | What it can do if stolen, after controls |
|---|---|
| Server Privy authorization key | Sign small, capped intents to our router only, for absent-user flows; visible to anomaly alerts |
| Router contract | Immutable; only executes signed intents from listed submitters; fuzzed, analysed, audited |
| Executor keys | Spend their own gas float; can only execute intents users signed, on the signed terms |
| Treasury | Safe multisig per chain; no hot key reaches it |
| User accounts | MFA on withdrawals and key export |
| Infrastructure credentials | Short-lived via Workload Identity and OIDC; no long-lived keys in CI |

## Controls (D57)

- **Signing:** the user's own Privy session signs intents when they're present (manual trades, creating/editing orders). Server signing only for absent flows (copy trades, auto-armed TP/SL, background approve/wrap), under Privy policy: router EIP-712 domain only, per-intent cap, per-user daily cap, minimum-output floor vs quote.
- **Intents name their submitter** (D58): our executor set or the user. Leaked intents are useless to anyone else; users can always self-submit.
- **Router:** Foundry fuzz + invariant tests, Slither, Aderyn, verified source, audit contest / bug bounty before real user funds. No owner, no pause.
- **Executors:** keys encrypted with Cloud KMS at rest, in memory at runtime; small gas float; capped auto top-up; rotation. Fees and refunds go straight to the treasury.
- **Treasury:** Safe multisig per chain.
- **Users:** Privy auth, MFA for withdrawals and key export, rate limits, D39 order limits.
- **Infrastructure:** private GKE, Workload Identity, least-privilege IAM, network policies, Secret Manager, CI → GCP via OIDC.
- **Supply chain:** `cargo-audit`, `cargo-deny`, Dependabot, pinned dependencies, cosign-signed images + SBOM, reproducible builds, secret scanning with push protection, branch protection.
- **Privacy:** pseudonymous telemetry; per-user encryption keys; crypto-shredding on deletion (D52).

## Incident response

Brake levels and automatic breakers from D56; runbooks per scenario (key compromise, router bug, pricing bug, provider outage, executor drain); every action lands in the tamper-evident audit log (D55).

## Still to measure

Browser signing latency to Privy (decides whether clicks use user-session or server signing, D57).
