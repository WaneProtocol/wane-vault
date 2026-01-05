use anchor_lang::prelude::*;

/// Emitted when a new transaction graph is submitted for parallel execution.
#[event]
pub struct GraphSubmitted {
    /// Unique identifier for the graph.
    pub graph_id: [u8; 32],

    /// The owner / submitter of the graph.
    pub owner: Pubkey,

    /// Number of nodes in the graph.
    pub node_count: u16,

    /// Number of edges in the graph.
    pub edge_count: u16,

    /// Number of parallel lanes the graph is divided into.
    pub lane_count: u8,

    /// Unix timestamp of submission.
    pub created_at: i64,
}

/// Emitted when a specific lane of a graph is executed.
#[event]
pub struct LaneExecuted {
    /// The graph this lane belongs to.
    pub graph_id: [u8; 32],

    /// Index of the executed lane.
    pub lane_index: u8,

    /// The executor who ran the lane.
    pub executor: Pubkey,

    /// Whether execution succeeded.
    pub success: bool,

    /// Compute units consumed.
    pub compute_units_used: u64,

    /// Unix timestamp of execution.
    pub executed_at: i64,

    /// Number of lanes executed so far (including this one).
    pub lanes_completed: u8,

    /// Total lanes in the graph.
    pub lanes_total: u8,
}

/// Emitted when a graph is fully settled after all lanes complete.
#[event]
pub struct GraphSettled {
    /// The settled graph identifier.
    pub graph_id: [u8; 32],

    /// The owner of the graph.
    pub owner: Pubkey,

    /// Total compute units consumed across all lanes.
    pub total_compute_units: u64,

    /// Number of lanes that succeeded.
    pub lanes_succeeded: u8,

    /// Number of lanes that failed.
    pub lanes_failed: u8,

    /// Final status of the graph.
    pub final_status: u8,

    /// Unix timestamp of settlement.
    pub settled_at: i64,
}

/// Emitted when the engine configuration is updated.
#[event]
pub struct ConfigUpdated {
    /// The authority that made the update.
    pub authority: Pubkey,

    /// New fee in basis points (if changed).
    pub fee_bps: u16,

    /// New max nodes per graph (if changed).
    pub max_nodes_per_graph: u16,

    /// New max lanes (if changed).
    pub max_lanes: u8,

    /// Whether the engine is paused.
    pub paused: bool,

    /// Unix timestamp of the update.
    pub updated_at: i64,
}
