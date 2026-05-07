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

```solidity
error NotOwner();
error Blocked(address target, uint8 reason);
error CallFailed();
error BatchLengthMismatch();

event Screened(address indexed target, uint256 value, bool allowed, uint8 reason);
event Executed(address indexed target, uint256 value, bytes4 selector);
event Withdrawn(address indexed token, uint256 amount);
```

## WaneVaultFactory

Deploys one `WaneVault` per owner at a deterministic CREATE2 address.

### createVault / createVaultFor

```solidity
function createVault() external returns (address vault)
function createVaultFor(address owner) external returns (address vault)
```

Deploy the caller's vault, or a vault for a named owner. Reverts with
`VaultExists` if the owner already has one. Records `vaultOf[owner]` and emits
`VaultCreated(owner, vault)`.

### predict

```solidity
function predict(address owner) external view returns (address)
```

The deterministic vault address for `owner`, whether or not it exists yet. The
salt is the owner address, so each owner maps to exactly one vault and the
address is stable across chains where the factory is deployed at the same
address.

## Policy surface the vault reads

```solidity
function evaluate(address agent, address target, uint128 amount)
    external view returns (bool allowed, uint8 reason);
function evaluateCall(address agent, address target, bytes4 selector, uint128 amount)
    external view returns (bool allowed, uint8 reason);
```

Both are pure `view`. The vault calls `evaluateCall` when the calldata carries a
4-byte selector, otherwise `evaluate`. For ERC-20 movements it additionally calls
`evaluate` on the decoded recipient with amount `0`. Reason codes are documented
in [`reason-codes.md`](./reason-codes.md).
