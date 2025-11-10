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

        info!("Analyzing parallelism for {} nodes", graph.node_count());

        // Compute parallel levels using BFS-based topological layering.
        let levels = self.compute_levels(graph)?;

        let max_parallelism = levels.iter().map(|l| l.width()).max().unwrap_or(0);
        let avg_parallelism = if levels.is_empty() {
            0.0
        } else {
            let total_nodes: usize = levels.iter().map(|l| l.width()).sum();
            total_nodes as f64 / levels.len() as f64
        };

        // Compute independent subgraphs.
        let independent_subgraphs = self.find_independent_subgraphs(graph);

        let depth = levels.len();

        info!(
            "Parallelism analysis: {} levels, max_par={}, avg_par={:.2}, subgraphs={}",
            depth,
            max_parallelism,
            avg_parallelism,
            independent_subgraphs.len()
        );

        Ok(ParallelismResult {
            levels,
            max_parallelism,
            avg_parallelism,
            independent_subgraphs,
            depth,
        })
    }

    /// Compute parallel levels using Kahn's algorithm with level tracking.
    /// Each level contains all nodes whose in-degree becomes zero at that step.
    fn compute_levels(&self, graph: &TransactionGraph) -> Result<Vec<ParallelLevel>> {
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
        for &id in graph.nodes.keys() {
            in_degree.insert(id, graph.in_degree(id));
        }

        // Start with all zero-in-degree nodes.
        let mut current_level: Vec<NodeId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();
        current_level.sort();

        let mut levels = Vec::new();
        let mut processed = 0;

        while !current_level.is_empty() {
            let total_cu: u64 = current_level
                .iter()
                .filter_map(|id| graph.nodes.get(id))
                .map(|n| n.estimated_cu)
                .sum();

            levels.push(ParallelLevel {
                level: levels.len(),
                nodes: current_level.clone(),
                total_cu,
            });

            processed += current_level.len();

            // Compute next level.
            let mut next_level = Vec::new();