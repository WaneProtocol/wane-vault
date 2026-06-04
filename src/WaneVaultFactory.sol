// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { WaneVault } from "./WaneVault.sol";

/// @title WaneVaultFactory
/// @notice Deploys one WaneVault per owner at a deterministic CREATE2 address, so
///         a client can compute its vault address before it exists and fund it
///         ahead of time. All vaults share the same WanePolicy and antibody
///         registry the policy reads.
contract WaneVaultFactory {
    address public immutable policy;
    mapping(address => address) public vaultOf;

    event VaultCreated(address indexed owner, address vault);

    error VaultExists();

    constructor(
        address policy_
    ) {
        policy = policy_;
    }

    /// @notice Create the caller's vault.
    function createVault() external returns (address vault) {
        return _create(msg.sender);
    }

    /// @notice Create a vault for `owner` (owner still solely controls it).
    function createVaultFor(
        address owner
    ) external returns (address vault) {
        return _create(owner);
    }

    function _create(
        address owner
    ) internal returns (address vault) {
        if (vaultOf[owner] != address(0)) revert VaultExists();
        vault = address(new WaneVault{ salt: _salt(owner) }(owner, policy));
        vaultOf[owner] = vault;
        emit VaultCreated(owner, vault);
    }

    /// @notice The deterministic vault address for `owner`, whether or not it
    ///         has been created yet.
    function predict(
        address owner
    ) external view returns (address) {
        bytes32 initHash =
            keccak256(abi.encodePacked(type(WaneVault).creationCode, abi.encode(owner, policy)));
        bytes32 h = keccak256(abi.encodePacked(bytes1(0xff), address(this), _salt(owner), initHash));
        return address(uint160(uint256(h)));
    }

    function _salt(
        address owner
    ) internal pure returns (bytes32) {
        return bytes32(uint256(uint160(owner)));
    }
}
