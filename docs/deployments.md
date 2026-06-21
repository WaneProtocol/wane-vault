# Deployments

All addresses below are live on Base mainnet (chain `8453`) and verifiable
on-chain. The factory mints per-owner vaults that reuse the already-deployed
policy and antibody registry, so deploying the factory does not create a new
economy or genesis.

| Contract | Address | Explorer |
|---|---|---|
| WaneVaultFactory | `0x571Ac11310fb5d69D660C30f696a81e097Db8586` | [BaseScan](https://basescan.org/address/0x571Ac11310fb5d69D660C30f696a81e097Db8586) |
| WanePolicy (reused) | `0x26deE4503C7f67356837ED41cE285026EF256667` | [BaseScan](https://basescan.org/address/0x26deE4503C7f67356837ED41cE285026EF256667) |
| WaneRegistry (reused) | `0x027F371fB139A57EcD2A2E175d30157eEA1C56de` | [BaseScan](https://basescan.org/address/0x027F371fB139A57EcD2A2E175d30157eEA1C56de) |

## Per-owner vaults

There is no single vault address. Each owner gets exactly one vault at a
deterministic CREATE2 address derived from the owner address. Compute it before
the vault exists:

```bash
cast call 0x571Ac11310fb5d69D660C30f696a81e097Db8586 \
  "predict(address)(address)" $OWNER --rpc-url base
```

or with the SDK:

```ts
const vault = await wane.predictVault(owner);
```

Create the vault when ready:

```bash
cast send 0x571Ac11310fb5d69D660C30f696a81e097Db8586 \
  "createVault()(address)" --rpc-url base --private-key $PK
```

The created address equals the predicted one, and the factory records it in
`vaultOf(owner)` and emits `VaultCreated(owner, vault)`.
