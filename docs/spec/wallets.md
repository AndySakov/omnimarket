# Wallets & Custody

## The problem a terminal has to solve

A trading terminal needs to **sign transactions fast and without the user present**: instant buys, and limit / take-profit / stop-loss / copy-trade orders that fire at 3am. That requirement is what shapes wallet architecture in this category. Asking the user to sign every trade in their browser wallet is a non-starter.

So the question isn't "custodial or not"; it's **who can make the key sign, under what rules, and how fast.**

## The four models

| Model | How it works | Signing speed | Automation | Who can steal funds | Used by |
|---|---|---|---|---|---|
| **1. Server-held keys** | Backend generates keys, encrypts them at rest (AES / cloud KMS), and decrypts in memory to sign | Fastest (in-process) | Full | The operator, or anyone who breaches the operator | Early Telegram bots (Maestro, Banana Gun, Unibot era) |
| **2. Embedded wallets in secure enclaves** | Keys are generated and used only inside hardware enclaves (TEE) run by a vendor. The user authenticates to unlock signing. The backend gets **delegated, policy-limited** signing rights | Fast (one network call to the vendor) | Full, within policy | Much harder: needs the user's auth or a policy-allowed action | Trojan (Privy, verified), Axiom (Turnkey) |
| **3. MPC wallets** | The key is split into shares held by different parties, and signatures are computed jointly without ever reassembling the key | Slower (multiple network round trips) | Possible | Needs several parties to collude | Institutional custody (Fireblocks) |
| **4. Self-custody + session keys** | User keeps their own wallet (EOA or smart account) and grants a scoped, expiring "session key" to the backend via ERC-4337 or EIP-7702 | Fast once the session exists | Within session scope | Only within the session's limits | Emerging; not yet standard for terminals |

### What "enclave" means here

The vendor runs signing inside AWS Nitro Enclaves: isolated VMs with no disk, no shell, and no outside network except through a narrow channel. Code running inside can be proven by attestation (a signed hash of what's running). Neither the vendor's engineers nor we can read a raw key. Privy splits each key into shares and reassembles it only in enclave memory at signing time. Turnkey keeps keys encrypted and decrypts them only inside the enclave.

### Why the industry moved from model 1 to model 2

Model 1 makes the operator's servers a single target holding every user's keys. The Banana Gun incident in Sept 2024 (~$3M drained from 11 wallets via a messaging-flow bug) is the canonical example. Model 2 keeps model 1's speed and automation but removes the "one breach empties everyone" failure. It also lets the product honestly say "non-custodial, exportable keys", which users now expect.

## The two vendors

| | Privy | Turnkey |
|---|---|---|
| Shape | Bundled: login + embedded wallets + policies + funding, one SDK | Lower-level building block: keys + signing + policy engine; you build the product around it |
| Key storage | Key split into shares; assembled only in enclave memory | Encrypted keys, decrypted only in the enclave |
| Verifiability | Enclave attestation | Attestation plus published boot/app proofs, so outsiders can verify what ran |
| Automation | Server-side signing via authorization keys + policies (method rules, contract allowlists, time windows) | Policy engine (allow/deny, multi-approver) + scoped, time-limited sessions |
| Pricing | Per monthly active user | Per signature |
| Ownership | Acquired by Stripe (2025) | Independent |
| Who uses it in this space | Trojan (verified) | Axiom |

## Decision (D4)

**Privy**, used behind our own `Signer` boundary, with two implementations:

1. **Privy signer** for the real product path and demo. It's the stack Trojan reportedly runs, and it also gives us login, so we don't build auth.
2. **Local signer** (keys in an encrypted local keystore) for load tests and mainnet-fork runs, where we can't hammer a vendor API at thousands of signatures per second or point it at a fork.

Turnkey is the stronger pick if we wanted to showcase the policy engine itself or cared about per-signature cost at scale. The `Signer` boundary makes switching cheap either way.

## Engineering problems this creates (the interesting part)

- **Signing latency is now a network hop.** *Resolved by D42:* users sign **intents**, not transactions, and our executor wallets submit them. Trigger intents are signed when the order is created, so a firing needs no vendor call at all.
- **Nonce ownership.** *Resolved by D32/D42:* user intents use Permit2's unordered nonces (no gaps possible); only our executor wallets have sequential nonces, owned by the execution service.
- **Policy as a safety net.** *Tightened by D57:* when the user is present, their own session signs; server signing is only for absent-user flows and is limited by Privy policy to our router's EIP-712 domain, per-intent and daily caps, and a minimum-output floor. Plain transfers only as user-initiated withdrawals with MFA.
- **Multi-wallet per user.** Trojan allows up to 10 active wallets per user, plus grouping and moving funds between them. This is cheap to support and realistic.

## Open questions

- Privy supports server-side signing on any EVM chain and lists MegaETH explicitly (verified).
- ~~Own router vs DEX routers~~ → our own immutable router executing signed intents (D26, D42, D58).
- Smart accounts / EIP-7702: reviewed in D47. Permit2 intents in phase 1; a 7702 delegate is a phase 2 candidate.

## Sources

- [Trojan wallet management docs](https://docs.trojan.com/trading-on-trojan/wallet-management)
- [Trojan review mentioning Privy](https://medium.com/@gemQueenx/trojan-solana-trading-bot-review-2026-web-terminal-and-telegram-bot-47bef50956cc)
- [Turnkey vs Privy — Den](https://www.onchainden.com/blog/turnkey-vs-privy)
- [Axiom uses Turnkey — Axiom guide](https://medium.com/@geggonen/axiom-trade-complete-guide-938e97c0a3a6)
- [Telegram bot custody & Banana Gun incident — TechBullion](https://techbullion.com/5-telegram-trading-bots-compared-on-security-audits-key-custody-and-hack-history/)
- [Fireblocks embedded wallet comparison](https://www.fireblocks.com/report/compare-embedded-wallet-infrastructure)
