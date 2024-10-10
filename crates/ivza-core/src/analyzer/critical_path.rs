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
        let mut pairs: Vec<(NodeId, f64)> = self
            .timings
            .iter()
            .map(|(&id, t)| (id, t.slack))
            .collect();
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
    /// If true, use estimated CU as duration. If false, use uniform duration of 1.0.
    pub use_cu_as_duration: bool,
}

impl CriticalPathAnalyzer {
    pub fn new() -> Self {
        Self {
            use_cu_as_duration: true,
        }
    }

    pub fn with_uniform_duration(mut self) -> Self {
        self.use_cu_as_duration = false;
        self
    }

    /// Run the critical path analysis on the given graph.
    pub fn analyze(&self, graph: &TransactionGraph) -> Result<CriticalPathResult> {
        let topo_order = graph
            .topological_sort()
            .ok_or_else(|| anyhow!("Cannot compute critical path: graph has a cycle"))?;

        if topo_order.is_empty() {
            return Ok(CriticalPathResult {
                timings: HashMap::new(),
                critical_path: Vec::new(),
                makespan: 0.0,
                critical_cu: 0,
            });
        }

        info!(
            "Computing critical path for {} nodes",
            topo_order.len()
        );

        // Compute duration for each node.
        let durations: HashMap<NodeId, f64> = graph
            .nodes
            .iter()
            .map(|(&id, node)| {
                let dur = if self.use_cu_as_duration {
                    node.estimated_cu as f64
                } else {
                    1.0
                };
                (id, dur)
            })
            .collect();

        // Forward pass: compute earliest start and finish.
        let mut earliest_start: HashMap<NodeId, f64> = HashMap::new();
        let mut earliest_finish: HashMap<NodeId, f64> = HashMap::new();

        for &node_id in &topo_order {
            let preds = graph.predecessors(node_id);
            let es = if preds.is_empty() {
                0.0
            } else {
                preds
                    .iter()
                    .map(|&p| earliest_finish.get(&p).copied().unwrap_or(0.0))
                    .fold(0.0_f64, f64::max)
            };
            let dur = durations[&node_id];
            earliest_start.insert(node_id, es);
            earliest_finish.insert(node_id, es + dur);
        }

        // Project finish time is the max earliest finish.
        let makespan = earliest_finish.values().copied().fold(0.0_f64, f64::max);

        