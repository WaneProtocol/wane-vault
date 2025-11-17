use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;

use crate::graph::GraphNode;
use crate::types::NodeId;

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