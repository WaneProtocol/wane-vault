use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tracing::{debug, info};

use crate::graph::TransactionGraph;
use crate::types::NodeId;

/// Timing information computed for each node during critical path analysis.
#[derive(Debug, Clone)]
pub struct NodeTiming {
    /// Node ID.
    pub node_id: NodeId,
    /// Earliest start time (forward pass).
    pub earliest_start: f64,
    /// Earliest finish time (earliest_start + duration).
    pub earliest_finish: f64,
    /// Latest start time (backward pass).
    pub latest_start: f64,
    /// Latest finish time.
    pub latest_finish: f64,
    /// Slack (latest_start - earliest_start). Zero means on the critical path.
    pub slack: f64,
    /// Duration of this node (estimated CU as proxy for time).
    pub duration: f64,
}

impl NodeTiming {
    /// Returns true if this node is on the critical path (zero slack).
    pub fn is_critical(&self) -> bool {
        self.slack.abs() < 1e-9
    }
}

/// Result of a critical path analysis.
#[derive(Debug, Clone)]
pub struct CriticalPathResult {
    /// Timing for every node.
    pub timings: HashMap<NodeId, NodeTiming>,
    /// Nodes on the critical path, in order.
    pub critical_path: Vec<NodeId>,
    /// Total duration of the critical path (makespan).
    pub makespan: f64,
    /// Total estimated CU on the critical path.
    pub critical_cu: u64,
}

impl CriticalPathResult {
    /// Returns the slack for a given node.
    pub fn slack(&self, node_id: NodeId) -> Option<f64> {
        self.timings.get(&node_id).map(|t| t.slack)
    }

    /// Returns whether a node is on the critical path.
    pub fn is_critical(&self, node_id: NodeId) -> bool {
        self.critical_path.contains(&node_id)
    }

    /// Returns nodes sorted by slack (ascending).
    pub fn nodes_by_slack(&self) -> Vec<(NodeId, f64)> {
        let mut pairs: Vec<(NodeId, f64)> =
            self.timings.iter().map(|(&id, t)| (id, t.slack)).collect();
        pairs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        pairs
    }
}

/// Analyzes the critical path through a transaction DAG.
///
/// Uses the standard CPM (Critical Path Method) algorithm:
/// 1. Forward pass: compute earliest start/finish times.
/// 2. Backward pass: compute latest start/finish times.
/// 3. Slack computation: latest_start - earliest_start.
/// 4. Critical path: nodes with zero slack.
pub struct CriticalPathAnalyzer {