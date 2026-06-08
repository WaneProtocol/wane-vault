// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { Test } from "forge-std/Test.sol";
import { WaneVault } from "../src/WaneVault.sol";
import { WaneVaultFactory } from "../src/WaneVaultFactory.sol";
import { WaneRegistry } from "../src/WaneRegistry.sol";
import { WanePolicy } from "../src/WanePolicy.sol";
import { WaneToken } from "../src/WaneToken.sol";
import { WaneTypes } from "../src/WaneTypes.sol";

interface IERC20 {
    function transfer(
        address to,
        uint256 amount
    ) external returns (bool);
    function balanceOf(
        address account
    ) external view returns (uint256);
}

/// @notice The non-custodial screening vault: clean actions run, flagged targets
///         revert before value moves, and crucially an ERC-20 transfer to a
///         flagged recipient is blocked by decoding the real recipient from
///         calldata (a target-only screen would miss it). Funds are never trapped.
contract WaneVaultTest is Test {
    WaneToken token;
    WaneRegistry reg;
    WanePolicy pol;
    WaneVaultFactory factory;
    WaneVault vault;

    address treasury = makeAddr("treasury");
    address owner = makeAddr("owner");
    address drainer = makeAddr("drainer");
    address friend = makeAddr("friend");

    function setUp() public {
        token = new WaneToken(treasury);
        reg = new WaneRegistry(address(token), treasury);
        pol = new WanePolicy(address(reg));
        factory = new WaneVaultFactory(address(pol));

        // seed drainer as a genesis antibody (enforces immediately)
        WaneTypes.ThreatKind[] memory k = new WaneTypes.ThreatKind[](1);
        bytes32[] memory s = new bytes32[](1);
        bytes32[] memory e = new bytes32[](1);
        k[0] = WaneTypes.ThreatKind.Address;
        s[0] = bytes32(uint256(uint160(drainer)));
        e[0] = keccak256("seed");
        reg.seedGenesis(k, s, e);

        // owner creates their vault and enrolls a full-protection policy
        vm.prank(owner);
        vault = WaneVault(payable(factory.createVault()));
        vm.prank(owner);
        pol.enroll(owner, pol.K_ALL(), 0, 0, 0, 0);

        // fund the vault with ETH and tokens
        vm.deal(address(vault), 10 ether);
        vm.prank(treasury);
        IERC20(address(token)).transfer(address(vault), 1000 ether);
    }

    /* ── factory address is deterministic ────────────────────────────── */

    function test_PredictMatchesCreated() public view {
        assertEq(factory.predict(owner), address(vault), "predicted == created");
        assertEq(factory.vaultOf(owner), address(vault));
    }

    /* ── clean ETH send runs ─────────────────────────────────────────── */

    function test_CleanEthSend() public {
        uint256 before = friend.balance;
        vm.prank(owner);
        vault.execute(friend, 1 ether, "");
        assertEq(friend.balance, before + 1 ether, "clean ETH send went through");
    }

    /* ── ETH send to drainer is blocked ──────────────────────────────── */

    function test_DrainerEthBlocked() public {
        vm.expectRevert(
            abi.encodeWithSelector(WaneVault.Blocked.selector, drainer, pol.R_ANTIBODY())
        );
        vm.prank(owner);
        vault.execute(drainer, 1 ether, "");
        assertEq(drainer.balance, 0, "no ETH moved to drainer");
    }

    /* ── ERC-20 transfer to drainer is blocked (recipient decoded) ───── */

    function test_DrainerTokenBlocked() public {
        bytes memory data = abi.encodeWithSignature("transfer(address,uint256)", drainer, 100 ether);
        vm.expectRevert(
            abi.encodeWithSelector(WaneVault.Blocked.selector, drainer, pol.R_ANTIBODY())
        );
        vm.prank(owner);
        vault.execute(address(token), 0, data);
        assertEq(IERC20(address(token)).balanceOf(drainer), 0, "no tokens moved to drainer");
    }

    /* ── ERC-20 transfer to a clean address runs ─────────────────────── */

    function test_CleanTokenSend() public {
        bytes memory data = abi.encodeWithSignature("transfer(address,uint256)", friend, 100 ether);
        vm.prank(owner);
        vault.execute(address(token), 0, data);
        assertEq(
            IERC20(address(token)).balanceOf(friend), 100 ether, "clean token send went through"
        );
    }

    /* ── only the owner may drive the vault ──────────────────────────── */

    function test_OutsiderCannotExecute() public {
        vm.expectRevert(WaneVault.NotOwner.selector);
        vm.prank(makeAddr("attacker"));
        vault.execute(friend, 1 ether, "");
    }

    /* ── owner can always withdraw (funds never trapped) ─────────────── */

    function test_OwnerWithdraw() public {
        uint256 beforeEth = owner.balance;
        vm.prank(owner);
        vault.withdrawETH(3 ether);
        assertEq(owner.balance, beforeEth + 3 ether, "ETH back to owner");

        uint256 beforeTok = IERC20(address(token)).balanceOf(owner);
        vm.prank(owner);
        vault.withdrawToken(address(token), 500 ether);
        assertEq(
            IERC20(address(token)).balanceOf(owner), beforeTok + 500 ether, "tokens back to owner"
        );
    }

    /* ── batch reverts entirely if any action is flagged ─────────────── */

    function test_BatchRevertsOnDrainer() public {
        address[] memory t = new address[](2);
        uint256[] memory v = new uint256[](2);
        bytes[] memory d = new bytes[](2);
        t[0] = friend;
        v[0] = 0.1 ether;
        d[0] = "";
        t[1] = drainer;
        v[1] = 0.1 ether;
        d[1] = "";
        vm.expectRevert(
            abi.encodeWithSelector(WaneVault.Blocked.selector, drainer, pol.R_ANTIBODY())
        );
        vm.prank(owner);
        vault.executeBatch(t, v, d);
        assertEq(friend.balance, 0, "batch rolled back, friend got nothing");
    }

    /* ── dry-run matches enforcement ─────────────────────────────────── */

    function test_WouldAllow() public view {
        (bool okClean,) = vault.wouldAllow(friend, 1 ether, "");
        assertTrue(okClean, "clean allowed");
        (bool okDrain,) = vault.wouldAllow(drainer, 1 ether, "");
        assertFalse(okDrain, "drainer blocked");
    }
}
