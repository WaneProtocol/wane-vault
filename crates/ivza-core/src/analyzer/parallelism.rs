use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tracing::info;

use crate::graph::TransactionGraph;
use crate::types::NodeId;

/// A single parallel execution level: all nodes in this level can execute concurrently.
#[derive(Debug, Clone)]
pub struct ParallelLevel {
    /// Level index (0 = first to execute).
    pub level: usize,
    /// Node IDs in this level.
    pub nodes: Vec<NodeId>,
    /// Total estimated CU across all nodes in this level.
    pub total_cu: u64,
}

impl ParallelLevel {
    /// Returns the number of nodes in this level (parallelism width).
    pub fn width(&self) -> usize {
        self.nodes.len()
    }
}

/// Result of parallelism analysis.
#[derive(Debug, Clone)]
pub struct ParallelismResult {
    /// Nodes grouped into parallel levels (topological layers).
    pub levels: Vec<ParallelLevel>,
    /// Maximum parallelism degree (width of the widest level).
    pub max_parallelism: usize,
    /// Average parallelism degree.
    pub avg_parallelism: f64,
    /// Independent subgraphs (connected components that share no edges).
    pub independent_subgraphs: Vec<Vec<NodeId>>,
    /// Total number of levels (sequential depth).
    pub depth: usize,
}

impl ParallelismResult {
    /// Returns the parallelism efficiency: avg_parallelism / max_parallelism.
    pub fn efficiency(&self) -> f64 {
        if self.max_parallelism == 0 {
            return 0.0;
        }
        self.avg_parallelism / self.max_parallelism as f64
    }

    /// Returns the speedup over purely sequential execution.
    /// Speedup = total_nodes / depth.
    pub fn speedup(&self, total_nodes: usize) -> f64 {
        if self.depth == 0 {
            return 0.0;
        }
        total_nodes as f64 / self.depth as f64
    }
}

/// Analyzes the maximum parallelism available in a transaction DAG.
///
/// Groups nodes into parallel execution levels using topological sort.
/// All nodes at the same level have their dependencies satisfied by
/// prior levels and can execute concurrently.
pub struct ParallelismAnalyzer;

impl ParallelismAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyze the parallelism of the given graph.
    pub fn analyze(&self, graph: &TransactionGraph) -> Result<ParallelismResult> {
        if graph.node_count() == 0 {
            return Ok(ParallelismResult {
                levels: Vec::new(),
                max_parallelism: 0,
                avg_parallelism: 0.0,
                independent_subgraphs: Vec::new(),
                depth: 0,
            });
        }
