## Summary

<!-- 1 to 3 bullets describing the change and the problem it solves. -->

## Changes

- [ ] Contract change(s)
- [ ] SDK change(s)
- [ ] Tests added or updated
- [ ] Docs updated (CHANGELOG, README, docs/*)

## Verification

<!--
How did you test this? Foundry test output, transaction hashes, or before/after
behavior.
-->

```bash
forge test
cd sdk && npm run lint && npm run build
```

## Checklist

- [ ] Followed conventional commit format
- [ ] No change to fund-custody or screening behavior (or, if so, coordinated via SECURITY.md)
- [ ] SDK ABI matches the contract surface it calls
