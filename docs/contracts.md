# Contract Reference

Function-level reference for the two contracts this repo deploys: `WaneVault` and
`WaneVaultFactory`. The reused `WanePolicy` / `WaneRegistry` surfaces the vault
depends on are summarized at the end.

## WaneVault

Holds the owner's ETH and ERC-20 balances. Constructed with the owner address
and the policy address; both are immutable.

### State

| Member | Type | Notes |
|---|---|---|
| `owner` | `address` immutable | the sole driver |
| `policy` | `IWanePolicyView` immutable | the screen the vault reads |

### execute

```solidity
function execute(address target, uint256 value, bytes calldata data)
    external onlyOwner returns (bytes memory ret)
```

Screens `(target, value, data)` against the owner's policy, then performs a
low-level call forwarding `value`. Reverts with `Blocked(flagged, reason)` if the
screen fails, `NotOwner` if the caller is not the owner, `CallFailed` if the
inner call reverts. Emits `Screened` then `Executed`.
