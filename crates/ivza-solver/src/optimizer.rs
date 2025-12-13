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