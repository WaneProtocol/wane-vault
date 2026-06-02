// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

/// @title WaneTypes
/// @notice Shared types for the Wane antibody registry. An "antibody" is an
///         on-chain memory of a threat: once one agent is attacked, the threat
///         is recorded here and flows through the network so every other agent
///         is immune. The threat wanes because antibodies circulate.
library WaneTypes {
    /// @notice What kind of threat an antibody recognizes.
    enum ThreatKind {
        Address, //  0  a specific wallet / contract (drainer, malicious tool endpoint)
        CallPattern, //  1  a calldata selector + shape used to drain
        Bytecode, //  2  a contract codehash (re-deployed drainers)
        Semantic //  3  a prompt-injection / tool-poisoning marker hash
    }

    /// @notice Lifecycle of an antibody.
    enum Status {
        None, //  0  never minted
        Active, //  1  live, enforced on check()
        Challenged, //  2  someone staked against it; under dispute
        Revoked //  3  proven false; minter slashed
    }

    /// @notice A single on-chain antibody.
    /// @dev key = keccak256(kind, subject). subject is the abi-encoded target
    ///      (address, selector, codehash, or semantic hash) depending on kind.
    struct Antibody {
        uint64 id; //          monotonic id, also the human "WANE-YYYY-NNNN" anchor
        ThreatKind kind; //    threat category
        Status status; //      lifecycle
        address publisher; //  who minted it (earns reward share)
        uint96 stake; //       $WANE locked by publisher (slashable)
        uint64 mintedBlock; //  when minted
        uint32 corroborations; // independent confirmations (raises trust)
        bytes32 subject; //    the thing being flagged (see key note)
        bytes32 evidence; //   hash of off-chain evidence (tx, payload, report)
    }
}
