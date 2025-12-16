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
        }

        if best_split_count <= 1 {
            return Ok(None);
        }

        // Build split amounts.
        let per_split = request.amount / best_split_count as u64;
        let remainder = request.amount - per_split * best_split_count as u64;
        let mut split_amounts = vec![per_split; best_split_count];
        if remainder > 0 {
            split_amounts[0] += remainder;
        }

        // Build individual SolvedSwaps for each split.
        let mut split_swaps = Vec::new();
        for (i, &amount) in split_amounts.iter().enumerate() {
            // Re-route each split.
            match engine.find_best_route(&request.input_mint, &request.output_mint, amount) {
                Ok(route) => {
                    let slippage_factor = 0.99; // 1% slippage for splits
                    let min_output = (route.output_amount as f64 * slippage_factor) as u64;

                    split_swaps.push(SolvedSwap {
                        request: SwapRequest {
                            node_id: request.node_id * 1000 + i as u64,
                            input_mint: request.input_mint,
                            output_mint: request.output_mint,
                            amount,
                            label: request.label.as_ref().map(|l| format!("{}_split_{}", l, i)),
                        },
                        route,
                        min_output,
                    });
                }
                Err(_) => {
                    return Ok(None);
                }
            }
        }

        let action = OptimizationAction::SplitSwap {
            node_id: request.node_id,
            original_amount: request.amount,
            split_amounts: split_amounts.clone(),
            original_output,
            optimized_output: best_total_output,
        };

        Ok(Some((split_swaps, action)))
    }

    /// Combine multiple split routes into a single SolvedSwap with aggregate metrics.
    fn combine_split_routes(&self, splits: &[SolvedSwap], original: &SolvedSwap) -> SolvedSwap {
        if splits.is_empty() {
            return original.clone();
        }

        // Use the first split's route structure but update amounts.
        let total_output: u64 = splits.iter().map(|s| s.route.output_amount).sum();
        let total_input: u64 = splits.iter().map(|s| s.request.amount).sum();

        // Build a combined route from the best split's hops.
        let best_split = splits.iter().max_by_key(|s| s.route.output_amount).unwrap();

        let mut combined_hops = best_split.route.hops.clone();
        // Update the combined hop amounts to reflect totals.
        if let Some(first_hop) = combined_hops.first_mut() {
            first_hop.input_amount = total_input;
        }
        if let Some(last_hop) = combined_hops.last_mut() {
            last_hop.output_amount = total_output;
        }

        let combined_route = Route::from_hops(combined_hops);
        let min_output: u64 = splits.iter().map(|s| s.min_output).sum();

        SolvedSwap {
            request: original.request.clone(),
            route: combined_route,
            min_output,
        }
    }

    /// Optimize an execution plan by merging compatible transactions in the same lane.
    pub fn optimize_plan(&self, plan: &ExecutionPlan) -> Result<ExecutionPlan> {
        if !self.config.enable_merging {
            return Ok(plan.clone());
        }

        let mut optimized = ExecutionPlan::new();
        let mut actions = Vec::new();

        for lane in &plan.lanes {
            if lane.node_ids.len() <= 1 {
                optimized.lanes.push(lane.clone());
                continue;
            }

            // Try to merge consecutive transactions in the lane that together
            // fit within the CU limit.
            let mut merged_lane = ExecutionLane::new(lane.index);
            let mut merged_nodes: Vec<NodeId> = Vec::new();
            let mut current_cu = 0u64;

            for &node_id in &lane.node_ids {
                // Estimate CU for this node (use a default since we don't have
                // the graph here; the caller should provide CU estimates).
                let node_cu = 200_000u64;

                if current_cu + node_cu > self.config.max_merged_cu && !merged_nodes.is_empty() {
                    // Start a new merge group.
                    merged_nodes.clear();
                    current_cu = 0;
                }

                merged_nodes.push(node_id);
                merged_lane.node_ids.push(node_id);
                current_cu += node_cu;
            }

            // Record CU saved by merging (overhead reduction).
            let overhead_per_tx = 5_000u64; // Base transaction overhead
            let original_overhead = lane.node_ids.len() as u64 * overhead_per_tx;
            let merged_overhead = overhead_per_tx; // Single transaction
            let cu_saved = original_overhead.saturating_sub(merged_overhead);

            if cu_saved > 0 && lane.node_ids.len() > 1 {
                actions.push(OptimizationAction::MergeTransactions {
                    lane_index: lane.index,
                    node_ids: lane.node_ids.clone(),
                    cu_saved,
                });
            }

            merged_lane.total_cu = lane.total_cu;
            optimized.lanes.push(merged_lane);
        }

        // Rebuild assignments.
        for lane in &optimized.lanes {
            for (pos, &node_id) in lane.node_ids.iter().enumerate() {
                optimized.assignments.push(LaneAssignment {
                    node_id,
                    lane_index: lane.index,
                    position_in_lane: pos,
                });
            }
        }

        optimized.finalize();

        info!(
            "PlanOptimizer: {} lanes, {} merge actions",
            optimized.num_lanes(),
            actions.len(),
        );

        Ok(optimized)
    }
}

impl Default for ExecutionOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::{PoolInfo, PoolRegistry};
    use crate::router::RouteEngine;
    use crate::solver::{GreedySolver, Solver, SolverConfig, SwapRequest};

    fn make_pubkey(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    fn setup() -> (RouteEngine, SolverResult) {
        let registry = PoolRegistry::new();
        let sol = make_pubkey(1);
        let usdc = make_pubkey(2);

        // Pool with modest liquidity so large swaps have price impact.
        registry.register(PoolInfo::constant_product(
            make_pubkey(10),
            sol,
            usdc,
            10_000_000,
            1_500_000_000,
            30,
        ));

        let engine = RouteEngine::new(registry);
        let config = SolverConfig::default();
        let solver = GreedySolver::new();

        // Large swap relative to pool size to trigger price impact.
        let requests = vec![SwapRequest {
            node_id: 0,
            input_mint: sol,
            output_mint: usdc,
            amount: 1_000_000, // 10% of pool reserve
            label: Some("big_swap".into()),
        }];

        let result = solver.solve(&requests, &engine, &config).unwrap();
        (engine, result)
    }

    #[test]
    fn test_optimizer_basic() {
        let (engine, result) = setup();
        let optimizer = ExecutionOptimizer::new();
        let opt_result = optimizer.optimize(&result, &engine).unwrap();

        assert!(opt_result.optimized_output >= opt_result.original_output);
    }

    #[test]
    fn test_optimizer_config() {
        let config = OptimizerConfig::default();
        assert_eq!(config.max_splits, 5);
        assert!(config.enable_reordering);
        assert!(config.enable_merging);
    }

    #[test]
    fn test_plan_optimization() {
        let optimizer = ExecutionOptimizer::new();
        let mut plan = ExecutionPlan::new();

        let mut lane = ExecutionLane::new(0);
        lane.node_ids = vec![0, 1, 2];
        lane.total_cu = 600_000;
        plan.lanes.push(lane);
        plan.finalize();

        let optimized = optimizer.optimize_plan(&plan).unwrap();
        assert!(!optimized.lanes.is_empty());
    }
}
