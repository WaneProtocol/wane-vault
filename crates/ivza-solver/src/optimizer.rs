//! Execution plan optimizer.
//!
//! Takes a solved execution plan and applies optimizations:
//! - Split large swaps across multiple routes to reduce price impact.
//! - Merge compatible transactions in the same lane to save compute units.
//! - Reorder transactions within lanes for optimal execution sequence.

use std::fmt;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use ivza_core::scheduler::{ExecutionLane, ExecutionPlan, LaneAssignment};
use ivza_core::types::NodeId;

use crate::pool::calculate_output;
use crate::router::{Route, RouteEngine};
use crate::solver::{SolvedSwap, SolverResult, SwapRequest};

/// Configuration for the optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    /// Minimum price impact (fraction) at which to consider splitting a swap.
    pub split_threshold: f64,
    /// Maximum number of splits for a single swap.
    pub max_splits: usize,
    /// Minimum amount per split (to avoid dust).
    pub min_split_amount: u64,
    /// Whether to reorder transactions within lanes.
    pub enable_reordering: bool,
    /// Whether to merge compatible transactions.
    pub enable_merging: bool,
    /// Maximum CU per merged transaction.
    pub max_merged_cu: u64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            split_threshold: 0.01, // 1% price impact triggers split
            max_splits: 5,
            min_split_amount: 1_000,
            enable_reordering: true,
            enable_merging: true,
            max_merged_cu: 1_400_000, // Solana transaction CU limit
        }
    }
}

/// Describes a single optimization action taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationAction {
    /// Split a large swap into multiple smaller ones.
    SplitSwap {
        node_id: NodeId,
        original_amount: u64,
        split_amounts: Vec<u64>,
        original_output: u64,
        optimized_output: u64,
    },
    /// Merge two transactions in the same lane.
    MergeTransactions {
        lane_index: usize,
        node_ids: Vec<NodeId>,
        cu_saved: u64,
    },
    /// Reorder transactions within a lane.
    ReorderLane {
        lane_index: usize,
        original_order: Vec<NodeId>,
        new_order: Vec<NodeId>,
    },
}

impl fmt::Display for OptimizationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptimizationAction::SplitSwap {
                node_id,
                original_amount,
                split_amounts,
                original_output,
                optimized_output,
            } => {
                let improvement = if *original_output > 0 {
                    (*optimized_output as f64 / *original_output as f64 - 1.0) * 100.0
                } else {
                    0.0
                };
                write!(
                    f,
                    "SplitSwap(node={}, {} -> {:?}, output +{:.2}%)",
                    node_id, original_amount, split_amounts, improvement,
                )
            }
            OptimizationAction::MergeTransactions {
                lane_index,
                node_ids,
                cu_saved,
            } => {
                write!(
                    f,
                    "MergeTransactions(lane={}, nodes={:?}, saved={} CU)",
                    lane_index, node_ids, cu_saved,
                )
            }
            OptimizationAction::ReorderLane {
                lane_index,
                original_order,
                new_order,
            } => {
                write!(
                    f,
                    "ReorderLane(lane={}, {:?} -> {:?})",
                    lane_index, original_order, new_order,
                )
            }
        }
    }
}

/// Result of optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    /// Original total estimated cost (output lost to price impact, in input token units).
    pub original_cost: u64,
    /// Optimized total estimated cost.
    pub optimized_cost: u64,
    /// Savings in basis points.
    pub savings_bps: u16,
    /// Original total output.
    pub original_output: u64,
    /// Optimized total output.
    pub optimized_output: u64,
    /// Actions taken.
    pub actions: Vec<OptimizationAction>,
    /// Optimized solver result (with split/merged routes).
    pub optimized_result: SolverResult,
}

impl OptimizationResult {
    /// Returns the absolute output improvement.
    pub fn output_improvement(&self) -> u64 {
        self.optimized_output.saturating_sub(self.original_output)
    }

    /// Returns the improvement as a percentage.
    pub fn improvement_pct(&self) -> f64 {
        if self.original_output == 0 {
            return 0.0;
        }
        (self.optimized_output as f64 / self.original_output as f64 - 1.0) * 100.0
    }
}

impl fmt::Display for OptimizationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OptimizationResult(original_out={}, optimized_out={}, improvement={:.2}%, {} actions)",
            self.original_output,
            self.optimized_output,
            self.improvement_pct(),
            self.actions.len(),
        )
    }
}

/// The main execution optimizer.
pub struct ExecutionOptimizer {
    pub config: OptimizerConfig,
}

impl ExecutionOptimizer {
    pub fn new() -> Self {
        Self {
            config: OptimizerConfig::default(),
        }
    }

    pub fn with_config(mut self, config: OptimizerConfig) -> Self {
        self.config = config;
        self
    }

