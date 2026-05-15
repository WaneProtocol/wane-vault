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
