// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import { WaneTypes } from "./WaneTypes.sol";
import { IERC20 } from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import { SafeERC20 } from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import { ReentrancyGuard } from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/// @title WaneRegistry
/// @notice The blood. Antibodies (threat memories) live here on Base and any
///         agent reads check() for free before it signs. One agent gets
///         attacked once; every agent after is immune. The threat wanes.
///
/// @dev Hardened build. Key invariants:
///   - A fresh antibody is NOT hook-enforceable until it survives a challenge
///     window OR gathers enough corroborations (anti false-flag censorship).
///   - A `Challenged` antibody stays fail-closed (still blocks) so a drainer
///     can't self-challenge to un-block itself; only `Revoked` stops blocking.
///   - Token liabilities (stakes + bonds + earned) are tracked in `reserved`;
///     payouts can never dip into another user's locked principal.
///   - All token-moving externals are nonReentrant and follow CEI.
contract WaneRegistry is ReentrancyGuard {
    using WaneTypes for *;
    using SafeERC20 for IERC20;

    /* ?? config ???????????????????????????????????????????????????????? */

    IERC20 public immutable wane; // $WANE token (stake / reward currency)

    address public governor; // arbiter + admin (use a multisig/timelock in prod)
    address public pendingGovernor; // two-step transfer
    address public treasury;
    bool public genesisOpen = true; // Genesis seeding window
    bool public paused; // emergency: check() returns clear, writes blocked

    uint96 public mintStake = 100e18; //    $WANE locked to mint an antibody
    uint96 public challengeStake = 200e18; // $WANE to challenge
    uint64 public maturity = 21_600; //     ~72h @ 2s blocks: stake reclaimable
    uint64 public enforceWindow = 1800; // ~1h: blocks before an unchallenged,
    //                                    un-corroborated antibody enforces
    uint32 public enforceCorrobs = 2; //    OR this many corroborations to enforce now
    uint16 public publisherBps = 8000; //  80% of check fees to publisher
    uint96 public checkFee; //              optional metered-check fee (0 = free)

    /* ?? state ????????????????????????????????????????????????????????? */

    uint64 public antibodyCount;
    uint256 public reserved; // sum of all locked stakes + bonds + earned
    mapping(uint64 => WaneTypes.Antibody) public antibodies;
    mapping(bytes32 => uint64) public idByKey; // keccak(kind,subject) => id
    mapping(uint64 => mapping(address => bool)) public hasCorroborated;
    mapping(uint64 => address) public challenger;
    mapping(uint64 => uint96) public challengeBond;
    mapping(address => uint256) public earned; // claimable rewards

    /* ?? events ???????????????????????????????????????????????????????? */

    event AntibodyMinted(
        uint64 indexed id,
        WaneTypes.ThreatKind indexed kind,
        bytes32 indexed subject,
        address publisher,
        bytes32 evidence
    );
    event Corroborated(uint64 indexed id, address indexed by, uint32 total);
    event Challenged(uint64 indexed id, address indexed by);
    event Resolved(uint64 indexed id, WaneTypes.Status status);
    event StakeReclaimed(uint64 indexed id, address indexed publisher, uint96 amount);
    event RewardClaimed(address indexed who, uint256 amount);
    event GenesisSeeded(uint64 indexed id, WaneTypes.ThreatKind kind, bytes32 subject);
    event AntibodyRevokedKeyCleared(uint64 indexed id, bytes32 key);
    event ParamsSet(
        uint96 mintStake,
        uint96 challengeStake,
        uint64 maturity,
        uint16 publisherBps,
        uint96 checkFee
    );
    event WindowSet(uint64 enforceWindow, uint32 enforceCorrobs);
    event GovernorTransferStarted(address indexed pending);
    event GovernorTransferred(address indexed governor);
    event PausedSet(bool paused);
    event GenesisClosedEvent();

    /* ?? errors ???????????????????????????????????????????????????????? */

    error NotGovernor();
    error GenesisClosed();
    error Exists();
    error Unknown();
    error NotActive();
    error AlreadyChallenged();
    error NotChallenged();
    error TooEarly();
    error NotPublisher();
    error SelfCorroborate();
    error AlreadyCorroborated();
    error Paused();
    error BadParams();
    error LengthMismatch();
    error ZeroAddress();
    error NotPending();

    modifier onlyGovernor() {
        if (msg.sender != governor) revert NotGovernor();
        _;
    }

    modifier notPaused() {
        if (paused) revert Paused();
        _;
    }

    constructor(
        address waneToken,
        address treasury_
    ) {
        if (waneToken == address(0) || treasury_ == address(0)) revert ZeroAddress();
        wane = IERC20(waneToken);
        governor = msg.sender;
        treasury = treasury_;
    }

    /* ?? read path: every agent / hook calls this before it signs ?????? */

    /// @notice Is `subject` of `kind` covered by an ENFORCEABLE antibody?
    ///         Free, view. Reading is immunity. Returns false while paused.
    function check(
        WaneTypes.ThreatKind kind,
        bytes32 subject
    ) public view returns (bool active, uint64 id) {
        if (paused) return (false, 0);
        id = idByKey[_key(kind, subject)];
        if (id == 0) return (false, 0);
        active = _enforceable(antibodies[id]);
    }

    /// @notice Convenience: check a plain address (most common case).
    function checkAddress(
        address target
    ) external view returns (bool active, uint64 id) {
        return check(WaneTypes.ThreatKind.Address, bytes32(uint256(uint160(target))));
    }

    /// @notice Check a contract's runtime codehash (catches re-deployed drainers).
    function checkBytecode(
        bytes32 codehash
    ) external view returns (bool active, uint64 id) {
        return check(WaneTypes.ThreatKind.Bytecode, codehash);
    }

    /// @dev An antibody enforces (blocks) only if it is past the dispute risk:
    ///      Active AND (window elapsed OR enough corroborations OR stake==0 i.e.
    ///      protocol-seeded genesis), OR currently Challenged (fail-closed).
    ///      Revoked never enforces.
    function _enforceable(
        WaneTypes.Antibody storage a
    ) internal view returns (bool) {
        if (a.id == 0) return false;
        if (a.status == WaneTypes.Status.Revoked) return false;
        if (a.status == WaneTypes.Status.Challenged) return true; // fail-closed during dispute
        // Active:
        if (a.stake == 0) return true; // genesis / protocol-owned, trusted
        if (a.corroborations >= enforceCorrobs) return true;
        if (block.number >= a.mintedBlock + enforceWindow) return true;
        return false; // young, un-corroborated => not yet weaponizable
    }

    /* ?? write path: an attacked agent mints an antibody ??????????????? */

    function mintAntibody(
        WaneTypes.ThreatKind kind,
        bytes32 subject,
        bytes32 evidence
    ) external nonReentrant notPaused returns (uint64 id) {
        bytes32 key = _key(kind, subject);
        uint64 existing = idByKey[key];
        // allow overwriting a Revoked key (re-flag a re-offender); else block dup
        if (existing != 0 && antibodies[existing].status != WaneTypes.Status.Revoked) {
            revert Exists();
        }

        wane.safeTransferFrom(msg.sender, address(this), mintStake);
        reserved += mintStake;

        id = ++antibodyCount;
        antibodies[id] = WaneTypes.Antibody({
            id: id,
            kind: kind,
            status: WaneTypes.Status.Active,
            publisher: msg.sender,
            stake: mintStake,
            mintedBlock: uint64(block.number),
            corroborations: 0,
            subject: subject,
            evidence: evidence
        });
        idByKey[key] = id;

        emit AntibodyMinted(id, kind, subject, msg.sender, evidence);
    }

    function corroborate(
        uint64 id
    ) external notPaused {
        WaneTypes.Antibody storage a = antibodies[id];
        if (a.id == 0) revert Unknown();
        if (a.status != WaneTypes.Status.Active) revert NotActive();
        if (msg.sender == a.publisher) revert SelfCorroborate();
        if (hasCorroborated[id][msg.sender]) revert AlreadyCorroborated();

        hasCorroborated[id][msg.sender] = true;
        a.corroborations += 1;
        emit Corroborated(id, msg.sender, a.corroborations);
    }

    /* ?? dispute path ?????????????????????????????????????????????????? */

    function challenge(
        uint64 id
    ) external nonReentrant notPaused {
        WaneTypes.Antibody storage a = antibodies[id];
        if (a.id == 0) revert Unknown();
        if (a.status != WaneTypes.Status.Active) revert NotActive();
        if (challenger[id] != address(0)) revert AlreadyChallenged();

        wane.safeTransferFrom(msg.sender, address(this), challengeStake);
        reserved += challengeStake;
        challenger[id] = msg.sender;
        challengeBond[id] = challengeStake;
        a.status = WaneTypes.Status.Challenged; // still fail-closed via _enforceable
        emit Challenged(id, msg.sender);
    }

    /// @notice Governor arbitrates. falsePositive=true => antibody was wrong:
    ///         publisher slashed, challenger gets bond + slashed stake.
    ///         false => antibody upheld: challenger bond credited to publisher.
    /// @dev CEI: all state cleared before any external transfer.
    function resolve(
        uint64 id,
        bool falsePositive
    ) external onlyGovernor nonReentrant {
        WaneTypes.Antibody storage a = antibodies[id];
        if (a.status != WaneTypes.Status.Challenged) revert NotChallenged();
        address chal = challenger[id];
        uint96 bond = challengeBond[id];
        uint96 stake = a.stake;

        challenger[id] = address(0);
        challengeBond[id] = 0;

        if (falsePositive) {
            a.status = WaneTypes.Status.Revoked;
            a.stake = 0;
            // free the key so the subject can be legitimately re-flagged later
            bytes32 key = _key(a.kind, a.subject);
            if (idByKey[key] == id) {
                delete idByKey[key];
                emit AntibodyRevokedKeyCleared(id, key);
            }
            uint256 payout = uint256(bond) + uint256(stake);
            reserved -= payout;
            emit Resolved(id, a.status);
            wane.safeTransfer(chal, payout); // external last
        } else {
            a.status = WaneTypes.Status.Active;
            // move challenger bond from "locked bond" to publisher "earned"
            // (still reserved, just reclassified) ??net reserved unchanged
            earned[a.publisher] += bond;
            emit Resolved(id, a.status);
        }
    }

    /* ?? economics ????????????????????????????????????????????????????? */

    function reclaimStake(
        uint64 id
    ) external nonReentrant {
        WaneTypes.Antibody storage a = antibodies[id];
        if (msg.sender != a.publisher) revert NotPublisher();
        if (a.status != WaneTypes.Status.Active) revert NotActive();
        if (block.number < a.mintedBlock + maturity) revert TooEarly();
        uint96 amt = a.stake;
        a.stake = 0;
        if (amt > 0) {
            reserved -= amt;
            emit StakeReclaimed(id, a.publisher, amt);
            wane.safeTransfer(a.publisher, amt);
        }
    }

    function claimRewards() external nonReentrant {
        uint256 amt = earned[msg.sender];
        earned[msg.sender] = 0;
        if (amt > 0) {
            reserved -= amt;
            emit RewardClaimed(msg.sender, amt);
            wane.safeTransfer(msg.sender, amt);
        }
    }

    /// @notice Optional metered check fee, split 80/20 publisher/treasury.
    ///         Basic check() stays free; this is for premium/high-frequency.
    function payCheck(
        uint64 id
    ) external nonReentrant notPaused {
        if (checkFee == 0) return;
        WaneTypes.Antibody storage a = antibodies[id];
        if (a.id == 0) revert Unknown();
        if (a.status != WaneTypes.Status.Active) revert NotActive();
        uint96 fee = checkFee;
        wane.safeTransferFrom(msg.sender, address(this), fee);
        reserved += fee;
        uint256 toPub = (uint256(fee) * publisherBps) / 10_000;
        // genesis antibodies are protocol-owned; route their share to treasury
        address pub = a.publisher == address(this) ? treasury : a.publisher;
        earned[pub] += toPub;
        earned[treasury] += fee - toPub;
    }

    /* ?? Genesis seeding (cold-start kill) ????????????????????????????? */

    function seedGenesis(
        WaneTypes.ThreatKind[] calldata kinds,
        bytes32[] calldata subjects,
        bytes32[] calldata evidence
    ) external onlyGovernor {
        if (!genesisOpen) revert GenesisClosed();
        uint256 n = kinds.length;
        if (subjects.length != n || evidence.length != n) revert LengthMismatch();
        for (uint256 i; i < n;) {
            bytes32 key = _key(kinds[i], subjects[i]);
            if (idByKey[key] == 0) {
                uint64 id = ++antibodyCount;
                antibodies[id] = WaneTypes.Antibody({
                    id: id,
                    kind: kinds[i],
                    status: WaneTypes.Status.Active,
                    publisher: address(this), // protocol-owned, stake 0 => enforces immediately
                    stake: 0,
                    mintedBlock: uint64(block.number),
                    corroborations: 0,
                    subject: subjects[i],
                    evidence: evidence[i]
                });
                idByKey[key] = id;
                emit GenesisSeeded(id, kinds[i], subjects[i]);
            }
            unchecked {
                ++i;
            }
        }
    }

    function closeGenesis() external onlyGovernor {
        genesisOpen = false;
        emit GenesisClosedEvent();
    }

    /* ?? admin ????????????????????????????????????????????????????????? */

    function setPaused(
        bool p
    ) external onlyGovernor {
        paused = p;
        emit PausedSet(p);
    }

    /// @dev two-step governor transfer (no fat-finger to address(0)).
    function transferGovernor(
        address next
    ) external onlyGovernor {
        if (next == address(0)) revert ZeroAddress();
        pendingGovernor = next;
        emit GovernorTransferStarted(next);
    }

    function acceptGovernor() external {
        if (msg.sender != pendingGovernor) revert NotPending();
        governor = msg.sender;
        pendingGovernor = address(0);
        emit GovernorTransferred(msg.sender);
    }

    function setTreasury(
        address t
    ) external onlyGovernor {
        if (t == address(0)) revert ZeroAddress();
        treasury = t;
    }

    function setParams(
        uint96 mintStake_,
        uint96 challengeStake_,
        uint64 maturity_,
        uint16 publisherBps_,
        uint96 checkFee_
    ) external onlyGovernor {
        if (publisherBps_ > 10_000) revert BadParams();
        mintStake = mintStake_;
        challengeStake = challengeStake_;
        maturity = maturity_;
        publisherBps = publisherBps_;
        checkFee = checkFee_;
        emit ParamsSet(mintStake_, challengeStake_, maturity_, publisherBps_, checkFee_);
    }

    function setEnforcement(
        uint64 window_,
        uint32 corrobs_
    ) external onlyGovernor {
        enforceWindow = window_;
        enforceCorrobs = corrobs_;
        emit WindowSet(window_, corrobs_);
    }

    /* ?? views ????????????????????????????????????????????????????????? */

    /// @notice Surplus WANE not backing any liability (excess can't strand claims).
    function surplus() external view returns (uint256) {
        uint256 bal = wane.balanceOf(address(this));
        return bal > reserved ? bal - reserved : 0;
    }

    /* ?? internal ?????????????????????????????????????????????????????? */

    function _key(
        WaneTypes.ThreatKind kind,
        bytes32 subject
    ) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked(kind, subject));
    }
}
