// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { Script, console2 } from "forge-std/Script.sol";
import { VmSafe } from "forge-std/Vm.sol";
import { WaneVault } from "../src/WaneVault.sol";
import { WaneVaultFactory } from "../src/WaneVaultFactory.sol";
import { WaneRegistry } from "../src/WaneRegistry.sol";
import { WanePolicy } from "../src/WanePolicy.sol";
import { WaneToken } from "../src/WaneToken.sol";
import { WaneTypes } from "../src/WaneTypes.sol";

/// @notice End-to-end walk of the session-key lifecycle, printed step by step.
///         Run: forge script script/SessionDemo.s.sol -vv
///         No chain, no funds: pure local simulation.
contract SessionDemo is Script {
    function run() external {
        address treasury = vm.addr(0xA11CE);
        address owner = vm.addr(0x0E); // the human / master key
        address friend = vm.addr(0xF12E2D);
        address drainer = vm.addr(0xDEAD);

        // deploy the Wane stack
        WaneToken token = new WaneToken(treasury);
        WaneRegistry reg = new WaneRegistry(address(token), treasury);
        WanePolicy pol = new WanePolicy(address(reg));
        WaneVaultFactory factory = new WaneVaultFactory(address(pol));

        // seed a known drainer so the screen has something to block
        WaneTypes.ThreatKind[] memory k = new WaneTypes.ThreatKind[](1);
        bytes32[] memory s = new bytes32[](1);
        bytes32[] memory e = new bytes32[](1);
        k[0] = WaneTypes.ThreatKind.Address;
        s[0] = bytes32(uint256(uint160(drainer)));
        e[0] = keccak256("seed");
        reg.seedGenesis(k, s, e);

        // owner creates + funds + enrolls their vault
        vm.prank(owner);
        WaneVault vault = WaneVault(payable(factory.createVault()));
        vm.prank(owner);
        pol.enroll(owner, pol.K_ALL(), 0, 0, 0, 0);
        vm.deal(address(vault), 10 ether);

        console2.log("=== Wane session key, end to end ===");
        console2.log("owner (master key) :", owner);
        console2.log("vault (funds here) :", address(vault));
        console2.log("vault balance      :", address(vault).balance);

        // 1) the agent's SESSION KEY is a brand-new keypair (NOT the owner key)
        VmSafe.Wallet memory session = vm.createWallet("agent-session");
        console2.log("");
        console2.log("-- 1. generate a fresh session keypair for the agent --");
        console2.log("session key address:", session.addr);
        console2.log("session PRIVATE key:", vm.toString(bytes32(session.privateKey)));
        console2.log("(this private key is what the AGENT holds + signs with)");

        // 2) owner authorizes it: 1 ETH per-tx cap, 7-day expiry
        vm.prank(owner);
        vault.setSession(session.addr, uint64(block.timestamp + 7 days), 1 ether, 0);
        console2.log("");
        console2.log("-- 2. owner authorizes the session key (perTxCap 1 ETH, 7d) --");
        console2.log("vault.sessionKey() :", vault.sessionKey());

        // 3) agent sends within cap, to a clean address -> WORKS
        console2.log("");
        console2.log("-- 3. agent sends 0.5 ETH to a clean address --");
        vm.prank(session.addr);
        try vault.execute(friend, 0.5 ether, "") {
            console2.log("   OK, sent. friend balance:", friend.balance);
        } catch {
            console2.log("   (unexpected revert)");
        }

        // 4) agent tries 2 ETH (over the 1 ETH per-tx cap) -> BLOCKED
        console2.log("");
        console2.log("-- 4. agent tries 2 ETH (over per-tx cap) --");
        vm.prank(session.addr);
        try vault.execute(friend, 2 ether, "") {
            console2.log("   (unexpected success)");
        } catch {
            console2.log("   BLOCKED: over per-tx cap");
        }

        // 5) agent tries sending to the flagged drainer -> BLOCKED (screen)
        console2.log("");
        console2.log("-- 5. agent tries to send to a flagged drainer --");
        vm.prank(session.addr);
        try vault.execute(drainer, 0.5 ether, "") {
            console2.log("   (unexpected success)");
        } catch {
            console2.log("   BLOCKED: drainer flagged by antibody");
        }

        // 6) agent tries to WITHDRAW -> BLOCKED (session keys can never withdraw)
        console2.log("");
        console2.log("-- 6. agent tries to withdraw funds out --");
        vm.prank(session.addr);
        try vault.withdrawETH(1 ether) {
            console2.log("   (unexpected success)");
        } catch {
            console2.log("   BLOCKED: session key cannot withdraw");
        }

        // 7) owner revokes -> the session key is dead instantly
        vm.prank(owner);
        vault.revokeSession();
        console2.log("");
        console2.log("-- 7. owner revokes the session key --");
        vm.prank(session.addr);
        try vault.execute(friend, 0.1 ether, "") {
            console2.log("   (unexpected success)");
        } catch {
            console2.log("   BLOCKED: session revoked, key is dead");
        }

        console2.log("");
        console2.log("=== the agent never touched the owner key. done. ===");
    }
}
