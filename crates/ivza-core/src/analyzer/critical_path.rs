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

        // Backward pass: compute latest start and finish.
        let mut latest_start: HashMap<NodeId, f64> = HashMap::new();
        let mut latest_finish: HashMap<NodeId, f64> = HashMap::new();

        for &node_id in topo_order.iter().rev() {
            let succs = graph.successors(node_id);
            let lf = if succs.is_empty() {
                makespan
            } else {
                succs
                    .iter()
                    .map(|&s| latest_start.get(&s).copied().unwrap_or(makespan))
                    .fold(f64::MAX, f64::min)
            };
            let dur = durations[&node_id];
            latest_finish.insert(node_id, lf);
            latest_start.insert(node_id, lf - dur);
        }

        // Compute slack and build timings.
        let mut timings: HashMap<NodeId, NodeTiming> = HashMap::new();
        for &node_id in &topo_order {
            let es = earliest_start[&node_id];
            let ef = earliest_finish[&node_id];
            let ls = latest_start[&node_id];
            let lf = latest_finish[&node_id];
            let slack = ls - es;

            timings.insert(
                node_id,
                NodeTiming {
                    node_id,
                    earliest_start: es,
                    earliest_finish: ef,
                    latest_start: ls,
                    latest_finish: lf,
                    slack,
                    duration: durations[&node_id],
                },
            );

            debug!(
                "Node {}: ES={:.0}, EF={:.0}, LS={:.0}, LF={:.0}, slack={:.0}",
                node_id, es, ef, ls, lf, slack
            );
        }

        // Extract the critical path (nodes with zero slack), in topological order.
        let critical_path: Vec<NodeId> = topo_order
            .iter()
            .filter(|&&id| timings[&id].is_critical())
            .copied()
            .collect();

        let critical_cu: u64 = critical_path
            .iter()
            .filter_map(|id| graph.nodes.get(id))
            .map(|n| n.estimated_cu)
            .sum();

        info!(
            "Critical path: {} nodes, makespan={:.0}, critical_cu={}",
            critical_path.len(),
            makespan,
            critical_cu
        );

        Ok(CriticalPathResult {
            timings,
            critical_path,
            makespan,
            critical_cu,
        })
    }

    /// Compute the depth (longest path from any root) for each node.
    pub fn compute_depths(&self, graph: &TransactionGraph) -> Result<HashMap<NodeId, u32>> {
        let topo_order = graph
            .topological_sort()
            .ok_or_else(|| anyhow!("Graph has a cycle"))?;

        let mut depths: HashMap<NodeId, u32> = HashMap::new();

        for &node_id in &topo_order {
            let preds = graph.predecessors(node_id);
            let depth = if preds.is_empty() {
                0
            } else {
                preds
                    .iter()
                    .map(|&p| depths.get(&p).copied().unwrap_or(0) + 1)
                    .max()
                    .unwrap_or(0)
            };
            depths.insert(node_id, depth);
        }

        Ok(depths)
    }
}

impl Default for CriticalPathAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
