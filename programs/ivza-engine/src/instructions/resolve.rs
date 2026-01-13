use anchor_lang::prelude::*;

use crate::errors::EngineError;
use crate::events::{ConfigUpdated, GraphSettled};
use crate::state::*;

// ---------------------------------------------------------------------------
// Settle
// ---------------------------------------------------------------------------

/// Accounts required for the `settle` instruction.
#[derive(Accounts)]
#[instruction(graph_id: [u8; 32])]
pub struct Settle<'info> {
    /// The owner of the graph, who receives any leftover funds.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// The engine configuration.
    #[account(
        mut,
        seeds = [SEED_ENGINE_CONFIG],
        bump = engine_config.bump,
    )]
    pub engine_config: Account<'info, EngineConfig>,

    /// The transaction graph being settled.
    #[account(
        mut,
        seeds = [SEED_GRAPH, graph_id.as_ref()],
        bump = graph.bump,
        has_one = owner @ EngineError::Unauthorized,
    )]
    pub graph: Account<'info, TransactionGraph>,

    /// System program.
    pub system_program: Program<'info, System>,
}

/// Handler for the `settle` instruction.
///
/// Finalizes a graph after all lanes have been executed. Updates global stats
/// and marks the graph as settled (or failed if any lane failed).
pub fn settle_handler(ctx: Context<Settle>, graph_id: [u8; 32]) -> Result<()> {
    let config = &mut ctx.accounts.engine_config;
    let graph = &mut ctx.accounts.graph;

    // Graph must not already be in a terminal state.
    require!(!graph.status.is_terminal(), EngineError::AlreadyTerminal);

    // All lanes must have been executed.
    require!(graph.all_lanes_executed(), EngineError::IncompleteLanes);

    let clock = Clock::get()?;

    // Determine final status: settled if all lanes succeeded, failed otherwise.
    let final_status = if graph.lanes_failed > 0 {
        GraphStatus::Failed
    } else {
        GraphStatus::Settled
    };

    graph.status = final_status;

    // Update global engine statistics.
    config.total_settled = config
        .total_settled
        .checked_add(1)
        .ok_or(EngineError::ArithmeticOverflow)?;

    config.total_compute_units = config
        .total_compute_units
        .checked_add(graph.total_compute_units)
        .ok_or(EngineError::ArithmeticOverflow)?;

    let status_byte = match final_status {
        GraphStatus::Pending => 0u8,
        GraphStatus::Executing => 1u8,
        GraphStatus::Settled => 2u8,
        GraphStatus::Failed => 3u8,
    };

    emit!(GraphSettled {
        graph_id,
        owner: graph.owner,
        total_compute_units: graph.total_compute_units,
        lanes_succeeded: graph.lanes_succeeded,
        lanes_failed: graph.lanes_failed,
        final_status: status_byte,
        settled_at: clock.unix_timestamp,
    });

    msg!(
        "Graph settled: status={}, total_cu={}, succeeded={}, failed={}",
        status_byte,
        graph.total_compute_units,
        graph.lanes_succeeded,
        graph.lanes_failed,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// UpdateConfig
// ---------------------------------------------------------------------------

/// Accounts required for the `update_config` instruction.
#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    /// The current authority. Must match the stored authority in config.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// The engine configuration to update.
    #[account(
        mut,
        seeds = [SEED_ENGINE_CONFIG],
        bump = engine_config.bump,
        has_one = authority @ EngineError::Unauthorized,
    )]
    pub engine_config: Account<'info, EngineConfig>,
}

/// Handler for the `update_config` instruction.
///
/// Allows the authority to update fee, node limits, lane limits, or
/// pause/unpause the engine. Only provided `Some` values are applied.
pub fn update_config_handler(
    ctx: Context<UpdateConfig>,
    new_fee_bps: Option<u16>,
    new_max_nodes: Option<u16>,
    new_max_lanes: Option<u8>,
    paused: Option<bool>,
) -> Result<()> {
    let config = &mut ctx.accounts.engine_config;

    // Update fee if provided.
    if let Some(fee) = new_fee_bps {
        require!(fee <= MAX_FEE_BPS, EngineError::InvalidFee);
        config.fee_bps = fee;
    }

    // Update max nodes if provided.
    if let Some(max_nodes) = new_max_nodes {
        require!(
            max_nodes > 0 && max_nodes <= MAX_GRAPH_NODES,
            EngineError::NodeLimitExceeded
        );
        config.max_nodes_per_graph = max_nodes;
    }

    // Update max lanes if provided.
    if let Some(lanes) = new_max_lanes {
        require!(
            lanes > 0 && lanes <= MAX_LANES,
            EngineError::InvalidMaxLanes
        );
        config.max_lanes = lanes;
    }

    // Update paused state if provided.
    if let Some(p) = paused {
        config.paused = p;
    }

    let clock = Clock::get()?;

    emit!(ConfigUpdated {
        authority: config.authority,
        fee_bps: config.fee_bps,
        max_nodes_per_graph: config.max_nodes_per_graph,
        max_lanes: config.max_lanes,
        paused: config.paused,
        updated_at: clock.unix_timestamp,
    });

    msg!(
        "Config updated: fee={}bps, max_nodes={}, max_lanes={}, paused={}",
        config.fee_bps,
        config.max_nodes_per_graph,
        config.max_lanes,
        config.paused,
    );

    Ok(())
}
