# Glossary

Terms used across the Wane vault docs and code.

**Vault.** A `WaneVault` contract that holds one owner's ETH and ERC-20 balances
and screens every outbound action. One per owner.

**Owner.** The single EOA that controls a vault. The sole `msg.sender` allowed to
call `execute`, `executeBatch`, and the withdraw functions.

**Screen.** The pre-flight check the vault runs before an outbound action. It
reads the owner's policy and the antibody registry and returns allowed or
blocked. Fail-closed.

**Policy.** A `WanePolicy` entry: the per-owner protection scope (enabled, kill
switch, threat kinds, sensitivity, spend caps, expiry, allow / block lists,
selector and token scoping). The vault reads it via `evaluate` / `evaluateCall`.

**Antibody.** An on-chain memory of a threat in `WaneRegistry`. Once recorded and
enforceable, every reader is immune. Kinds: address, call pattern, bytecode,
semantic.

**Registry.** The `WaneRegistry` contract that stores antibodies. The policy
reads it; the vault reads the policy.

**Reason code.** The `uint8` a screen returns to explain a block. Codes match
`WanePolicy` and are listed in [`reason-codes.md`](./reason-codes.md).

**Decoded recipient.** The real destination of an ERC-20 movement, extracted from
`transfer` / `transferFrom` / `approve` calldata and screened in addition to the
call target.

**Predict.** The factory view that returns a vault's deterministic CREATE2
address before it is deployed, so it can be funded ahead of time.

**Genesis antibody.** A protocol-seeded antibody with zero stake that enforces
immediately, used to cold-start the registry against known threats.
