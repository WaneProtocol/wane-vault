// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { Script, console2 } from "forge-std/Script.sol";
import { WaneVaultFactory } from "../src/WaneVaultFactory.sol";

/// @notice Deploy the WaneVaultFactory, wired to the already-live WanePolicy.
///         Per-owner WaneVaults are created from this factory at runtime; only
///         the factory contract needs deploying here. It reuses the existing
///         registry + policy, so no new economy or genesis is involved.
///
/// Env:
///   PRIVATE_KEY  deployer
///   POLICY       WanePolicy address (defaults to the live Base mainnet policy)
contract DeployVaultFactory is Script {
    function run() external {
        uint256 pk = vm.envUint("PRIVATE_KEY");
        address policy = vm.envOr("POLICY", address(0x26deE4503C7f67356837ED41cE285026EF256667));

        vm.startBroadcast(pk);
        WaneVaultFactory factory = new WaneVaultFactory(policy);
        vm.stopBroadcast();

        console2.log("WaneVaultFactory:", address(factory));
        console2.log("wired policy    :", policy);
        console2.log("--- chainid", block.chainid, "---");
    }
}
