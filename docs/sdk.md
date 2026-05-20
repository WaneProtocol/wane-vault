# SDK Reference

`@wane/vault-sdk` is a thin viem client around the Wane vault factory and a
per-owner vault. Every outbound send routes through the vault's `execute()`,
which screens against the owner's policy before any value moves.

## Install

```bash
npm install @wane/vault-sdk viem
```

## Construct a client

```ts
import { createPublicClient, createWalletClient, http } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { base } from "viem/chains";
import { WaneVaultClient } from "@wane/vault-sdk";

const account = privateKeyToAccount(process.env.PRIVATE_KEY as `0x${string}`);
const publicClient = createPublicClient({ chain: base, transport: http() });
const walletClient = createWalletClient({ account, chain: base, transport: http() });

const wane = new WaneVaultClient({ publicClient, walletClient });
```

`walletClient` is optional. Without it the client supports read-only calls
(`predictVault`, `vaultOf`, `wouldAllow`); state-changing calls throw.

## Methods
