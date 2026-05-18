# Integration Guide

This guide walks an integrator from zero to a funded, screened vault in
production, with the gotchas called out.

## 1. Pick the network and addresses

Everything here targets Base mainnet (chain `8453`). The factory and the reused
policy / registry are listed in [`deployments.md`](./deployments.md). The SDK
defaults to the live factory, so you do not need to pass an address unless you
are running against a fork or a test deployment.

## 2. Compute the vault address first

A vault has a deterministic CREATE2 address derived from the owner address, so
you can compute it and fund it before it is deployed. This matters for flows
where you want to hand a user a deposit address immediately:

```ts
const vault = await wane.predictVault(owner);
// safe to display / fund now, even though createVault() has not run
```

Deposits are plain transfers to `vault`. The vault's `receive()` accepts ETH;
ERC-20 deposits are ordinary token transfers to the address. Wane screens only
outbound actions, so inbound value always lands.

## 3. Create the vault

```ts
const existing = await wane.vaultOf(owner);
if (existing === "0x0000000000000000000000000000000000000000") {
  await wane.createVault();
}
```

`createVault` is idempotent at the protocol level: a second call for an owner
that already has a vault reverts with `VaultExists`. Guard with `vaultOf` to
avoid the wasted gas.

## 4. Configure the owner's policy

The vault reads the owner's `WanePolicy` entry. If the owner has not enrolled,
the policy returns allowed for everything (the vault is then a plain held-funds
wallet with no screening). To turn protection on, the owner enrolls once:

```bash
cast send $POLICY \
  "enroll(address,uint8,uint32,uint128,uint128,uint40)" \
  $OWNER 15 0 0 0 0 \
  --rpc-url base --private-key $PK
```

`blockKinds = 15` is `K_ALL` (address, call pattern, bytecode, semantic). The
remaining args set sensitivity, per-tx cap, daily cap, and expiry. See the
policy contract for the full scope surface.

## 5. Send through the screen

Always dry-run first if you want to show the user a result before they spend gas:

```ts
const check = await wane.wouldAllow(vault, recipient, value);
if (!check.allowed) {
  // surface check.label, e.g. "antibody match"
  return;
}
await wane.send(vault, recipient, value);
```

For ERC-20, use `sendToken` / `wouldAllowToken`. The vault decodes the recipient
from the transfer calldata and screens it, so you do not need to screen the
recipient yourself.

## 6. Handle a blocked send

On enforcement the vault reverts with `Blocked(target, reason)`. With viem you
can decode it from the revert data:

```ts
import { decodeVaultError } from "@wane/vault-sdk";
