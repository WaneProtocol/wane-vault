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