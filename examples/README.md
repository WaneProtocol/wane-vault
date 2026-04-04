# Examples

Runnable TypeScript snippets against the live Base mainnet factory. They use the
SDK from `../sdk`, so build it first.

```bash
cd ../sdk && npm install && npm run build && cd ../examples
npm install -g tsx   # or: npx tsx <file>
```

Set the environment, then run:

```bash
export PRIVATE_KEY=0x...        # owner key (signs, drives the vault)
export RECIPIENT=0x...          # destination to screen + send to
export TOKEN=0x...              # ERC-20 for the token example

tsx create-and-send.ts          # create a vault, dry-run, send ETH if allowed
tsx screened-token-send.ts      # screened ERC-20 send (recipient decoded on-chain)
```

| File | What it shows |
|---|---|
| `create-and-send.ts` | predict + create a vault, free `wouldAllow` dry-run, screened ETH send |
| `screened-token-send.ts` | screened ERC-20 send where the real recipient is decoded from calldata |

Both print the resulting transaction hash. A flagged recipient causes the send
to revert with `Blocked(target, reason)` on-chain; the dry-run reports the same
decision for free before you spend gas.
