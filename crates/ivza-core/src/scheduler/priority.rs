use std::collections::HashMap;

use anyhow::Result;
use tracing::{debug, info};

use crate::analyzer::{CriticalPathAnalyzer, CriticalPathResult};
use crate::graph::TransactionGraph;
use crate::types::NodeId;

/// Priority information for a node.
#[derive(Debug, Clone)]
pub struct NodePriority {
    pub node_id: NodeId,
    /// Final computed priority (higher = schedule first).
    pub priority: i64,
    /// Component from critical path analysis (0 to 100).
    pub critical_score: i64,
    /// Component from CU cost (normalized 0 to 50).
    pub cu_score: i64,
    /// Component from dependency depth (deeper = higher priority, 0 to 30).
    pub depth_score: i64,
    /// Whether this node is on the critical path.
    pub is_critical: bool,
}

/// Assigns priorities to graph nodes based on:
/// - Critical path analysis (nodes on the critical path get highest priority).
/// - Estimated CU cost (more expensive nodes scheduled first to reduce makespan).
/// - Dependency depth (deeper nodes need to start sooner).
pub struct PriorityScheduler {
    /// Weight for critical path component.
    pub critical_weight: f64,
    /// Weight for CU component.
    pub cu_weight: f64,
    /// Weight for depth component.
    pub depth_weight: f64,
}

impl PriorityScheduler {
    pub fn new() -> Self {
        Self {
            critical_weight: 100.0,
            cu_weight: 50.0,
            depth_weight: 30.0,
        }
    }

    pub fn with_weights(mut self, critical: f64, cu: f64, depth: f64) -> Self {
        self.critical_weight = critical;
        self.cu_weight = cu;
        self.depth_weight = depth;
        self
    }

    /// Compute priorities for all nodes in the graph.
    pub fn compute_priorities(
        &self,
        graph: &TransactionGraph,
    ) -> Result<HashMap<NodeId, NodePriority>> {
        info!(
            "Computing priorities for {} nodes",
            graph.node_count()
        );

        // Run critical path analysis.
        let cpa = CriticalPathAnalyzer::new();
        let cp_result = cpa.analyze(graph)?;
        let depths = cpa.compute_depths(graph)?;

        // Find max CU and max depth for normalization.
        let max_cu = graph
            .nodes
            .values()
            .map(|n| n.estimated_cu)
            .max()
            .unwrap_or(1) as f64;
        let max_depth = depths.values().copied().max().unwrap_or(1) as f64;

        let mut priorities = HashMap::new();

        for &node_id in graph.nodes.keys() {
            let is_critical = cp_result.is_critical(node_id);
            let slack = cp_result.slack(node_id).unwrap_or(f64::MAX);

            // Critical score: inversely proportional to slack.
            // On critical path (slack=0) gets maximum score.
            let max_slack = cp_result.makespan;
            let critical_score = if max_slack > 0.0 {
                ((1.0 - (slack / max_slack).min(1.0)) * self.critical_weight) as i64
            } else {
                self.critical_weight as i64
            };

            // CU score: proportional to estimated CU.
            let cu = graph.nodes[&node_id].estimated_cu as f64;
            let cu_score = ((cu / max_cu) * self.cu_weight) as i64;

            // Depth score: proportional to depth.
            let depth = depths.get(&node_id).copied().unwrap_or(0) as f64;
            let depth_score = ((depth / max_depth) * self.depth_weight) as i64;

            let priority = critical_score + cu_score + depth_score;

            debug!(
                "Node {}: priority={}, critical={}, cu={}, depth={}, is_critical={}",
                node_id, priority, critical_score, cu_score, depth_score, is_critical
            );

            priorities.insert(
                node_id,
                NodePriority {
                    node_id,
                    priority,
                    critical_score,
                    cu_score,
                    depth_score,
                    is_critical,
                },
            );
        }

        Ok(priorities)
    }

    /// Apply computed priorities back into the graph nodes.
    pub fn apply_priorities(&self, graph: &mut TransactionGraph) -> Result<()> {
        let priorities = self.compute_priorities(graph)?;

        for (node_id, pri) in &priorities {
            if let Some(node) = graph.nodes.get_mut(node_id) {
                node.priority = pri.priority;
            }
        }

        Ok(())
    }

    /// Return nodes sorted by priority (highest first).
    pub fn sorted_nodes(&self, graph: &TransactionGraph) -> Result<Vec<(NodeId, i64)>> {
        let priorities = self.compute_priorities(graph)?;
        let mut sorted: Vec<(NodeId, i64)> = priorities
            .iter()
            .map(|(&id, pri)| (id, pri.priority))
            .collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(sorted)
    }
}

impl Default for PriorityScheduler {
    fn default() -> Self {
        Self::new()
    }
}
