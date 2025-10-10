use std::collections::HashMap;

use anyhow::{anyhow, Result};
use tracing::{debug, info};

use crate::types::{DependencyType, InstructionData, NodeId};

use super::edge::GraphEdge;
use super::node::GraphNode;

/// The transaction graph: a collection of nodes and directed edges forming a DAG.
#[derive(Debug, Clone)]
pub struct TransactionGraph {
    /// All nodes keyed by their ID.
    pub nodes: HashMap<NodeId, GraphNode>,
    /// All edges in the graph.
    pub edges: Vec<GraphEdge>,
    /// Adjacency list: node_id -> list of (neighbor_id, edge_index).
    pub adjacency: HashMap<NodeId, Vec<(NodeId, usize)>>,
    /// Reverse adjacency: node_id -> list of (predecessor_id, edge_index).
    pub reverse_adjacency: HashMap<NodeId, Vec<(NodeId, usize)>>,
    /// Next node ID to assign.
    next_id: NodeId,
}

impl TransactionGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            adjacency: HashMap::new(),
            reverse_adjacency: HashMap::new(),
            next_id: 0,
        }
    }

    /// Returns the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Returns all node IDs.
    pub fn node_ids(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.nodes.keys().copied().collect();
        ids.sort();
        ids
    }

    /// Returns root nodes (nodes with no incoming edges).
    pub fn root_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .keys()
            .filter(|id| {
                self.reverse_adjacency
                    .get(id)
                    .is_none_or(|preds| preds.is_empty())
            })
            .copied()
            .collect()
    }

    /// Returns leaf nodes (nodes with no outgoing edges).
    pub fn leaf_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .keys()
            .filter(|id| {
                self.adjacency
                    .get(id)
                    .is_none_or(|succs| succs.is_empty())
            })
            .copied()
            .collect()
    }

    /// Returns the successors of a node.
    pub fn successors(&self, node_id: NodeId) -> Vec<NodeId> {
        self.adjacency