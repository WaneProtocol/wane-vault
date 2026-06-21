# Cast Recipes

Drive the vault directly with `cast` (no SDK). All examples target Base mainnet;
set `PK` to the owner key and the addresses to your own.

```bash
export FACTORY=0x571Ac11310fb5d69D660C30f696a81e097Db8586
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

## Screened sends

```bash
# native ETH send through the screen
cast send $VAULT "execute(address,uint256,bytes)" $TO 100000000000000000 0x \
  --rpc-url $RPC --private-key $PK

# ERC-20 send: build the transfer calldata, pass it as data with value 0
DATA=$(cast calldata "transfer(address,uint256)" $TO 100000000)
cast send $VAULT "execute(address,uint256,bytes)" $TOKEN 0 $DATA \
  --rpc-url $RPC --private-key $PK
```

## Free dry-run

```bash
# would this send be allowed? returns (bool allowed, uint8 reason)
cast call $VAULT "wouldAllow(address,uint256,bytes)(bool,uint8)" \
  $TO 100000000000000000 0x --rpc-url $RPC
```

## Withdraw

```bash
cast send $VAULT "withdrawETH(uint256)" 50000000000000000 \
  --rpc-url $RPC --private-key $PK

cast send $VAULT "withdrawToken(address,uint256)" $TOKEN 100000000 \
  --rpc-url $RPC --private-key $PK
```
