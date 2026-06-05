# Tests

Foundry tests for the screening vault. Run with `forge test`.

`WaneVault.t.sol` wires the vault against the real policy and antibody registry,
seeds a drainer as a genesis antibody, funds the vault with ETH and tokens, then
checks the full screening surface:

| Test | Asserts |
|---|---|
| `test_PredictMatchesCreated` | factory `predict(owner)` equals the created vault, and `vaultOf` is set |
| `test_CleanEthSend` | a clean ETH send goes through |
| `test_DrainerEthBlocked` | an ETH send to a flagged address reverts, no ETH moves |
| `test_DrainerTokenBlocked` | an ERC-20 transfer to a flagged recipient reverts (recipient decoded from calldata) |
| `test_CleanTokenSend` | an ERC-20 transfer to a clean address goes through |
| `test_OutsiderCannotExecute` | a non-owner calling `execute` reverts with `NotOwner` |
| `test_OwnerWithdraw` | the owner can withdraw ETH and tokens back to themselves |
| `test_BatchRevertsOnDrainer` | a batch with one flagged action reverts entirely, clean action rolled back |
| `test_WouldAllow` | the free dry-run matches enforcement for clean and flagged targets |

The drainer-token test is the load-bearing one: a target-only screen would let a
token drain through because the call target is the clean token contract. The
vault decodes the real recipient from the transfer calldata and screens it too.
