use anchor_lang::prelude::*;

use crate::errors::EngineError;
use crate::events::LaneExecuted;
use crate::state::*;

/// Accounts required for the `execute_lane` instruction.
#[derive(Accounts)]
#[instruction(graph_id: [u8; 32], lane_index: u8)]
pub struct ExecuteLane<'info> {
    /// The executor signing the lane execution.
    #[account(mut)]
    pub executor: Signer<'info>,

    /// The engine configuration (must not be paused).
    #[account(
        seeds = [SEED_ENGINE_CONFIG],
        bump = engine_config.bump,
    )]
    pub engine_config: Account<'info, EngineConfig>,

    /// The transaction graph being executed.
    #[account(
        mut,
        seeds = [SEED_GRAPH, graph_id.as_ref()],
        bump = graph.bump,
    )]
    pub graph: Account<'info, TransactionGraph>,

    /// The execution record PDA for this lane.
    #[account(
        init,
        payer = executor,
        space = ExecutionRecord::LEN,
        seeds = [SEED_EXECUTION, graph_id.as_ref(), &[lane_index]],
        bump,
    )]
    pub execution_record: Account<'info, ExecutionRecord>,

    /// System program for account creation.
    pub system_program: Program<'info, System>,
}

/// Handler for the `execute_lane` instruction.
///
/// Records the execution of a specific lane within a graph. Updates the graph
/// status and lane bitmask accordingly.
pub fn handler(
    ctx: Context<ExecuteLane>,
    graph_id: [u8; 32],
    lane_index: u8,
    compute_units_used: u64,
    success: bool,
) -> Result<()> {
    let config = &ctx.accounts.engine_config;
    let graph = &mut ctx.accounts.graph;
    let record = &mut ctx.accounts.execution_record;

    // Engine must not be paused.
    require!(!config.paused, EngineError::Paused);

    // Graph must accept new lane executions.
    require!(graph.status.accepts_execution(), EngineError::GraphNotReady);

    // Validate lane index is within range.
    require!(lane_index < graph.lane_count, EngineError::InvalidLaneIndex);

    // Check lane has not already been executed.
    require!(
        !graph.is_lane_executed(lane_index),
        EngineError::AlreadyExecuted
    );

    let clock = Clock::get()?;

    // Record the execution details.
    record.graph_id = graph_id;
    record.lane_index = lane_index;
    record.executor = ctx.accounts.executor.key();
    record.executed_at = clock.unix_timestamp;
    record.success = success;
    record.compute_units_used = compute_units_used;
    record.bump = ctx.bumps.execution_record;
    record._reserved = [0u8; 16];

    // Update graph state.
    graph.mark_lane_executed(lane_index);
    graph.total_compute_units = graph
        .total_compute_units
        .checked_add(compute_units_used)
        .ok_or(EngineError::ArithmeticOverflow)?;

    if success {
        graph.lanes_succeeded += 1;
    } else {
        graph.lanes_failed += 1;
    }

    // Transition graph status.
    if graph.status == GraphStatus::Pending {
        graph.status = GraphStatus::Executing;
    }

    // If any lane failed, mark the graph as failed immediately.
    if !success {
        graph.status = GraphStatus::Failed;
    }

    let lanes_completed = graph.executed_lane_count();

    emit!(LaneExecuted {
        graph_id,
        lane_index,
        executor: record.executor,
        success,
        compute_units_used,
        executed_at: clock.unix_timestamp,
        lanes_completed,
        lanes_total: graph.lane_count,
    });

    msg!(
        "Lane {} executed: success={}, cu={}, progress={}/{}",
        lane_index,
        success,
        compute_units_used,
        lanes_completed,
        graph.lane_count,
    );

    Ok(())
}
