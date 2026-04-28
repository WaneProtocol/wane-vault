# Architecture

WaneVault is a non-custodial screening smart wallet. It holds the owner's ETH
and ERC-20 balances and gates every outbound action behind a policy screen. This
document walks the data flow and the contract surfaces.

## Components

- **WaneVault**: holds funds, owned by a single EOA. The owner drives it through
  `execute()` / `executeBatch()`; the contract screens each action and runs it
  only if allowed. `withdrawETH` / `withdrawToken` return funds to the owner and
  are intentionally unscreened (returning your own funds is always safe).
- **WaneVaultFactory**: deploys one vault per owner at a deterministic CREATE2
  address. `predict(owner)` returns that address before the vault exists, so a
  client can fund it ahead of deployment.
- **WanePolicy** (reused, already deployed): the per-owner protection scope. The
  vault calls `evaluate` and `evaluateCall` (both pure view) to decide if an
  action is allowed.
- **WaneRegistry** (reused, already deployed): the antibody registry the policy
  reads. An antibody is an on-chain memory of a threat; once recorded it makes
  every reader immune.

## Outbound action flow

1. The owner calls `execute(target, value, data)` on their vault.
2. `_screen` computes the screen via `_evaluate`:
   - It screens the call `target` itself. If `data` carries a 4-byte selector it
     uses `evaluateCall(owner, target, selector, amount)`, otherwise
     `evaluate(owner, target, amount)`.
   - For an ERC-20 movement it decodes the REAL recipient from calldata
     (`transfer` / `transferFrom` / `approve`) and screens that address too, with
     amount `0` so native-denominated spend caps do not misfire on token units.
3. If either screen returns not-allowed, `execute()` reverts with
   `Blocked(flagged, reason)` before any value moves.
4. If allowed, the vault performs the low-level call and forwards `value`.

```
execute(target, value, data)
        |
        v
   _screen ----> _evaluate
                    |  evaluateCall / evaluate (target)
                    |  evaluate (decoded ERC-20 recipient, amount = 0)
                    v
            allowed? --- no ---> revert Blocked(flagged, reason)
                    |
                   yes
                    v
            target.call{value}(data)
```
