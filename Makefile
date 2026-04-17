.PHONY: build test fmt fmt-check snapshot clean sdk deploy

build:
	forge build

test:
	forge test -vvv

fmt:
	forge fmt

fmt-check:
	forge fmt --check

snapshot:
	forge snapshot

clean:
	forge clean
	rm -rf sdk/dist

sdk:
	cd sdk && npm install && npm run build

# requires PRIVATE_KEY (and optionally POLICY) in the environment
deploy:
	forge script script/DeployVaultFactory.s.sol:DeployVaultFactory \
		--rpc-url base --broadcast --verify
