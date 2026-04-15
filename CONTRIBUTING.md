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