    /// Optimize a solver result: split large swaps, merge transactions, reorder lanes.
    pub fn optimize(
        &self,
        solver_result: &SolverResult,
        engine: &RouteEngine,
    ) -> Result<OptimizationResult> {
        let mut actions = Vec::new();
        let mut optimized_swaps = solver_result.solved_swaps.clone();
        let original_output = solver_result.total_output;

        // Phase 1: Split large swaps with high price impact.
        for (node_id, solved) in &solver_result.solved_swaps {
            if solved.route.total_price_impact >= self.config.split_threshold {
                match self.try_split_swap(solved, engine) {
                    Ok(Some((split_swaps, action))) => {
                        // Replace the single swap with the best split.
                        // The split_swaps are individual SolvedSwaps; we keep the one
                        // with the highest aggregate output.
                        let total_split_output: u64 =
                            split_swaps.iter().map(|s| s.route.output_amount).sum();

                        if total_split_output > solved.route.output_amount {
                            // Build a combined route that represents the split.
                            let combined = self.combine_split_routes(&split_swaps, solved);
                            optimized_swaps.insert(*node_id, combined);
                            actions.push(action);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        debug!("Split optimization failed for node {}: {}", node_id, e);
                    }
                }
            }
        }

        // Phase 2: Reorder within lanes (highest output first for priority fee optimization).
        if self.config.enable_reordering {
            // Group swaps by notional lane assignment (we use node_id ordering as proxy).
            let mut node_ids: Vec<NodeId> = optimized_swaps.keys().copied().collect();
            node_ids.sort();

            // Reorder by descending output amount (higher-value swaps first).
            let mut sorted_ids = node_ids.clone();
            sorted_ids.sort_by(|a, b| {
                let out_a = optimized_swaps
                    .get(a)
                    .map(|s| s.route.output_amount)
                    .unwrap_or(0);
                let out_b = optimized_swaps
                    .get(b)
                    .map(|s| s.route.output_amount)
                    .unwrap_or(0);
                out_b.cmp(&out_a)
            });

            if sorted_ids != node_ids && !node_ids.is_empty() {
                actions.push(OptimizationAction::ReorderLane {
                    lane_index: 0,
                    original_order: node_ids,
                    new_order: sorted_ids,
                });
            }
        }

        // Phase 3: Compute optimized totals.
        let optimized_output: u64 = optimized_swaps
            .values()
            .map(|s| s.route.output_amount)
            .sum();
        let optimized_input: u64 = optimized_swaps.values().map(|s| s.request.amount).sum();

        // Cost = input - output (the "loss" from fees and impact).
        let original_cost = solver_result
            .total_input
            .saturating_sub(solver_result.total_output);
        let optimized_cost = optimized_input.saturating_sub(optimized_output);

        let savings_bps = if original_cost > 0 {
            ((original_cost.saturating_sub(optimized_cost) as f64 / original_cost as f64)
                * 10_000.0) as u16
        } else {
            0
        };

        let total_hops: usize = optimized_swaps.values().map(|s| s.route.hop_count()).sum();
        let estimated_cost = 5_000u64 * optimized_swaps.len() as u64 + 200 * total_hops as u64;

        let optimized_result = SolverResult {
            solved_swaps: optimized_swaps,
            total_input: optimized_input,
            total_output: optimized_output,
            estimated_cost_lamports: estimated_cost,
            failed_count: solver_result.failed_count,
            solve_time_ms: solver_result.solve_time_ms,
        };

        info!(
            "Optimizer: original_out={}, optimized_out={}, savings={}bps, {} actions",
            original_output,
            optimized_output,
            savings_bps,
            actions.len(),
        );

        Ok(OptimizationResult {
            original_cost,
            optimized_cost,
            savings_bps,
            original_output,
            optimized_output,
            actions,
            optimized_result,
        })
    }

    /// Try to split a swap across multiple routes to reduce price impact.
    fn try_split_swap(
        &self,
        solved: &SolvedSwap,
        engine: &RouteEngine,
    ) -> Result<Option<(Vec<SolvedSwap>, OptimizationAction)>> {
        let request = &solved.request;
        let original_output = solved.route.output_amount;

        // Find the pools used by the current route.
        let pool_data: Vec<(u64, u64, u16)> = solved
            .route
            .hops
            .iter()
            .filter_map(|hop| {
                engine.registry.get(&hop.pool_address).map(|pool| {
                    let (res_in, res_out) = if hop.input_mint == pool.token_a {
                        (pool.reserve_a, pool.reserve_b)
                    } else {
                        (pool.reserve_b, pool.reserve_a)
                    };
                    (res_in, res_out, pool.fee_bps)
                })
            })
            .collect();

        if pool_data.is_empty() {
            return Ok(None);
        }

        // For multi-hop routes, splitting is more complex.  We only split
        // single-hop routes or use the first hop's pool data as proxy.
        let primary_pool = pool_data[0];

        // Try splitting across 2..=max_splits identical pools.
        let mut best_total_output = original_output;
        let mut best_split_count = 1usize;

        for n in 2..=self.config.max_splits {
            let per_split = request.amount / n as u64;
            if per_split < self.config.min_split_amount {
                break;
            }

            // Simulate splitting into n equal parts through the same pool.
            let mut total_out = 0u64;
            for _ in 0..n {
                let out =
                    calculate_output(primary_pool.0, primary_pool.1, per_split, primary_pool.2);
                total_out = total_out.saturating_add(out);
            }
            // Handle remainder.
            let remainder = request.amount - per_split * n as u64;
            if remainder > 0 {
                let out =
                    calculate_output(primary_pool.0, primary_pool.1, remainder, primary_pool.2);
                total_out = total_out.saturating_add(out);
            }

            if total_out > best_total_output {
                best_total_output = total_out;
                best_split_count = n;
            }