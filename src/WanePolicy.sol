// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { WaneTypes } from "./WaneTypes.sol";

interface IWaneRegistryView {
    function check(
        WaneTypes.ThreatKind kind,
        bytes32 subject
    ) external view returns (bool active, uint64 id);
    function antibodies(
        uint64 id
    )
        external
        view
        returns (
            uint64 id_,
            WaneTypes.ThreatKind kind,
            WaneTypes.Status status,
            address publisher,
            uint96 stake,
            uint64 mintedBlock,
            uint32 corroborations,
            bytes32 subject,
            bytes32 evidence
        );
}

/// @title WanePolicy
/// @notice Register a bot and set exactly how much Wane protects it. A bot owner
///         enrolls an agent once, picks a protection scope, and from then on the
///         SDK / v4 hook evaluates every action against (this policy + the shared
///         antibody registry) automatically.
///
///         Scope the owner controls, per agent:
///           - on/off + fast kill switch (owner OR guardian) + global pause
///           - which threat kinds to block (bitmask)
///           - sensitivity: only trust antibodies with >= minCorroborations
///           - per-tx and daily spend caps
///           - policy expiry (TTL / dead-man switch)
///           - allowlist (always pass) / blocklist (always stop)
///           - function-selector allowlist (only approved 4byte selectors)
///           - token allowlist (only approved assets)
///         Plus a contract-level curated recipient denylist (guardian-set).
contract WanePolicy {
    IWaneRegistryView public immutable registry;

    /* threat-kind bitmask flags (1<<kind) */
    uint8 public constant K_ADDRESS = 1 << 0;
    uint8 public constant K_CALL = 1 << 1;
    uint8 public constant K_BYTECODE = 1 << 2;
    uint8 public constant K_SEMANTIC = 1 << 3;
    uint8 public constant K_ALL = K_ADDRESS | K_CALL | K_BYTECODE | K_SEMANTIC;

    /* evaluate() reason codes */
    uint8 public constant R_OK = 0;
    uint8 public constant R_BLOCKLIST = 1;
    uint8 public constant R_ANTIBODY = 2;
    uint8 public constant R_PERTX = 3;
    uint8 public constant R_DAILY = 4;
    uint8 public constant R_PAUSED = 5; // agent or global kill switch
    uint8 public constant R_GLOBAL_DENY = 6; // curated global recipient denylist
    uint8 public constant R_EXPIRED = 7; // policy TTL elapsed
    uint8 public constant R_SELECTOR = 8; // function selector not allowed
    uint8 public constant R_TOKEN = 9; // token not allowed

    struct Policy {
        address owner;
        bool enabled; //        owner intent on/off
        bool paused; //         fast kill switch (owner or guardian)
        bool selectorScoped; // enforce selector allowlist only when true
        bool tokenScoped; //    enforce token allowlist only when true
        uint8 blockKinds; //    bitmask of threat kinds to enforce
        uint32 minCorrobs; //   sensitivity
        uint40 expiresAt; //    0 = never; else authority dies at this ts
        uint128 perTxCap; //    max value per action (0 = no cap)
        uint128 dailyCap; //    max value per rolling day (0 = no cap)
        uint128 spentToday;
        uint64 dayStart;
    }

    address public guardian; // watcher role: can kill agents + set global pause/denylist
    bool public globalPaused;

    mapping(address => Policy) public policies; // agent => policy
    mapping(address => mapping(address => bool)) public allowlist; // agent => target => pass
    mapping(address => mapping(address => bool)) public blocklist; // agent => target => stop
    mapping(address => mapping(bytes4 => bool)) public allowedSelector; // agent => selector => ok
    mapping(address => mapping(address => bool)) public allowedToken; // agent => token => ok
    mapping(address => bool) public globalDenied; // curated bad recipients (all agents)

    event AgentEnrolled(address indexed agent, address indexed owner);
    event PolicySet(
        address indexed agent,
        bool enabled,
        uint8 blockKinds,
        uint32 minCorrobs,
        uint128 perTxCap,
        uint128 dailyCap,
        uint40 expiresAt
    );
    event PausedSet(address indexed agent, bool paused, address by);
    event GlobalPausedSet(bool paused);
    event GlobalDeniedSet(address indexed target, bool value);
    event ListSet(address indexed agent, address indexed target, bool allow, bool value);
    event SelectorSet(address indexed agent, bytes4 indexed selector, bool value, bool scoped);
    event TokenSet(address indexed agent, address indexed token, bool value, bool scoped);
    event Spent(address indexed agent, uint128 amount, uint128 spentToday);
    event GuardianSet(address indexed guardian);

    error NotOwner();
    error NotGuardianOrOwner();
    error NotGuardian();

    constructor(
        address registry_
    ) {
        registry = IWaneRegistryView(registry_);
        guardian = msg.sender;
    }

    modifier onlyAgentOwner(
        address agent
    ) {
        if (policies[agent].owner != msg.sender) revert NotOwner();
        _;
    }

    /* ── enroll + configure ──────────────────────────────────────────── */

    function enroll(
        address agent,
        uint8 blockKinds,
        uint32 minCorrobs,
        uint128 perTxCap,
        uint128 dailyCap,
        uint40 expiresAt
    ) external {
        Policy storage p = policies[agent];
        if (p.owner == address(0)) {
            p.owner = msg.sender;
            emit AgentEnrolled(agent, msg.sender);
        } else if (p.owner != msg.sender) {
            revert NotOwner();
        }
        p.enabled = true;
        p.paused = false;
        p.blockKinds = blockKinds == 0 ? K_ALL : blockKinds;
        p.minCorrobs = minCorrobs;
        p.perTxCap = perTxCap;
        p.dailyCap = dailyCap;
        p.expiresAt = expiresAt;
        emit PolicySet(agent, p.enabled, p.blockKinds, minCorrobs, perTxCap, dailyCap, expiresAt);
    }

    function setEnabled(
        address agent,
        bool on
    ) external onlyAgentOwner(agent) {
        Policy storage p = policies[agent];
        p.enabled = on;
        emit PolicySet(agent, on, p.blockKinds, p.minCorrobs, p.perTxCap, p.dailyCap, p.expiresAt);
    }

    function setScope(
        address agent,
        uint8 blockKinds,
        uint32 minCorrobs,
        uint128 perTxCap,
        uint128 dailyCap,
        uint40 expiresAt
    ) external onlyAgentOwner(agent) {
        Policy storage p = policies[agent];
        p.blockKinds = blockKinds;
        p.minCorrobs = minCorrobs;
        p.perTxCap = perTxCap;
        p.dailyCap = dailyCap;
        p.expiresAt = expiresAt;
        emit PolicySet(agent, p.enabled, blockKinds, minCorrobs, perTxCap, dailyCap, expiresAt);
    }

    /* ── kill switch (owner OR guardian) + global pause (guardian) ───── */

    function setPaused(
        address agent,
        bool p_
    ) external {
        Policy storage p = policies[agent];
        if (msg.sender != p.owner && msg.sender != guardian) revert NotGuardianOrOwner();
        p.paused = p_;
        emit PausedSet(agent, p_, msg.sender);
    }

    function setGlobalPaused(
        bool p_
    ) external {
        if (msg.sender != guardian) revert NotGuardian();
        globalPaused = p_;
        emit GlobalPausedSet(p_);
    }

    function setGlobalDenied(
        address target,
        bool value
    ) external {
        if (msg.sender != guardian) revert NotGuardian();
        globalDenied[target] = value;
        emit GlobalDeniedSet(target, value);
    }

    function setGuardian(
        address g
    ) external {
        if (msg.sender != guardian) revert NotGuardian();
        guardian = g;
        emit GuardianSet(g);
    }

    /* ── per-agent lists ─────────────────────────────────────────────── */

    function setAllow(
        address agent,
        address target,
        bool value
    ) external onlyAgentOwner(agent) {
        allowlist[agent][target] = value;
        emit ListSet(agent, target, true, value);
    }

    function setBlock(
        address agent,
        address target,
        bool value
    ) external onlyAgentOwner(agent) {
        blocklist[agent][target] = value;
        emit ListSet(agent, target, false, value);
    }

    function setSelectorScoped(
        address agent,
        bool scoped
    ) external onlyAgentOwner(agent) {
        policies[agent].selectorScoped = scoped;
        emit SelectorSet(agent, 0x00000000, false, scoped);
    }

    function setSelector(
        address agent,
        bytes4 selector,
        bool value
    ) external onlyAgentOwner(agent) {
        allowedSelector[agent][selector] = value;
        emit SelectorSet(agent, selector, value, policies[agent].selectorScoped);
    }

    function setTokenScoped(
        address agent,
        bool scoped
    ) external onlyAgentOwner(agent) {
        policies[agent].tokenScoped = scoped;
        emit TokenSet(agent, address(0), false, scoped);
    }

    function setToken(
        address agent,
        address token,
        bool value
    ) external onlyAgentOwner(agent) {
        allowedToken[agent][token] = value;
        emit TokenSet(agent, token, value, policies[agent].tokenScoped);
    }

    /* ── evaluate: the layer reads this before the agent acts ────────── */

    /// @notice Value-only check (native send / generic target). Pure view, free.
    function evaluate(
        address agent,
        address target,
        uint128 amount
    ) public view returns (bool allowed, uint8 reason) {
        return _evaluate(agent, target, 0x00000000, amount, false);
    }

    /// @notice Full check including the called 4-byte selector. The executor /
    ///         v4 hook passes calldata[0:4]. Enforces selector allowlist when scoped.
    function evaluateCall(
        address agent,
        address target,
        bytes4 selector,
        uint128 amount
    ) public view returns (bool allowed, uint8 reason) {
        return _evaluate(agent, target, selector, amount, true);
    }

    function _evaluate(
        address agent,
        address target,
        bytes4 selector,
        uint128 amount,
        bool haveSelector
    ) internal view returns (bool, uint8) {
        Policy storage p = policies[agent];
        if (p.owner == address(0) || !p.enabled) return (true, R_OK);

        // fastest kills first
        if (globalPaused || p.paused) return (false, R_PAUSED);
        if (p.expiresAt != 0 && block.timestamp >= p.expiresAt) return (false, R_EXPIRED);

        // explicit lists
        if (allowlist[agent][target]) return (true, R_OK);
        if (blocklist[agent][target]) return (false, R_BLOCKLIST);
        if (globalDenied[target]) return (false, R_GLOBAL_DENY);

        // selector allowlist (only when owner opted in and a selector was provided)
        if (haveSelector && p.selectorScoped && selector != 0x00000000) {
            if (!allowedSelector[agent][selector]) return (false, R_SELECTOR);
        }
        // call-pattern antibody (wires K_CALL)
        if (haveSelector && p.blockKinds & K_CALL != 0 && selector != 0x00000000) {
            (bool fc, uint64 idc) =
                registry.check(WaneTypes.ThreatKind.CallPattern, bytes32(selector));
            if (fc && _meetsSensitivity(idc, p.minCorrobs)) return (false, R_ANTIBODY);
        }

        // antibody checks
        if (p.blockKinds & K_ADDRESS != 0) {
            (bool fa, uint64 ida) =
                registry.check(WaneTypes.ThreatKind.Address, bytes32(uint256(uint160(target))));
            if (fa && _meetsSensitivity(ida, p.minCorrobs)) return (false, R_ANTIBODY);
        }
        if (p.blockKinds & K_BYTECODE != 0 && target.code.length > 0) {
            (bool fb, uint64 idb) = registry.check(WaneTypes.ThreatKind.Bytecode, target.codehash);
            if (fb && _meetsSensitivity(idb, p.minCorrobs)) return (false, R_ANTIBODY);
        }

        // spend caps
        if (p.perTxCap != 0 && amount > p.perTxCap) return (false, R_PERTX);
        if (p.dailyCap != 0) {
            uint128 used = _usedToday(p);
            if (used + amount > p.dailyCap) return (false, R_DAILY);
        }
        return (true, R_OK);
    }

    /// @notice Is `token` allowed for this agent? Executor calls this at the
    ///         swap boundary where tokenIn/Out are explicit. Pass when not scoped.
    function isTokenAllowed(
        address agent,
        address token
    ) external view returns (bool) {
        Policy storage p = policies[agent];
        if (p.owner == address(0) || !p.enabled || !p.tokenScoped) return true;
        return allowedToken[agent][token];
    }

    function recordSpend(
        address agent,
        uint128 amount
    ) external onlyAgentOwner(agent) {
        Policy storage p = policies[agent];
        uint64 today = uint64(block.timestamp / 1 days);
        if (p.dayStart != today) {
            p.dayStart = today;
            p.spentToday = 0;
        }
        p.spentToday += amount;
        emit Spent(agent, amount, p.spentToday);
    }

    /* ── views ───────────────────────────────────────────────────────── */

    function usedToday(
        address agent
    ) external view returns (uint128) {
        return _usedToday(policies[agent]);
    }

    function _usedToday(
        Policy storage p
    ) internal view returns (uint128) {
        uint64 today = uint64(block.timestamp / 1 days);
        return p.dayStart == today ? p.spentToday : 0;
    }

    function _meetsSensitivity(
        uint64 id,
        uint32 minCorrobs
    ) internal view returns (bool) {
        if (minCorrobs == 0) return true;
        (,,,, uint96 stake,, uint32 corrobs,,) = registry.antibodies(id);
        if (stake == 0) return true; // genesis / protocol-owned, always trusted
        return corrobs >= minCorrobs;
    }
}
