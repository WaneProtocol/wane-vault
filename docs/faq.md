# FAQ

### Is the vault custodial?

No. The vault is owned by a single EOA and only that owner can drive it. The
contract has no admin, no upgrade path, and no code that sends funds anywhere
except where the owner directs (screened) or back to the owner (withdraw). It
can block an action but never divert or seize funds.

### How is this different from an EIP-7702 delegate guard?

A 7702 guard only runs when the wallet routes a call through its `execute()`. A
raw key-signed transaction from the same EOA skips the guard entirely. With the
vault, funds live in the contract, not the EOA, so there is no raw-signed path to
move them. The only exits are the screened `execute()` and the owner withdraw.

### What happens if the policy is wrong and blocks a legitimate send?

The owner withdraws the funds back to themselves (unscreened) and routes the
transfer manually. Funds are never trapped by a policy misconfiguration.

### Does screening cost gas?

Enforcement runs inside `execute()`, so it is part of the transaction you were
already sending. The `wouldAllow` dry-run is a free `view` call. The policy and
registry reads are `view`, so the marginal cost is small.

### Can the vault be drained by a malicious token contract?

The vault makes a low-level call to the target with the owner's calldata. A
malicious token cannot reach the vault's funds beyond what the call moves,
because the screen runs before the call and the call only forwards the `value`
the owner specified. Re-entrancy into `execute` still requires the owner as
`msg.sender`, which an external contract is not.

### What about a compromised owner key?

That is out of scope for the recipient screen: if an attacker holds the owner
key they can direct sends like the owner would. The owner can reduce blast
radius with per-tx and daily caps and a policy TTL in `WanePolicy`, but a stolen
key is not made harmless by the vault.

### Why are the policy, registry, token, and types in this repo?

So the test suite exercises the vault against the real screening path instead of
a stand-in. They are the same contracts already deployed on Base; the vault and
factory are what this repo deploys.

### One vault per owner, or one shared vault?

One per owner, at a deterministic CREATE2 address keyed by the owner address.
Compute it with `predict(owner)` before it exists.
