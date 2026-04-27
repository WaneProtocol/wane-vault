# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| 0.3.x | yes |
| 0.2.x | security fixes only |
| < 0.2 | no |

## Reporting a Vulnerability

The vault holds user funds, so we take reports seriously. If you believe you
have found a vulnerability in the vault, the factory, the policy surface, or
the SDK, report it privately by emailing `security@wane.network` with:

- A short description of the issue
- A minimal reproduction (a Foundry test case, transaction hashes, or logs)
- Your assessment of the impact
- Whether you intend to disclose publicly and, if so, on what timeline

We will acknowledge your report within 72 hours and provide a disclosure
timeline that does not put users at risk. Please do not open a public GitHub
issue for security reports.

## Scope

In scope:

- `WaneVault` and `WaneVaultFactory` (fund custody, screening, CREATE2)
- The policy view surface `WaneVault` calls (`evaluate`, `evaluateCall`)
- `@wane/vault-sdk` calldata construction and address derivation

Out of scope:

- Front-end (wane.network) issues unrelated to wallet interactions
- Issues that require physical access to a maintainer's device
- Social engineering of community members
- Already-known issues tracked in our internal queue

## Design Guarantees

- The vault can only block, never divert. The only exits are `execute()`
  (screened) and `withdraw*()` (owner-only, back to the owner), so funds are
  never trapped and never redirectable by the contract.
- Screening is fail-closed: if the policy flags a target, `execute()` reverts
  with `Blocked(target, reason)` before any value moves.
- The real ERC-20 recipient is decoded from calldata and screened, so a token
  drain to a flagged address is caught even though the call target is the token.

## Coordinated Disclosure

We follow a 90-day coordinated disclosure window by default and may extend it
for critical issues that require user-side action such as a migration.
