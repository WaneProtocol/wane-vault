# Threat Model

WaneVault defends the owner's funds against malicious outbound actions while
keeping the owner in sole control. This document states what it guarantees, what
it does not, and the attacks it is designed to stop.

## Trust assumptions

- The owner's key is honest-but-fallible: it may be tricked into signing a
  malicious action (drainer approval, transfer to an attacker), but it is not
  itself the attacker trying to steal from its own vault.
- The policy and antibody registry are deployed and behave as audited. The vault
  treats their view results as authoritative.

## Guarantees

1. **Block, never divert.** The contract has no code path that sends funds to an
   address of its own choosing. Outbound value only goes where `execute()` is
   told and the policy allows; withdrawals only go to the owner.
2. **Fail-closed screening.** If the policy returns not-allowed, `execute()`
   reverts with `Blocked(target, reason)` before any value moves. There is no
   "allow on error" branch.
3. **Real-recipient screening for tokens.** The vault decodes the recipient /
   spender from `transfer`, `transferFrom`, and `approve` calldata and screens
   it, so a token drain to a flagged address is blocked even though the call
   target is the (clean) token contract.
4. **Funds never trapped.** The owner can always `withdrawETH` / `withdrawToken`
   back to themselves, regardless of policy state.
5. **Owner-only control.** Every state-changing entry point is `onlyOwner`. An
   outsider calling `execute` reverts with `NotOwner`.

## Attacks stopped

- **ETH drain to a flagged address.** `execute(drainer, value, "")` reverts;
  no ETH moves. Covered by `test_DrainerEthBlocked`.
- **ERC-20 drain to a flagged recipient.** `execute(token, 0, transfer(drainer,
  amount))` reverts because the decoded recipient is screened. A target-only
  screen would miss this. Covered by `test_DrainerTokenBlocked`.
- **Batch laundering.** A batch that mixes a clean action with a flagged one
  reverts entirely; the clean action does not slip through. Covered by
  `test_BatchRevertsOnDrainer`.
- **Bypass via a raw key-signed tx.** Funds live in the vault, not the EOA, so
  there is no way to move vault funds without going through the screened
  `execute()`. This is the key advantage over a 7702 delegate guard, which only
  runs when the wallet chooses to route through it.

## Out of scope

- A compromised owner key acting against its own vault. Wane screens recipients
  and patterns, not the owner's intent. Spend caps and policy TTL in WanePolicy
  reduce blast radius but do not make a stolen key harmless.
- Bugs in the reused policy / registry contracts. Those have their own audits
  and disclosure path.
- Off-chain phishing that convinces a user to `withdraw` and then send manually.
  The vault protects the screened path; once funds are withdrawn they are an
  ordinary EOA balance again.
