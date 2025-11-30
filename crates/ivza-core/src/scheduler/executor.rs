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
            let mut level_nodes = par_level.nodes.clone();
            level_nodes.sort_by(|a, b| {
                let pri_a = priorities.get(a).map(|p| p.priority).unwrap_or(0);
                let pri_b = priorities.get(b).map(|p| p.priority).unwrap_or(0);
                pri_b.cmp(&pri_a)
            });

            // Greedy bin-packing: for each node, try to fit it into an existing
            // lane for this level. If no lane fits, create a new one.
            let mut level_lanes: Vec<ExecutionLane> = Vec::new();

            for &node_id in &level_nodes {
                let node = graph
                    .nodes
                    .get(&node_id)
                    .ok_or_else(|| anyhow!("Node {} not found in graph", node_id))?;

                let mut placed = false;
                for lane in &mut level_lanes {
                    if lane.width() >= self.max_lane_width {
                        continue;
                    }
                    if lane.total_cu + node.estimated_cu > self.max_lane_cu {
                        continue;
                    }
                    if lane.can_add(node) {
                        lane.add_node(node);
                        placed = true;
                        debug!(
                            "Placed node {} into lane {} (level {})",
                            node_id, lane.index, par_level.level
                        );
                        break;
                    }
                }

                if !placed {
                    let mut new_lane = ExecutionLane::new(lane_idx);
                    new_lane.add_node(node);
                    debug!(
                        "Created lane {} for node {} (level {})",
                        lane_idx, node_id, par_level.level
                    );
                    level_lanes.push(new_lane);
                    lane_idx += 1;
                }
            }

            plan.lanes.extend(level_lanes);
        }

        // Build the assignment map.
        for lane in &plan.lanes {
            for (pos, &node_id) in lane.node_ids.iter().enumerate() {
                plan.assignments.push(LaneAssignment {
                    node_id,
                    lane_index: lane.index,
                    position_in_lane: pos,
                });
            }
        }

        plan.finalize();

        info!(
            "Execution plan: {} lanes, {} txs, max_par={}, avg_par={:.2}",
            plan.num_lanes(),
            plan.total_transactions,
            plan.max_parallelism,
            plan.avg_parallelism()
        );

        Ok(plan)
    }

    /// Create an optimized plan that attempts to merge lanes across levels
    /// when no dependencies exist between them.
    pub fn plan_optimized(&self, graph: &TransactionGraph) -> Result<ExecutionPlan> {
        let base_plan = self.plan(graph)?;

        if base_plan.lanes.len() <= 1 {
            return Ok(base_plan);
        }

        info!(
            "Optimizing execution plan with {} lanes",
            base_plan.lanes.len()
        );

        // Try to merge consecutive lanes if they don't conflict.
        let mut merged_lanes: Vec<ExecutionLane> = Vec::new();
        let mut current_lane = base_plan.lanes[0].clone();

        for i in 1..base_plan.lanes.len() {
            let next = &base_plan.lanes[i];
            let can_merge = self.can_merge_lanes(&current_lane, next, graph);

            if can_merge
                && current_lane.width() + next.width() <= self.max_lane_width
                && current_lane.total_cu + next.total_cu <= self.max_lane_cu
            {
                // Merge: add all nodes from next into current.
                for &node_id in &next.node_ids {
                    if let Some(node) = graph.nodes.get(&node_id) {
                        current_lane.add_node(node);
                    }
                }
                debug!(
                    "Merged lane {} into lane {}",
                    next.index, current_lane.index
                );
            } else {
                merged_lanes.push(current_lane);
                current_lane = next.clone();
            }
        }
        merged_lanes.push(current_lane);

        // Re-index lanes.
        for (i, lane) in merged_lanes.iter_mut().enumerate() {
            lane.index = i;
        }

        let mut plan = ExecutionPlan::new();
        plan.lanes = merged_lanes;

        // Rebuild assignments.
        for lane in &plan.lanes {
            for (pos, &node_id) in lane.node_ids.iter().enumerate() {
                plan.assignments.push(LaneAssignment {
                    node_id,
                    lane_index: lane.index,
                    position_in_lane: pos,
                });
            }
        }

        plan.finalize();

        info!(
            "Optimized plan: {} lanes (from {}), max_par={}",
            plan.num_lanes(),
            base_plan.lanes.len(),
            plan.max_parallelism
        );

        Ok(plan)
    }

    /// Check if two lanes can be merged (no dependencies between their nodes).
    fn can_merge_lanes(
        &self,
        lane_a: &ExecutionLane,
        lane_b: &ExecutionLane,
        graph: &TransactionGraph,
    ) -> bool {
        // Check that no node in lane_b depends on any node in lane_a, or vice versa.
        let a_nodes: HashSet<NodeId> = lane_a.node_ids.iter().copied().collect();
        let b_nodes: HashSet<NodeId> = lane_b.node_ids.iter().copied().collect();

        for edge in &graph.edges {
            let from_in_a = a_nodes.contains(&edge.from);
            let to_in_b = b_nodes.contains(&edge.to);
            let from_in_b = b_nodes.contains(&edge.from);
            let to_in_a = a_nodes.contains(&edge.to);

            if (from_in_a && to_in_b) || (from_in_b && to_in_a) {
                return false;
            }
        }

        // Also check account conflicts between the two lanes.
        for &b_id in &lane_b.node_ids {
            if let Some(b_node) = graph.nodes.get(&b_id) {
                if !lane_a.can_add(b_node) {
                    return false;
                }
            }
        }

        true
    }
}

impl Default for ExecutionPlanner {
    fn default() -> Self {
        Self::new()
    }
}
