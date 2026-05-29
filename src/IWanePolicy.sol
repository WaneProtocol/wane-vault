// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title IWanePolicy
/// @notice The read surface WaneVault calls before any outbound action. The vault
///         never writes to the policy; it only asks "is this move allowed for my
///         owner". Both calls are pure view, so screening is free.
interface IWanePolicyView {
    /// @notice Value-only screen (native send / generic target).
    /// @return allowed false means block; reason is a WanePolicy R_* code.
    function evaluate(
        address agent,
        address target,
        uint128 amount
    ) external view returns (bool allowed, uint8 reason);

    /// @notice Full screen including the 4-byte selector of the call. Enforces
    ///         the selector allowlist and call-pattern antibodies when scoped.
    function evaluateCall(
        address agent,
        address target,
        bytes4 selector,
        uint128 amount
    ) external view returns (bool allowed, uint8 reason);
}

/// @notice Minimal ERC-20 surface the vault uses for owner withdrawals.
interface IERC20Minimal {
    function transfer(
        address to,
        uint256 amount
    ) external returns (bool);
    function balanceOf(
        address account
    ) external view returns (uint256);
}
