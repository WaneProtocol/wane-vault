use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod state;

use instructions::*;

declare_id!("DiiCy7G6p5QVvGcAWfwA6m5EjyPeD5BRnEit9ersPUq2");

#[program]
pub mod ivza_engine {
    use super::*;

    /// Initialize the engine configuration account.
    /// Only callable once by the deployer to set up initial parameters.
    pub fn initialize(
        ctx: Context<Initialize>,
        fee_bps: u16,
        max_nodes_per_graph: u16,
        max_lanes: u8,
    ) -> Result<()> {
        instructions::initialize::handler(ctx, fee_bps, max_nodes_per_graph, max_lanes)
    }

    /// Submit a new transaction graph for parallel execution.
    /// The graph is validated for correctness (no cycles, valid edges, within limits).
    pub fn submit_graph(
        ctx: Context<SubmitGraph>,
        graph_id: [u8; 32],
        nodes_data: Vec<u8>,
        edges_data: Vec<u8>,
        node_count: u16,
        edge_count: u16,
        lane_count: u8,
    ) -> Result<()> {
        instructions::submit_graph::handler(
            ctx, graph_id, nodes_data, edges_data, node_count, edge_count, lane_count,
        )
    }

    /// Execute a specific lane of a previously submitted graph.
    /// Each lane represents a parallelizable subset of the graph's transactions.
    pub fn execute_lane(
        ctx: Context<ExecuteLane>,
        graph_id: [u8; 32],
        lane_index: u8,
        compute_units_used: u64,
        success: bool,
    ) -> Result<()> {
        instructions::execute::handler(ctx, graph_id, lane_index, compute_units_used, success)
    }

    /// Settle a fully executed graph.
    /// All lanes must be executed before settlement. Releases funds and finalizes status.
    pub fn settle(ctx: Context<Settle>, graph_id: [u8; 32]) -> Result<()> {
        instructions::resolve::settle_handler(ctx, graph_id)
    }

    /// Update the engine configuration.
    /// Only callable by the current authority.
    pub fn update_config(
        ctx: Context<UpdateConfig>,
        new_fee_bps: Option<u16>,
        new_max_nodes: Option<u16>,
        new_max_lanes: Option<u8>,
        paused: Option<bool>,
    ) -> Result<()> {
        instructions::resolve::update_config_handler(
            ctx,
            new_fee_bps,
            new_max_nodes,
            new_max_lanes,
            paused,
        )
    }
}
