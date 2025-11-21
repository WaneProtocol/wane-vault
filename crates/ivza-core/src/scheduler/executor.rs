use std::collections::HashSet;

use anyhow::{anyhow, Result};
use tracing::{debug, info};

use crate::analyzer::ParallelismAnalyzer;
use crate::graph::TransactionGraph;
use crate::types::NodeId;

use super::lane::{ExecutionLane, ExecutionPlan, LaneAssignment};
use super::priority::PriorityScheduler;

/// Produces an ExecutionPlan from an analyzed transaction graph.
///
/// The planner works in three stages:
/// 1. Compute node priorities using PriorityScheduler.
/// 2. Group nodes into parallel levels using topological layering.
/// 3. Within each level, pack nodes into lanes using a greedy bin-packing
///    algorithm that respects account conflict constraints.
///
/// The goal is to minimize the total number of lanes (maximize parallelism
/// within each lane) while ensuring correctness (no conflicting transactions
/// in the same lane).
pub struct ExecutionPlanner {
    /// The priority scheduler used to order nodes.
    pub priority_scheduler: PriorityScheduler,
    /// Maximum number of transactions per lane (e.g., limited by Solana block limits).
    pub max_lane_width: usize,
    /// Maximum CU per lane.
    pub max_lane_cu: u64,
}

impl ExecutionPlanner {
    pub fn new() -> Self {
        Self {
            priority_scheduler: PriorityScheduler::new(),
            max_lane_width: 64,
            max_lane_cu: 48_000_000, // Solana block limit approximation.
        }
    }

    pub fn with_max_lane_width(mut self, max: usize) -> Self {
        self.max_lane_width = max;
        self
    }

    pub fn with_max_lane_cu(mut self, max_cu: u64) -> Self {
        self.max_lane_cu = max_cu;
        self
    }

    pub fn with_priority_scheduler(mut self, scheduler: PriorityScheduler) -> Self {
        self.priority_scheduler = scheduler;
        self
    }

    /// Create an execution plan from the given graph.
    pub fn plan(&self, graph: &TransactionGraph) -> Result<ExecutionPlan> {
        if graph.node_count() == 0 {
            return Ok(ExecutionPlan::new());
        }

        info!(
            "Planning execution for {} nodes, {} edges",
            graph.node_count(),
            graph.edge_count()
        );

        // Step 1: Compute parallel levels (topological layers).
        let analyzer = ParallelismAnalyzer::new();
        let par_result = analyzer.analyze(graph)?;

        // Step 2: Compute priorities for ordering within levels.
        let priorities = self.priority_scheduler.compute_priorities(graph)?;

        // Step 3: For each parallel level, pack nodes into lanes.
        let mut plan = ExecutionPlan::new();
        let mut lane_idx = 0;

        for par_level in &par_result.levels {
            // Sort nodes within this level by priority (highest first).