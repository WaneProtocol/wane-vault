//! Solver trait and implementations for finding optimal execution routes.
//!
//! Provides:
//! - `Solver` trait: interface for route-finding strategies.
//! - `GreedySolver`: fast solver that picks the best route for each node independently.
//! - `BranchAndBoundSolver`: optimal solver that minimizes total cost across the plan.
//! - `SolverConfig`: shared configuration for all solver implementations.

use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info, warn};

use ivza_core::scheduler::ExecutionPlan;
use ivza_core::types::NodeId;

use crate::router::{Route, RouteEngine};

/// Configuration shared by all solver implementations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverConfig {
    /// Maximum number of routes to evaluate per node.
    pub max_routes: usize,
    /// Solver timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum acceptable slippage in basis points.
    pub slippage_bps: u16,
    /// Minimum output ratio (output/input) below which a route is rejected.
    pub min_output_ratio: f64,
    /// Whether to allow multi-hop routes.
    pub allow_multi_hop: bool,
    /// Maximum number of hops per route.
    pub max_hops: usize,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            max_routes: 10,
            timeout_ms: 5_000,
            slippage_bps: 100, // 1%
            min_output_ratio: 0.90,
            allow_multi_hop: true,
            max_hops: 4,
        }
    }
}

/// Description of a swap that a solver should find routes for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRequest {
    /// Node in the execution plan this swap belongs to.
    pub node_id: NodeId,
    /// Input token mint.
    pub input_mint: Pubkey,
    /// Output token mint.
    pub output_mint: Pubkey,
    /// Input amount.
    pub amount: u64,
    /// Optional label for debugging.
    pub label: Option<String>,
}

/// Result of solving a single swap request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolvedSwap {
    /// The original swap request.
    pub request: SwapRequest,
    /// The chosen route.
    pub route: Route,
    /// Minimum acceptable output after slippage.
    pub min_output: u64,
}

/// Overall result from the solver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverResult {
    /// Solved routes for each swap request, keyed by node ID.
    pub solved_swaps: HashMap<NodeId, SolvedSwap>,
    /// Total estimated input across all swaps.
    pub total_input: u64,
    /// Total estimated output across all swaps.
    pub total_output: u64,
    /// Total estimated cost in lamports (fees + rent).
    pub estimated_cost_lamports: u64,
    /// Number of swap requests that could not be routed.
    pub failed_count: usize,
    /// Time taken to solve, in milliseconds.
    pub solve_time_ms: u64,
}

impl SolverResult {
    /// Aggregate output/input ratio.
    pub fn output_ratio(&self) -> f64 {
        if self.total_input == 0 {
            return 0.0;
        }
        self.total_output as f64 / self.total_input as f64
    }

    /// Returns true if all swaps were successfully routed.
    pub fn all_solved(&self) -> bool {
        self.failed_count == 0
    }
}

impl fmt::Display for SolverResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SolverResult(swaps={}, failed={}, in={}, out={}, cost={}, time={}ms)",
            self.solved_swaps.len(),
            self.failed_count,
            self.total_input,
            self.total_output,
            self.estimated_cost_lamports,
            self.solve_time_ms,
        )
    }
}

/// Trait for solver implementations.
pub trait Solver {
    /// Solve the given swap requests and return routes.
    fn solve(
        &self,
        requests: &[SwapRequest],
        engine: &RouteEngine,
        config: &SolverConfig,
    ) -> Result<SolverResult>;

    /// Name of the solver strategy.
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// GreedySolver
// ---------------------------------------------------------------------------

/// Greedy solver: for each swap request, independently pick the best route.
///
/// Fast (O(n * k) where k is the number of candidate routes per request),
/// but does not consider interactions between swaps (e.g., shared pool liquidity).
pub struct GreedySolver;

impl GreedySolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GreedySolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for GreedySolver {
    fn name(&self) -> &str {
        "GreedySolver"
    }

    fn solve(
        &self,
        requests: &[SwapRequest],
        engine: &RouteEngine,
        config: &SolverConfig,
    ) -> Result<SolverResult> {
        let start = Instant::now();
        let deadline = start + std::time::Duration::from_millis(config.timeout_ms);

        let mut solved_swaps = HashMap::new();
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut failed_count = 0usize;

        for request in requests {
            if Instant::now() > deadline {
                warn!(
                    "GreedySolver: timeout after {}ms, {} of {} solved",
                    config.timeout_ms,
                    solved_swaps.len(),
                    requests.len()
                );
                failed_count += requests.len() - solved_swaps.len();
                break;
            }

            match engine.find_routes(&request.input_mint, &request.output_mint, request.amount) {
                Ok(routes) => {
                    // Filter by config constraints.
                    let valid_routes: Vec<Route> = routes
                        .into_iter()
                        .filter(|r| {
                            if !config.allow_multi_hop && r.hop_count() > 1 {
                                return false;
                            }
                            if r.hop_count() > config.max_hops {
                                return false;
                            }
                            let ratio = r.exchange_rate();
                            if ratio < config.min_output_ratio {
                                return false;
                            }
                            true
                        })
                        .take(config.max_routes)
                        .collect();

                    if let Some(best) = valid_routes.into_iter().next() {
                        let slippage_factor =
                            (10_000u64 - config.slippage_bps as u64) as f64 / 10_000.0;
                        let min_output = (best.output_amount as f64 * slippage_factor) as u64;

                        total_input += request.amount;
                        total_output += best.output_amount;

                        debug!(
                            "GreedySolver: node {} -> {} hops, out={}, min_out={}",
                            request.node_id,
                            best.hop_count(),
                            best.output_amount,
                            min_output,
                        );

                        solved_swaps.insert(
                            request.node_id,
                            SolvedSwap {
                                request: request.clone(),
                                route: best,
                                min_output,
                            },
                        );
                    } else {
                        warn!(
                            "GreedySolver: no valid route for node {} ({} -> {})",
                            request.node_id, request.input_mint, request.output_mint,
                        );
                        failed_count += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "GreedySolver: route finding failed for node {}: {}",
                        request.node_id, e
                    );
                    failed_count += 1;
                }
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;

        // Estimate cost: 5000 lamports base + 200 lamports per hop.
        let total_hops: usize = solved_swaps
            .values()
            .map(|s| s.route.hop_count())
            .sum();
        let estimated_cost = 5_000u64 * solved_swaps.len() as u64 + 200 * total_hops as u64;

        info!(
            "GreedySolver: solved {}/{} swaps in {}ms",
            solved_swaps.len(),
            requests.len(),
            elapsed,
        );

        Ok(SolverResult {
            solved_swaps,
            total_input,
            total_output,
            estimated_cost_lamports: estimated_cost,
            failed_count,
            solve_time_ms: elapsed,
        })
    }
}

// ---------------------------------------------------------------------------
