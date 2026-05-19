# Reason Codes

When the vault blocks an action it reverts with `Blocked(target, reason)`, where
`reason` is a `WanePolicy` `R_*` code. The free `wouldAllow` dry-run returns the
same code without reverting. The SDK exposes `REASON` and `reasonLabel` for
mapping the numeric code to text.

| Code | Name | Meaning |
|---|---|---|
| 0 | `OK` | allowed |
| 1 | `BLOCKLIST` | the owner's per-agent blocklist names this target |
| 2 | `ANTIBODY` | an enforceable antibody in the registry matches the target, recipient, codehash, or call pattern |
| 3 | `PER_TX_CAP` | the action exceeds the per-transaction value cap |
| 4 | `DAILY_CAP` | the action would exceed the rolling daily value cap |
| 5 | `PAUSED` | the agent kill switch or the global pause is on |
| 6 | `GLOBAL_DENY` | the curated global recipient denylist names this target |
| 7 | `EXPIRED` | the policy TTL has elapsed |
| 8 | `SELECTOR` | the called selector is not on the owner's selector allowlist (when scoped) |
| 9 | `TOKEN` | the token is not on the owner's token allowlist (when scoped) |

## Which target is flagged

`Blocked(target, reason)` reports the specific address that failed. For an ERC-20
movement this can be the decoded recipient rather than the token contract: the
vault screens both, and reports whichever one tripped. That is how a transfer to
a flagged recipient is blocked even though the call target (the token) is clean.

## Reading without sending

Always cheaper to check first:

```ts
const check = await wane.wouldAllow(vault, recipient, value);
if (!check.allowed) console.log("blocked:", check.label);
```
