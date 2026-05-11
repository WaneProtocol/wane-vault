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
