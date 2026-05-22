# Security Notes

Engineering notes on the choices that keep the vault safe. This complements the
formal [threat model](./threat-model.md).

## Fail-closed, not fail-open

`_screen` reverts the moment the policy returns not-allowed. There is no path
that logs a failure and proceeds, and no try/catch that swallows a policy revert
into an allow. If the policy view itself reverts, the whole `execute` reverts,
which is the safe direction: a broken screen blocks rather than waves traffic
through.

## Screening the decoded recipient

A naive screen checks only the call target. For an ERC-20 transfer the call
target is the token contract, which is clean, so a target-only screen would let a
drain to a flagged recipient through. The vault decodes the recipient from the
calldata of `transfer`, `transferFrom`, and `approve` and screens it with amount
`0`. Amount `0` is deliberate: spend caps are denominated in native value, so
passing the token amount would let token units misfire the native caps. The
address antibody check still runs on the decoded recipient.

## No divert path

The contract is auditable for a single property: it never sends value to an
address of its own choosing. `execute` forwards to the owner-supplied `target`.
`withdraw*` sends only to `owner`. There is no other `call`, `transfer`, or
`send` of value. This is what lets us say the contract can block but never
seize.

## Batch atomicity

`executeBatch` screens and runs actions in a loop and reverts on the first
failure. Because EVM reverts roll back all state, a batch is all-or-nothing: an
attacker cannot pair a clean action with a flagged one and have the clean one
commit.

## Immutable wiring

`owner` and `policy` are immutable, set at construction by the factory. There is
no setter to repoint the policy at a permissive contract or to change the owner.
A compromised factory cannot retroactively alter an already-deployed vault.

## Value cast safety

`_evaluate` casts `value` down to `uint128` for the policy call, saturating at
`type(uint128).max` rather than truncating. A value above `uint128` max is not
realistic for a transfer, and saturation means a huge value is screened as the
maximum (the most restrictive interpretation for spend caps), not as a small
wrapped number.

## What we do not claim

The vault does not defend against a compromised owner key, bugs in the reused
policy / registry, or off-chain phishing that ends in a manual withdraw-then-send.
Those are stated plainly in the threat model so integrators size their own
controls accordingly.
