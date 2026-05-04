# Comparison

Where the Wane vault sits relative to a plain EOA and an EIP-7702 delegate guard.

## Plain EOA

A normal externally owned account holds funds at the key. Any transaction the key
signs moves funds, with no screen. Phishing a single signature drains the wallet.

- Screening: none
- Bypass: not applicable, there is nothing to bypass
- Funds location: at the key

## EIP-7702 delegate guard

7702 lets an EOA temporarily run contract code. A guard can screen calls that the
wallet routes through its `execute()` entry point.

- Screening: only on calls routed through the delegate `execute()`
- Bypass: a raw key-signed transaction from the same EOA skips the delegate and
  moves funds directly
- Funds location: at the key

The bypass is the core limitation: because the funds still live at the key, any
ordinary signed transfer ignores the guard. The guard helps for wallet-initiated
flows but is not a hard custody boundary.

## Wane vault

Funds live in the vault contract, not at the key. The only ways out are the
screened `execute()` and the owner-only `withdraw*()`.

- Screening: every outbound action through `execute()` / `executeBatch()`
- Bypass: none for vault funds, there is no raw-signed path to move them
- Funds location: in the vault contract

## Side by side

| Property | EOA | 7702 guard | Wane vault |
|---|---|---|---|
| Funds held in a screened boundary | no | no | yes |
| Outbound actions screened | no | routed calls only | always |
| Raw-signed bypass possible | n/a | yes | no |
| Real ERC-20 recipient screened | no | depends on guard | yes |
| Owner can always recover funds | yes | yes | yes (withdraw) |
| Contract can seize / divert funds | n/a | no | no |

## When to use which

- Use the vault when you want a hard custody boundary with screening that a raw
  signature cannot skip, for example an agent or session wallet that holds a
  working balance.
- A 7702 guard is lighter weight and fits when funds must stay at the EOA and you
  accept that direct signed transfers are unscreened.
- A plain EOA is fine for funds you actively watch and do not want any screening
  overhead on.
