use anchor_lang::prelude::*;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of nodes allowed in a single transaction graph.
pub const MAX_GRAPH_NODES: u16 = 256;

/// Maximum number of edges allowed in a single transaction graph.
pub const MAX_EDGES: u16 = 1024;

/// Maximum number of parallel lanes a graph can be split into.
pub const MAX_LANES: u8 = 32;

/// Maximum byte size for serialized node data.
pub const MAX_NODES_DATA_LEN: usize = 8192;

/// Maximum byte size for serialized edge data.
pub const MAX_EDGES_DATA_LEN: usize = 4096;

/// Maximum number of execution records per graph.
pub const MAX_EXECUTION_RECORDS: usize = 32;

/// Seed prefix for the engine configuration PDA.
pub const SEED_ENGINE_CONFIG: &[u8] = b"engine_config";

/// Seed prefix for transaction graph PDAs.
pub const SEED_GRAPH: &[u8] = b"graph";

/// Seed prefix for execution record PDAs.
pub const SEED_EXECUTION: &[u8] = b"execution";

/// Seed prefix for the vault PDA.
pub const SEED_VAULT: &[u8] = b"vault";

/// Basis points denominator (100% = 10_000 bps).
pub const BPS_DENOMINATOR: u16 = 10_000;

/// Maximum fee in basis points (50%).
pub const MAX_FEE_BPS: u16 = 5_000;

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Status of a transaction graph throughout its lifecycle.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GraphStatus {
    /// Graph has been submitted and validated but not yet executing.
    Pending = 0,
    /// At least one lane has begun execution.
    Executing = 1,
    /// All lanes executed and the graph has been settled.
    Settled = 2,
    /// One or more lanes failed during execution.
    Failed = 3,
}

impl Default for GraphStatus {
    fn default() -> Self {
        GraphStatus::Pending
    }
}

impl GraphStatus {
    /// Returns true if the graph is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, GraphStatus::Settled | GraphStatus::Failed)
    }

    /// Returns true if new lanes can still be executed.
    pub fn accepts_execution(&self) -> bool {
        matches!(self, GraphStatus::Pending | GraphStatus::Executing)
    }
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

/// Global engine configuration. Singleton PDA derived from SEED_ENGINE_CONFIG.
#[account]
#[derive(Debug)]
pub struct EngineConfig {
    /// The authority that can update configuration and pause the engine.
    pub authority: Pubkey,

    /// Fee in basis points charged per graph settlement.
    pub fee_bps: u16,

    /// Maximum number of nodes allowed per graph.
    pub max_nodes_per_graph: u16,

    /// Maximum number of parallel lanes allowed.
    pub max_lanes: u8,

    /// Whether the engine is currently paused.
    pub paused: bool,

    /// Total number of graphs submitted since deployment.
    pub total_graphs: u64,

    /// Total number of graphs successfully settled.
    pub total_settled: u64,

    /// Total compute units consumed across all settled graphs.
    pub total_compute_units: u64,

    /// Bump seed for PDA derivation.
    pub bump: u8,

    /// Reserved space for future upgrades.
    pub _reserved: [u8; 64],
}

impl EngineConfig {
    /// Account discriminator (8) + fields.
    pub const LEN: usize = 8  // discriminator
        + 32  // authority
        + 2   // fee_bps
        + 2   // max_nodes_per_graph
        + 1   // max_lanes
        + 1   // paused
        + 8   // total_graphs
        + 8   // total_settled
        + 8   // total_compute_units
        + 1   // bump
        + 64; // _reserved
}

/// A submitted transaction graph awaiting parallel execution.
#[account]
#[derive(Debug)]
pub struct TransactionGraph {
    /// Unique 32-byte identifier for this graph.
    pub graph_id: [u8; 32],

    /// The owner / submitter of the graph.
    pub owner: Pubkey,

    /// Serialized node data (application-specific encoding).
    pub nodes_data: Vec<u8>,

    /// Serialized edge data (application-specific encoding).
    pub edges_data: Vec<u8>,

    /// Number of nodes in the graph.
    pub node_count: u16,

    /// Number of edges in the graph.
    pub edge_count: u16,

    /// Current status of the graph.
    pub status: GraphStatus,

    /// Unix timestamp when the graph was created.
    pub created_at: i64,

    /// Number of parallel lanes this graph is split into.
    pub lane_count: u8,

    /// Bitmask of lanes that have been executed (up to 32 lanes).
    pub lanes_executed: u32,

    /// Number of lanes that completed successfully.
    pub lanes_succeeded: u8,

    /// Number of lanes that failed.
    pub lanes_failed: u8,

    /// Total compute units consumed across all executed lanes.
    pub total_compute_units: u64,

    /// Bump seed for PDA derivation.
    pub bump: u8,

    /// Reserved space for future upgrades.
    pub _reserved: [u8; 32],
}

impl TransactionGraph {
    /// Compute the space needed for a graph with given data sizes.
    pub fn space(nodes_data_len: usize, edges_data_len: usize) -> usize {
        8       // discriminator
        + 32    // graph_id
        + 32    // owner
        + 4 + nodes_data_len  // nodes_data (vec prefix + data)
        + 4 + edges_data_len  // edges_data (vec prefix + data)
        + 2     // node_count
        + 2     // edge_count
        + 1     // status
        + 8     // created_at
        + 1     // lane_count
        + 4     // lanes_executed
        + 1     // lanes_succeeded
        + 1     // lanes_failed
        + 8     // total_compute_units
        + 1     // bump
        + 32    // _reserved
    }

    /// Check whether a specific lane has been executed.
    pub fn is_lane_executed(&self, lane_index: u8) -> bool {
        self.lanes_executed & (1u32 << lane_index) != 0
    }

    /// Mark a specific lane as executed.
    pub fn mark_lane_executed(&mut self, lane_index: u8) {
        self.lanes_executed |= 1u32 << lane_index;
    }

    /// Returns true if all lanes have been executed.
    pub fn all_lanes_executed(&self) -> bool {
        if self.lane_count == 0 {
            return true;
        }
        let mask = if self.lane_count >= 32 {
            u32::MAX
        } else {
            (1u32 << self.lane_count) - 1
        };
        (self.lanes_executed & mask) == mask
    }

    /// Returns the number of lanes that have been executed so far.
    pub fn executed_lane_count(&self) -> u8 {
        self.lanes_executed.count_ones() as u8
    }
}

/// Record of a single lane execution within a graph.
#[account]
#[derive(Debug)]
pub struct ExecutionRecord {
    /// The graph this execution belongs to.
    pub graph_id: [u8; 32],

    /// Index of the lane that was executed.
    pub lane_index: u8,

    /// The executor (signer) who ran this lane.
    pub executor: Pubkey,

    /// Unix timestamp when the lane was executed.
    pub executed_at: i64,

    /// Whether the lane execution succeeded.
    pub success: bool,

    /// Compute units consumed by this lane.
    pub compute_units_used: u64,

    /// Bump seed for PDA derivation.
    pub bump: u8,

    /// Reserved space.
    pub _reserved: [u8; 16],
}

impl ExecutionRecord {
    pub const LEN: usize = 8  // discriminator
        + 32  // graph_id
        + 1   // lane_index
        + 32  // executor
        + 8   // executed_at
        + 1   // success
        + 8   // compute_units_used
        + 1   // bump
        + 16; // _reserved
}
