# Development

Local workflow for working on the contracts and the SDK.

## Prerequisites

- [Foundry](https://book.getfoundry.sh) (forge, cast). Install with
  `curl -L https://foundry.paradigm.xyz | bash` then `foundryup`.
- Node.js 20+ for the SDK and examples.
- Git with submodule support.

## Clone

```bash
git clone --recurse-submodules https://github.com/WaneProtocol/wane-vault
cd wane-vault
```

If you forgot `--recurse-submodules`:

```bash
git submodule update --init --recursive
```

The submodules are `forge-std` (v1.16.1) and `openzeppelin-contracts` (v5.6.1),
pinned in `.gitmodules`. They are not vendored into the repo; `lib/` is
gitignored.

## Build and test

```bash
make build      # forge build
make test       # forge test -vvv
make fmt-check  # forge fmt --check
make snapshot   # forge snapshot (gas baseline)
```

or call forge directly:

```bash
forge build
forge test
forge fmt
```

## SDK

```bash
make sdk        # cd sdk && npm install && npm run build
```

or:

```bash
cd sdk
npm install
npm run lint    # tsc --noEmit
npm run build   # tsc -> dist/
```

Keep `sdk/src/abi.ts` in sync with any change to the vault or factory function
surface, since the SDK encodes calldata against it.

## Layout

| Path | Contents |
|---|---|
| `src/` | Solidity contracts |
| `test/` | Foundry tests |
| `script/` | deploy scripts |
| `sdk/` | TypeScript SDK (`@wane/vault-sdk`) |
| `examples/` | runnable TS snippets |
| `docs/` | reference and guides |
| `lib/` | submodules (gitignored) |

## Conventions

- Conventional commits (`feat:`, `fix:`, `docs:`, `test:`, `chore:`).
- Run `forge fmt` before committing Solidity.
- Update `CHANGELOG.md` under `## [Unreleased]` for any user-visible change.
