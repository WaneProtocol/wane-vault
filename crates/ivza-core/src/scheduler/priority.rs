use std::collections::HashMap;

use anyhow::Result;
use tracing::{debug, info};

use crate::analyzer::CriticalPathAnalyzer;
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