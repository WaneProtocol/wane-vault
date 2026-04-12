# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-06-12

### Added

- `WaneVaultFactory` deployed on Base mainnet at
  `0x6640dd13F172c356f671d35ef76695792908e2a9`, wired to the live policy and
  antibody registry.
- `@wane/vault-sdk` (viem): `predictVault`, `createVault`, `send`, `sendToken`,
  `execute`, `executeBatch`, `withdrawETH`, `withdrawToken`, `wouldAllow`.
- Human-readable `reasonLabel` mapping for every policy reason code.

### Changed

- `wouldAllow` dry-run now mirrors enforcement exactly, including the decoded
  ERC-20 recipient check.

## [0.2.0] - 2026-05-20

### Added

- ERC-20 recipient decoding from calldata (`transfer` / `transferFrom` /
  `approve`), so a token drain to a flagged address is blocked even though the
  call target is the token contract.
- `executeBatch` with atomic revert on any flagged action in the batch.

### Changed

- Spend-cap amount for an ERC-20 movement is screened as `0` so caps denominated
  in native value do not misfire on token units; the address antibody check
  still runs on the decoded recipient.

## [0.1.0] - 2026-04-08

### Added

- Initial `WaneVault`: ETH + ERC-20 held in contract, owner-driven `execute()`
  screened against the owner's `WanePolicy` before any value moves.
- Owner `withdrawETH` / `withdrawToken` so funds are never trapped.
- CREATE2 `WaneVaultFactory` with `predict` and `createVault`.
- Foundry test suite: clean pass, ETH drainer block, ERC-20 drainer block,
  withdraw, onlyOwner, batch, deterministic predict.

[Unreleased]: https://github.com/WaneProtocol/wane-vault/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/WaneProtocol/wane-vault/releases/tag/v0.3.0
[0.2.0]: https://github.com/WaneProtocol/wane-vault/releases/tag/v0.2.0
[0.1.0]: https://github.com/WaneProtocol/wane-vault/releases/tag/v0.1.0
