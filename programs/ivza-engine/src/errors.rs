use anchor_lang::prelude::*;

/// Custom error codes for the IVZA parallel execution engine.
#[error_code]
pub enum EngineError {
    /// 6000 - The submitted graph is structurally invalid.
    #[msg("The submitted graph is structurally invalid")]
    InvalidGraph,

    /// 6001 - The graph exceeds the maximum allowed size.
    #[msg("The graph exceeds the maximum allowed size")]
    GraphTooLarge,

    /// 6002 - The signer is not authorized to perform this action.
    #[msg("Unauthorized: signer does not have permission")]
    Unauthorized,

    /// 6003 - This lane has already been executed.
    #[msg("This lane has already been executed")]
    AlreadyExecuted,

    /// 6004 - The lane index is out of range for this graph.
    #[msg("Invalid lane index: out of range for this graph")]
    InvalidLaneIndex,

    /// 6005 - The graph is not in a state that allows this operation.
    #[msg("Graph is not ready for this operation")]
    GraphNotReady,

    /// 6006 - Lane execution failed.
    #[msg("Lane execution failed")]
    ExecutionFailed,

    /// 6007 - The engine is currently paused.
    #[msg("The engine is currently paused")]
    Paused,

    /// 6008 - The fee value is invalid (exceeds maximum).
    #[msg("Invalid fee: exceeds maximum allowed basis points")]
    InvalidFee,

    /// 6009 - The graph exceeds the maximum node limit.
    #[msg("Node count exceeds the configured maximum")]
    NodeLimitExceeded,

    /// 6010 - The graph exceeds the maximum edge limit.
    #[msg("Edge count exceeds the configured maximum")]
    EdgeLimitExceeded,

    /// 6011 - The graph contains a cycle and cannot be executed in parallel.
    #[msg("Graph contains a cycle: parallel execution is not possible")]
    CyclicGraph,

    /// 6012 - Not all lanes have been executed; settlement is not possible.
    #[msg("Not all lanes executed: cannot settle")]
    IncompleteLanes,

    /// 6013 - The graph has already been settled or has failed.
    #[msg("Graph is already in a terminal state")]
    AlreadyTerminal,

    /// 6014 - The lane count must be at least 1.
    #[msg("Lane count must be at least 1")]
    InvalidLaneCount,

    /// 6015 - Node data exceeds maximum allowed length.
    #[msg("Node data exceeds maximum allowed length")]
    NodeDataTooLarge,

    /// 6016 - Edge data exceeds maximum allowed length.
    #[msg("Edge data exceeds maximum allowed length")]
    EdgeDataTooLarge,

    /// 6017 - Edge references a node index that does not exist.
    #[msg("Edge references a non-existent node")]
    InvalidEdgeReference,

    /// 6018 - Duplicate edge detected.
    #[msg("Duplicate edge detected in graph")]
    DuplicateEdge,

    /// 6019 - Max lanes configuration is invalid.
    #[msg("Max lanes value is out of allowed range")]
    InvalidMaxLanes,

    /// 6020 - Arithmetic overflow during computation.
    #[msg("Arithmetic overflow")]
    ArithmeticOverflow,
}
