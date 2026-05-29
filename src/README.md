# Contracts

Solidity sources for the Wane screening vault. Built with Foundry, solc 0.8.27,
`via_ir`, evm version `cancun`.

| File | Role |
|---|---|
| `WaneVault.sol` | Holds ETH + ERC-20, owner-driven, screens every outbound action before value moves. |
| `WaneVaultFactory.sol` | CREATE2 factory: one deterministic vault per owner, `predict` + `createVault`. |
| `IWanePolicy.sol` | The policy view surface the vault calls (`evaluate`, `evaluateCall`) plus a minimal ERC-20 interface. |
| `WanePolicy.sol` | Per-owner protection scope and the `R_*` reason codes. Reused, already deployed. |
| `WaneRegistry.sol` | Antibody registry the policy reads. Reused, already deployed. |
| `WaneToken.sol` | `$WANE`, the stake / reward currency the registry uses. |
| `WaneTypes.sol` | Shared threat / antibody types. |

`WaneVault` and `WaneVaultFactory` are the contracts this repo deploys. The
policy, registry, token, and types are included so the test suite exercises the
vault against the real screening path rather than a stand-in.

## Invariants

- The vault never sends funds to an address it chooses. Outbound value only
  flows through `execute()` (screened) or `withdraw*()` (owner-only, to owner).
- Screening is fail-closed: a flagged target reverts with `Blocked` before the
  low-level call.
- The decoded ERC-20 recipient is screened in addition to the call target.
