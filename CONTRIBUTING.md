# Contributing to Wane Vault

Thanks for considering a contribution. This is a short, opinionated guide that
gets you from a clean checkout to a merged PR with minimal friction.

## Ground rules

- One concern per PR. Mixed PRs are hard to review and slow to merge.
- Solidity tests live in `test/` next to the contracts they exercise. SDK code
  ships with a typecheck (`npm run lint`) that must pass.
- New public functions require a short NatSpec comment.
- We follow [Conventional Commits](https://www.conventionalcommits.org)
  (`feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`).

## Local setup

```bash
git clone --recurse-submodules https://github.com/WaneProtocol/wane-vault
cd wane-vault

# contracts
forge build
forge test

# SDK
cd sdk && npm install && npm run build
```

You need:

- [Foundry](https://book.getfoundry.sh) (forge, cast)
- Solidity 0.8.27 (pinned in `foundry.toml`)
- Node.js 20+ for the SDK and examples

## Before submitting

1. `forge fmt` and `forge test` pass locally.
2. `cd sdk && npm run lint && npm run build` pass.
3. Update `CHANGELOG.md` under `## [Unreleased]`.
4. If you touched the vault or factory ABI, update `sdk/src/abi.ts` to match.

## Reporting bugs

Open an issue with the Bug Report template. On-chain reproductions are worth
their weight: include the transaction hash, expected vs actual behavior, and
the commit hash you tested against.

## Proposing features

Open a Feature Request issue first. Changes to the screening contract surface
need discussion before code lands, since the vault holds user funds.

## Security

If you find a vulnerability, follow the disclosure path in
[`SECURITY.md`](./SECURITY.md). Do not open a public issue.

## Code of conduct

This project follows the
[Contributor Covenant 2.1](https://www.contributor-covenant.org/version/2/1/code_of_conduct/).
See [`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md).
