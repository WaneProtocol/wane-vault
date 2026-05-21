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

| Method | Returns | Notes |
|---|---|---|
| `predictVault(owner)` | `Address` | deterministic CREATE2 address, even before creation |
| `vaultOf(owner)` | `Address` | created vault, or zero address |
| `createVault()` | `Hash` | deploys the connected account's vault |
| `createVaultFor(owner)` | `Hash` | deploys a vault owned by `owner` |
| `execute(vault, target, value, data?)` | `Hash` | screened raw action |
| `send(vault, to, value)` | `Hash` | screened native ETH send |
| `sendToken(vault, token, to, amount)` | `Hash` | screened ERC-20 send, recipient decoded on-chain |
| `executeBatch(vault, targets, values, datas)` | `Hash` | atomic, reverts on any flagged action |
| `withdrawETH(vault, amount)` | `Hash` | owner-only, back to owner |
| `withdrawToken(vault, token, amount)` | `Hash` | owner-only, back to owner |
| `wouldAllow(vault, target, value, data?)` | `ScreenResult` | free dry-run |
| `wouldAllowToken(vault, token, to, amount)` | `ScreenResult` | free dry-run for a token send |

## Reason codes

A blocked action reverts with `Blocked(target, reason)` on-chain. The dry-run
helpers return a `ScreenResult` with the numeric `reason` and a `label`:

```ts
const check = await wane.wouldAllow(vault, recipient, 1n);
// { allowed: false, reason: 2, label: "antibody match" }
```

`REASON` and `reasonLabel` are exported for mapping codes to text. The codes
match `WanePolicy`: `OK`, `BLOCKLIST`, `ANTIBODY`, `PER_TX_CAP`, `DAILY_CAP`,
`PAUSED`, `GLOBAL_DENY`, `EXPIRED`, `SELECTOR`, `TOKEN`.
