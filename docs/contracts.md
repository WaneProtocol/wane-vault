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

### executeBatch

```solidity
function executeBatch(
    address[] calldata targets,
    uint256[] calldata values,
    bytes[] calldata datas
) external onlyOwner returns (bytes[] memory rets)
```

Screens and runs each action in order. Any flagged action reverts the entire
batch, so a clean action cannot slip through alongside a flagged one. Reverts
with `BatchLengthMismatch` if the arrays differ in length.

### withdrawETH / withdrawToken

```solidity
function withdrawETH(uint256 amount) external onlyOwner
function withdrawToken(address token, uint256 amount) external onlyOwner
```

Return funds to the owner. Unscreened by design: returning your own funds to
yourself is always safe, and this is the escape hatch if a policy is
misconfigured. Emits `Withdrawn`.

### wouldAllow

```solidity
function wouldAllow(address target, uint256 value, bytes calldata data)
    external view returns (bool allowed, uint8 reason)
```

Free dry-run of the screen. Mirrors enforcement exactly, including the decoded
ERC-20 recipient check.

### receive

```solidity
receive() external payable
```

Accepts inbound ETH. Deposits are plain transfers to the vault address. Only
outbound actions are screened, so inbound value always lands.

### Errors and events
