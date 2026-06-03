// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { IWanePolicyView, IERC20Minimal } from "./IWanePolicy.sol";

/// @title WaneVault
/// @notice A non-custodial screening smart wallet, the EVM counterpart of the
///         Solana Wane session vault. Funds (ETH and ERC-20) live in this
///         contract; only the owner drives it; the contract can only BLOCK, never
///         divert. Every outbound action routed through execute() is screened
///         against the owner's WanePolicy (and the antibody registry that policy
///         reads) BEFORE it runs, and a flagged target reverts before any value
///         moves.
///
///         Why this is stronger than the 7702 delegate: a 7702 guard only runs
///         when the wallet routes a call through execute(); a raw key-signed tx
///         bypasses it. Funds held in this vault have no such bypass. The only
///         ways out are execute() (screened) and withdraw() (owner-only, back to
///         the owner), so deposits are never trapped either.
///
///         It also screens the REAL recipient of an ERC-20 movement, decoded from
///         calldata (transfer / transferFrom / approve), not just the token
///         contract being called. A token drain to a flagged address is therefore
///         caught, which a target-only screen would miss.
contract WaneVault {
    IWanePolicyView public immutable policy;
    address public immutable owner;

    /// ERC-20 selectors whose recipient/spender is decoded and screened
    bytes4 private constant SEL_TRANSFER = 0xa9059cbb; // transfer(address,uint256)
    bytes4 private constant SEL_TRANSFER_FROM = 0x23b872dd; // transferFrom(address,address,uint256)
    bytes4 private constant SEL_APPROVE = 0x095ea7b3; // approve(address,uint256)

    error NotOwner();
    error Blocked(address target, uint8 reason);
    error CallFailed();
    error BatchLengthMismatch();

    event Screened(address indexed target, uint256 value, bool allowed, uint8 reason);
    event Executed(address indexed target, uint256 value, bytes4 selector);
    event Withdrawn(address indexed token, uint256 amount);

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    constructor(
        address owner_,
        address policy_
    ) {
        owner = owner_;
        policy = IWanePolicyView(policy_);
    }

    /// @notice Accept inbound ETH. Deposits are just transfers to this address.
    ///         Wane screens only OUTBOUND actions, so incoming value passes.
    receive() external payable { }

    /* screened outbound actions */

    /// @notice Screen + run one action from the vault's own balance.
    function execute(
        address target,
        uint256 value,
        bytes calldata data
    ) external onlyOwner returns (bytes memory ret) {
        _screen(target, value, data);
        bool ok;
        (ok, ret) = target.call{ value: value }(data);
        if (!ok) revert CallFailed();
        emit Executed(target, value, data.length >= 4 ? bytes4(data[0:4]) : bytes4(0));
    }

    /// @notice Screen + run a batch. Any flagged action reverts the whole batch.
    function executeBatch(
        address[] calldata targets,
        uint256[] calldata values,
        bytes[] calldata datas
    ) external onlyOwner returns (bytes[] memory rets) {
        uint256 n = targets.length;
        if (values.length != n || datas.length != n) revert BatchLengthMismatch();
        rets = new bytes[](n);
        for (uint256 i; i < n;) {
            _screen(targets[i], values[i], datas[i]);
            (bool ok, bytes memory r) = targets[i].call{ value: values[i] }(datas[i]);
            if (!ok) revert CallFailed();
            rets[i] = r;
            unchecked {
                ++i;
            }
        }
    }

    /* owner withdraw (unscreened: returning your own funds is safe) */

    function withdrawETH(
        uint256 amount
    ) external onlyOwner {
        (bool ok,) = owner.call{ value: amount }("");
        if (!ok) revert CallFailed();
        emit Withdrawn(address(0), amount);
    }

    function withdrawToken(
        address token,
        uint256 amount
    ) external onlyOwner {
        if (!IERC20Minimal(token).transfer(owner, amount)) revert CallFailed();
        emit Withdrawn(token, amount);
    }

    /* views */

    /// @notice Dry-run the screen without executing. Free.
    function wouldAllow(
        address target,
        uint256 value,
        bytes calldata data
    ) external view returns (bool allowed, uint8 reason) {
        (allowed, reason,) = _evaluate(target, value, data);
    }

    /* internal screening */

    function _screen(
        address target,
        uint256 value,
        bytes calldata data
    ) internal {
        (bool allowed, uint8 reason, address flagged) = _evaluate(target, value, data);
        emit Screened(target, value, allowed, reason);
        if (!allowed) revert Blocked(flagged, reason);
    }

    function _evaluate(
        address target,
        uint256 value,
        bytes calldata data
    ) internal view returns (bool allowed, uint8 reason, address flagged) {
        uint128 amount = value > type(uint128).max ? type(uint128).max : uint128(value);
        bytes4 selector = data.length >= 4 ? bytes4(data[0:4]) : bytes4(0);

        // 1. screen the call target itself (native recipient, contract address +
        //    bytecode, call-pattern selector, spend caps) via the owner's policy.
        (bool a1, uint8 r1) = selector != bytes4(0)
            ? policy.evaluateCall(owner, target, selector, amount)
            : policy.evaluate(owner, target, amount);
        if (!a1) return (false, r1, target);

        // 2. for an ERC-20 movement, screen the REAL recipient decoded from
        //    calldata. amount = 0 so spend caps (denominated in native value) do
        //    not misfire on token units; the address antibody check still runs.
        address recip = _erc20Recipient(selector, data);
        if (recip != address(0)) {
            (bool a2, uint8 r2) = policy.evaluate(owner, recip, 0);
            if (!a2) return (false, r2, recip);
        }
        return (true, 0, address(0));
    }

    function _erc20Recipient(
        bytes4 selector,
        bytes calldata data
    ) internal pure returns (address) {
        if ((selector == SEL_TRANSFER || selector == SEL_APPROVE) && data.length >= 36) {
            return address(uint160(uint256(bytes32(data[4:36]))));
        }
        if (selector == SEL_TRANSFER_FROM && data.length >= 68) {
            return address(uint160(uint256(bytes32(data[36:68]))));
        }
        return address(0);
    }
}
