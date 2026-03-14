# @wane/vault-sdk

TypeScript SDK for the Wane screening vault on EVM, built on [viem](https://viem.sh).
Create a vault, predict its address, run screened sends, and withdraw, all
against the live Base mainnet factory.

## Install

```bash
npm install @wane/vault-sdk viem
```

## Use

```ts
import { createPublicClient, createWalletClient, http, parseEther } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { base } from "viem/chains";
import { WaneVaultClient } from "@wane/vault-sdk";

const account = privateKeyToAccount(process.env.PRIVATE_KEY as `0x${string}`);
const publicClient = createPublicClient({ chain: base, transport: http() });
const walletClient = createWalletClient({ account, chain: base, transport: http() });

const wane = new WaneVaultClient({ publicClient, walletClient });

const vault = await wane.predictVault(account.address); // deterministic address
await wane.createVault();                                // deploy it
await wane.send(vault, "0xRecipient...", parseEther("0.1")); // screened send
```

The full method table and reason-code reference are in
[`../docs/sdk.md`](../docs/sdk.md).

## Deployments

Base mainnet (chain `8453`):

- WaneVaultFactory: `0x6640dd13F172c356f671d35ef76695792908e2a9`
- WanePolicy (reused): `0x26deE4503C7f67356837ED41cE285026EF256667`
- WaneRegistry (reused): `0x027F371fB139A57EcD2A2E175d30157eEA1C56de`

## License

MIT
