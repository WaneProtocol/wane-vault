use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::graph::GraphNode;
use crate::types::{AccountSet, NodeId};

/// An execution lane: a set of non-conflicting transactions that can execute in parallel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLane {
    /// Lane index within the execution plan.
    pub index: usize,
    /// Node IDs assigned to this lane.
    pub node_ids: Vec<NodeId>,
    /// Combined write set of all nodes in this lane.
    #[serde(skip)]
    pub combined_writes: HashSet<Pubkey>,
    /// Combined read set of all nodes in this lane.
    #[serde(skip)]
    pub combined_reads: HashSet<Pubkey>,
    /// Total estimated CU for this lane.
    pub total_cu: u64,
}

impl ExecutionLane {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            node_ids: Vec::new(),
            combined_writes: HashSet::new(),
            combined_reads: HashSet::new(),
            total_cu: 0,
        }
    }

    /// Check if a node can be added to this lane without conflicts.
    /// A node conflicts if it writes to any account already accessed (read or write)
    /// by the lane, or reads any account already written by the lane.
    pub fn can_add(&self, node: &GraphNode) -> bool {
        // Check: node's writes vs lane's reads and writes.
        for w in &node.account_set.writes {
            if self.combined_reads.contains(w) || self.combined_writes.contains(w) {
                return false;
            }
        }
        // Check: node's reads vs lane's writes.
        for r in &node.account_set.reads {
            if self.combined_writes.contains(r) {
                return false;
            }
        }
        true
    }

    /// Add a node to this lane. Caller must ensure can_add() returned true.
    pub fn add_node(&mut self, node: &GraphNode) {
        self.node_ids.push(node.id);
        self.combined_writes.extend(&node.account_set.writes);
        self.combined_reads.extend(&node.account_set.reads);
        self.total_cu += node.estimated_cu;
    }

    /// Returns the number of transactions in this lane.
    pub fn width(&self) -> usize {
        self.node_ids.len()
    }

    /// Returns true if the lane has no transactions.
    pub fn is_empty(&self) -> bool {
        self.node_ids.is_empty()
    }
}

/// Assignment of a node to a lane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaneAssignment {
    pub node_id: NodeId,
    pub lane_index: usize,
    pub position_in_lane: usize,
}

/// An execution plan: an ordered sequence of lanes that execute sequentially.
/// Each lane contains transactions that are independent and can execute in parallel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Lanes in execution order.
    pub lanes: Vec<ExecutionLane>,
    /// Complete assignment map.
    pub assignments: Vec<LaneAssignment>,
    /// Total estimated CU across all lanes.
    pub total_cu: u64,
    /// Maximum parallelism (width of the widest lane).
    pub max_parallelism: usize,
    /// Total number of transactions.
    pub total_transactions: usize,
}

impl ExecutionPlan {
    pub fn new() -> Self {
        Self {
            lanes: Vec::new(),
            assignments: Vec::new(),
            total_cu: 0,
            max_parallelism: 0,
            total_transactions: 0,
        }
    }

    /// Compute summary statistics from the lanes.
    pub fn finalize(&mut self) {
        self.total_cu = self.lanes.iter().map(|l| l.total_cu).sum();
        self.max_parallelism = self.lanes.iter().map(|l| l.width()).max().unwrap_or(0);
        self.total_transactions = self.lanes.iter().map(|l| l.width()).sum();
    }

    /// Returns the number of lanes (sequential steps).
    pub fn num_lanes(&self) -> usize {
        self.lanes.len()
    }

    /// Returns the average parallelism.
    pub fn avg_parallelism(&self) -> f64 {
        if self.lanes.is_empty() {
            return 0.0;
        }
        self.total_transactions as f64 / self.lanes.len() as f64
    }

    /// Returns the estimated makespan: sum of max CU per lane.
    pub fn estimated_makespan(&self) -> u64 {
        self.lanes.iter().map(|l| l.total_cu).sum()
    }

    /// Returns the sequential cost (sum of all CUs).
    pub fn sequential_cost(&self) -> u64 {
        self.total_cu
    }

    /// Returns the speedup ratio.
    pub fn speedup(&self) -> f64 {
        let makespan = self.estimated_makespan();
        if makespan == 0 {
            return 1.0;
        }
        self.total_cu as f64 / makespan as f64
    }

    /// Returns a human-readable summary of the execution plan.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "ExecutionPlan: {} lanes, {} transactions, {} total CU\n",
            self.num_lanes(),
            self.total_transactions,
            self.total_cu
        );
        s.push_str(&format!(
            "  Max parallelism: {}, Avg parallelism: {:.2}\n",
            self.max_parallelism,
            self.avg_parallelism()
        ));
        for lane in &self.lanes {
            s.push_str(&format!(
                "  Lane {}: {} txs, {} CU, nodes: {:?}\n",
                lane.index,
                lane.width(),
                lane.total_cu,
                lane.node_ids
            ));
        }
        s
    }
}

impl Default for ExecutionPlan {
    fn default() -> Self {
        Self::new()
    }
}
