// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { IWanePolicyView, IERC20Minimal } from "./IWanePolicy.sol";

/// @title WaneVault
/// @notice A non-custodial screening smart wallet with a true session key.
///
///         Funds (ETH and ERC-20) live in this contract. The OWNER is a master
///         key that can do anything (screened execute + owner-only withdraw).
///         The owner can also grant an AGENT a scoped SESSION KEY: a separate
///         key that can ONLY run screened execute(), bounded by a per-tx cap, a
///         rolling daily cap, and an expiry, and that can NEVER withdraw or
///         change the session. So you hand an agent a session key instead of
///         your master key: if that key is phished or leaked, the blast radius
///         is capped and time-boxed, and the owner can revoke it instantly.
///
///         Every outbound action (owner OR session key) is screened against the
///         owner's WanePolicy and the antibody registry it reads BEFORE it runs;
///         a flagged target reverts before any value moves. The contract can
///         only block, never divert: the sole exits are execute() (screened) and
///         withdraw() (owner-only, back to the owner), so deposits are never
///         trapped.
///
///         It also screens the REAL recipient of an ERC-20 movement, decoded from
///         calldata (transfer / transferFrom / approve), not just the token
///         contract being called. A token drain to a flagged address is therefore
///         caught, which a target-only screen would miss.
contract WaneVault {
    IWanePolicyView public immutable policy;
    address public immutable owner;

    /// ── scoped agent session key ─────────────────────────────────────────
    address public sessionKey; // 0 = no active session
    uint64 public sessionExpiry; // unix ts; the key is dead at/after this time
    uint128 public sessionPerTxCap; // max native value per execute (0 = unlimited)
    uint128 public sessionDailyCap; // max native value per rolling 24h (0 = unlimited)
    uint128 public sessionSpent; // native value spent in the current window
    uint64 public sessionWindowStart; // start of the current 24h window

    /// ERC-20 selectors whose recipient/spender is decoded and screened
    bytes4 private constant SEL_TRANSFER = 0xa9059cbb; // transfer(address,uint256)
    bytes4 private constant SEL_TRANSFER_FROM = 0x23b872dd; // transferFrom(address,address,uint256)
    bytes4 private constant SEL_APPROVE = 0x095ea7b3; // approve(address,uint256)

    error NotOwner();
    error NotDriver();
    error SessionExpired();
    error OverPerTxCap();
    error OverDailyCap();
    error Blocked(address target, uint8 reason);
    error CallFailed();
    error BatchLengthMismatch();

    event Screened(address indexed target, uint256 value, bool allowed, uint8 reason);
    event Executed(address indexed target, uint256 value, bytes4 selector);
    event Withdrawn(address indexed token, uint256 amount);
    event SessionSet(address indexed key, uint64 expiry, uint128 perTxCap, uint128 dailyCap);
    event SessionRevoked(address indexed key);

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

    /* session key management (owner only) */

    /// @notice Grant or replace the scoped agent session key. key=0 clears it.
    ///         The rolling window resets so the daily cap starts fresh.
    function setSession(
        address key,
        uint64 expiry,
        uint128 perTxCap,
        uint128 dailyCap
    ) external onlyOwner {
        sessionKey = key;
        sessionExpiry = expiry;
        sessionPerTxCap = perTxCap;
        sessionDailyCap = dailyCap;
        sessionSpent = 0;
        sessionWindowStart = uint64(block.timestamp);
        emit SessionSet(key, expiry, perTxCap, dailyCap);
    }

    /// @notice Revoke the session key immediately. The agent can no longer move
    ///         anything; the owner's funds are untouched.
    function revokeSession() external onlyOwner {
        emit SessionRevoked(sessionKey);
        sessionKey = address(0);
        sessionExpiry = 0;
        sessionPerTxCap = 0;
        sessionDailyCap = 0;
        sessionSpent = 0;
    }

    /// @notice Is there a live (set, non-expired) session key right now?
    function sessionActive() external view returns (bool) {
        return sessionKey != address(0) && block.timestamp < sessionExpiry;
    }

    /* screened outbound actions (owner or session key) */

    /// @notice Screen + run one action from the vault's own balance.
    function execute(
        address target,
        uint256 value,
        bytes calldata data
    ) external returns (bytes memory ret) {
        if (!_driverIsOwner()) _chargeSession(value);
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
    ) external returns (bytes[] memory rets) {
        uint256 n = targets.length;
        if (values.length != n || datas.length != n) revert BatchLengthMismatch();
        bool isOwner = _driverIsOwner();
        rets = new bytes[](n);
        for (uint256 i; i < n;) {
            if (!isOwner) _chargeSession(values[i]);
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
    /* A session key can NEVER reach these: withdraw is owner-only. */

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

    /* internal: driver authorization + session caps */

    /// @dev Authorize the caller as a driver. Returns true if it is the owner.
    ///      A session caller must be the live, non-expired session key.
    function _driverIsOwner() internal view returns (bool) {
        if (msg.sender == owner) return true;
        if (msg.sender != sessionKey || sessionKey == address(0)) revert NotDriver();
        if (block.timestamp >= sessionExpiry) revert SessionExpired();
        return false;
    }

    /// @dev Enforce + record the session key's per-tx and rolling daily caps.
    function _chargeSession(
        uint256 value
    ) internal {
        if (sessionPerTxCap != 0 && value > sessionPerTxCap) revert OverPerTxCap();
        if (sessionDailyCap != 0) {
            if (block.timestamp >= sessionWindowStart + 1 days) {
                sessionWindowStart = uint64(block.timestamp);
                sessionSpent = 0;
            }
            uint256 spent = uint256(sessionSpent) + value;
            if (spent > sessionDailyCap) revert OverDailyCap();
            sessionSpent = uint128(spent);
        }
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
