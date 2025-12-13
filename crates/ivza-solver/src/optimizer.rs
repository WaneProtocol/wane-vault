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