# Scripts

Foundry deploy scripts.

## DeployVaultFactory

Deploys `WaneVaultFactory` wired to an already-live `WanePolicy`. Per-owner
vaults are created from the factory at runtime, so only the factory is deployed
here. It reuses the existing registry + policy, so no new economy or genesis is
involved.

```bash
forge script script/DeployVaultFactory.s.sol:DeployVaultFactory \
  --rpc-url base \
  --broadcast \
  --verify
```

Environment:

| Var | Meaning |
|---|---|
| `PRIVATE_KEY` | deployer key |
| `POLICY` | `WanePolicy` address (defaults to the live Base mainnet policy `0x26deE4503C7f67356837ED41cE285026EF256667`) |

The script logs the deployed factory address, the wired policy, and the chain id.
