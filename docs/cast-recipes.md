# Cast Recipes

Drive the vault directly with `cast` (no SDK). All examples target Base mainnet;
set `PK` to the owner key and the addresses to your own.

```bash
export FACTORY=0x6640dd13F172c356f671d35ef76695792908e2a9
export RPC=base
```

## Predict and create

```bash
# deterministic vault address for an owner (works before creation)
cast call $FACTORY "predict(address)(address)" $OWNER --rpc-url $RPC

# create the caller's vault
cast send $FACTORY "createVault()(address)" --rpc-url $RPC --private-key $PK

# look up an already-created vault
cast call $FACTORY "vaultOf(address)(address)" $OWNER --rpc-url $RPC
```

## Fund the vault

```bash
# ETH: a plain transfer to the vault address
cast send $VAULT --value 0.1ether --rpc-url $RPC --private-key $PK

# ERC-20: an ordinary token transfer to the vault address
cast send $TOKEN "transfer(address,uint256)(bool)" $VAULT 100000000 \
  --rpc-url $RPC --private-key $PK
```
