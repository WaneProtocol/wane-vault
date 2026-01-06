use anchor_lang::prelude::*;

use crate::errors::EngineError;
use crate::events::ConfigUpdated;
use crate::state::*;

/// Accounts required for the `initialize` instruction.
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// The authority that will govern engine configuration.
    #[account(mut)]
    pub authority: Signer<'info>,

    /// The engine configuration PDA. Created once; subsequent calls will fail.
    #[account(
        init,
        payer = authority,
        space = EngineConfig::LEN,
        seeds = [SEED_ENGINE_CONFIG],
        bump,
    )]
    pub engine_config: Account<'info, EngineConfig>,

    /// System program for account creation.
    pub system_program: Program<'info, System>,
}

/// Handler for the `initialize` instruction.
///
/// Sets up the singleton `EngineConfig` account with the provided parameters.
/// Validates that fee_bps does not exceed the maximum and that max_lanes is
/// within allowed bounds.
pub fn handler(
    ctx: Context<Initialize>,
    fee_bps: u16,
    max_nodes_per_graph: u16,
    max_lanes: u8,
) -> Result<()> {
    // Validate fee is within allowed range.
    require!(fee_bps <= MAX_FEE_BPS, EngineError::InvalidFee);

    // Validate max_nodes_per_graph is positive and within hard limit.
    require!(
        max_nodes_per_graph > 0 && max_nodes_per_graph <= MAX_GRAPH_NODES,
        EngineError::NodeLimitExceeded
    );

    // Validate max_lanes is within allowed range.
    require!(
        max_lanes > 0 && max_lanes <= MAX_LANES,
        EngineError::InvalidMaxLanes
    );

    let config = &mut ctx.accounts.engine_config;

    config.authority = ctx.accounts.authority.key();
    config.fee_bps = fee_bps;
    config.max_nodes_per_graph = max_nodes_per_graph;
    config.max_lanes = max_lanes;
    config.paused = false;
    config.total_graphs = 0;
    config.total_settled = 0;
    config.total_compute_units = 0;
    config.bump = ctx.bumps.engine_config;
    config._reserved = [0u8; 64];

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
        "iVZA Engine initialized: fee={}bps, max_nodes={}, max_lanes={}",
        fee_bps,
        max_nodes_per_graph,
        max_lanes
    );

    Ok(())
}
