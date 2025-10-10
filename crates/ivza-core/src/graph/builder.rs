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
            .get(&node_id)
            .map(|succs| succs.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default()
    }

    /// Returns the predecessors of a node.
    pub fn predecessors(&self, node_id: NodeId) -> Vec<NodeId> {
        self.reverse_adjacency
            .get(&node_id)
            .map(|preds| preds.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default()
    }

    /// Returns the in-degree of a node.
    pub fn in_degree(&self, node_id: NodeId) -> usize {
        self.reverse_adjacency.get(&node_id).map_or(0, |v| v.len())
    }

    /// Returns the out-degree of a node.
    pub fn out_degree(&self, node_id: NodeId) -> usize {
        self.adjacency.get(&node_id).map_or(0, |v| v.len())
    }

    /// Insert a pre-built node into the graph.
    pub fn insert_node(&mut self, node: GraphNode) {
        let id = node.id;
        if id >= self.next_id {
            self.next_id = id + 1;
        }
        self.nodes.insert(id, node);
        self.adjacency.entry(id).or_default();
        self.reverse_adjacency.entry(id).or_default();
    }

    /// Add an edge between two existing nodes.
    pub fn add_edge(&mut self, edge: GraphEdge) -> Result<()> {
        if !self.nodes.contains_key(&edge.from) {
            return Err(anyhow!("Source node {} not found", edge.from));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(anyhow!("Destination node {} not found", edge.to));
        }

        let edge_idx = self.edges.len();
        self.adjacency
            .entry(edge.from)
            .or_default()
            .push((edge.to, edge_idx));
        self.reverse_adjacency
            .entry(edge.to)
            .or_default()
            .push((edge.from, edge_idx));
        self.edges.push(edge);
        Ok(())
    }

    /// Allocate the next available node ID.
    pub fn next_node_id(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Check if the graph contains a cycle using DFS.
    pub fn has_cycle(&self) -> bool {
        let mut visited = HashMap::new(); // 0=unvisited, 1=in-progress, 2=done

        for &node_id in self.nodes.keys() {
            if self.dfs_cycle_check(node_id, &mut visited) {
                return true;
            }
        }
        false
    }

    fn dfs_cycle_check(&self, node: NodeId, visited: &mut HashMap<NodeId, u8>) -> bool {
        match visited.get(&node) {
            Some(1) => return true,  // Back edge found, cycle exists.
            Some(2) => return false, // Already fully explored.
            _ => {}
        }

        visited.insert(node, 1); // Mark in-progress.