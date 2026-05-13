# Gas

Measured with `forge test --gas-report` against the test suite in
`test/WaneVault.t.sol` (solc 0.8.27, `via_ir`, optimizer 200 runs). Numbers are
indicative and depend on calldata size and how much policy state a screen reads.

## WaneVault

| Function | Min | Avg | Median | Max |
|---|---|---|---|---|
| `execute` | 22,441 | 72,890 | 83,285 | 113,510 |
| `executeBatch` (2 actions) | 111,754 | 111,754 | 111,754 | 111,754 |
| `withdrawETH` | 29,870 | 29,870 | 29,870 | 29,870 |
| `withdrawToken` | 55,978 | 55,978 | 55,978 | 55,978 |

`execute` spans a wide range because the cost depends on the action: a blocked
send reverts early and cheap, a clean ETH send is mid-range, and a clean ERC-20
send pays for the token transfer plus the decoded-recipient screen.

## WaneVaultFactory

| Function | Gas |
|---|---|
| `createVault` | 710,489 |
| `predict` | 3,245 |

`createVault` pays for the full CREATE2 deployment of a vault. `predict` is a
cheap `view` and is free when called off-chain through `eth_call`.

## Reproduce

```bash
forge test --gas-report
```

The `wouldAllow` dry-run and `predict` are `view` calls, so off-chain they cost
nothing. Prefer `wouldAllow` to check a send before paying for `execute`.
