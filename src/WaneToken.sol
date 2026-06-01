// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { ERC20 } from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

/// @title WaneToken ($WANE)
/// @notice The currency of the bloodstream. Staked to mint antibodies, slashed
///         for false ones, paid out as rewards. Fixed supply, no mint after
///         deploy. Distribution handled externally (LP, airdrop, treasury).
contract WaneToken is ERC20 {
    uint256 public constant MAX_SUPPLY = 1_000_000_000e18; // 1B WANE

    constructor(
        address treasury
    ) ERC20("Wane", "WANE") {
        _mint(treasury, MAX_SUPPLY);
    }
}
