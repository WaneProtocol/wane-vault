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
            .filter(|id| self.adjacency.get(id).is_none_or(|succs| succs.is_empty()))
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

        if let Some(succs) = self.adjacency.get(&node) {
            for (succ, _) in succs {
                if self.dfs_cycle_check(*succ, visited) {
                    return true;
                }
            }
        }

        visited.insert(node, 2); // Mark done.
        false
    }

    /// Compute a topological ordering of the graph using Kahn's algorithm.
    /// Returns None if the graph contains a cycle.
    pub fn topological_sort(&self) -> Option<Vec<NodeId>> {
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
        for &id in self.nodes.keys() {
            in_degree.insert(id, self.in_degree(id));
        }

        let mut queue: Vec<NodeId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();
        queue.sort(); // Deterministic ordering.

        let mut result = Vec::with_capacity(self.nodes.len());

        while let Some(node) = queue.first().copied() {
            queue.remove(0);
            result.push(node);

            for succ in self.successors(node) {
                if let Some(deg) = in_degree.get_mut(&succ) {
                    *deg -= 1;
                    if *deg == 0 {
                        // Insert in sorted position for determinism.
                        let pos = queue.binary_search(&succ).unwrap_or_else(|e| e);
                        queue.insert(pos, succ);
                    }
                }
            }
        }

        if result.len() == self.nodes.len() {
            Some(result)
        } else {
            None // Cycle detected.
        }
    }

    /// Total estimated compute units across all nodes.
    pub fn total_estimated_cu(&self) -> u64 {
        self.nodes.values().map(|n| n.estimated_cu).sum()
    }
}

impl Default for TransactionGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// Fluent builder for constructing a TransactionGraph.
pub struct TransactionGraphBuilder {
    graph: TransactionGraph,
}

impl TransactionGraphBuilder {
    pub fn new() -> Self {
        Self {
            graph: TransactionGraph::new(),
        }
    }

    /// Add a node with the given instructions and return its assigned ID.
    pub fn add_node(&mut self, instructions: Vec<InstructionData>) -> NodeId {
        let id = self.graph.next_node_id();
        let node = GraphNode::new(id, instructions);
        self.graph.insert_node(node);
        debug!("Added graph node {}", id);
        id
    }

    /// Add a pre-built GraphNode.
    pub fn add_graph_node(&mut self, node: GraphNode) -> NodeId {
        let id = node.id;
        self.graph.insert_node(node);
        id
    }

    /// Add a node with a label.
    pub fn add_labeled_node(
        &mut self,
        label: impl Into<String>,
        instructions: Vec<InstructionData>,
    ) -> NodeId {
        let id = self.graph.next_node_id();
        let node = GraphNode::new(id, instructions).with_label(label);
        self.graph.insert_node(node);
        id
    }

    /// Add a node with custom estimated CU.
    pub fn add_node_with_cu(&mut self, instructions: Vec<InstructionData>, cu: u64) -> NodeId {
        let id = self.graph.next_node_id();
        let node = GraphNode::new(id, instructions).with_estimated_cu(cu);
        self.graph.insert_node(node);
        id
    }

    /// Add an edge between two nodes.
    pub fn add_edge(
        &mut self,
        from: NodeId,
        to: NodeId,
        dep_type: DependencyType,
    ) -> Result<&mut Self> {
        let edge = GraphEdge::new(from, to, dep_type);
        self.graph.add_edge(edge)?;
        debug!("Added edge {} -> {} ({:?})", from, to, dep_type);
        Ok(self)
    }

    /// Add a data dependency edge.
    pub fn add_data_dependency(&mut self, from: NodeId, to: NodeId) -> Result<&mut Self> {
        self.add_edge(from, to, DependencyType::DataDependency)
    }

    /// Add an order dependency edge.
    pub fn add_order_dependency(&mut self, from: NodeId, to: NodeId) -> Result<&mut Self> {
        self.add_edge(from, to, DependencyType::OrderDependency)
    }

    /// Add an account conflict edge.
    pub fn add_account_conflict(&mut self, from: NodeId, to: NodeId) -> Result<&mut Self> {
        self.add_edge(from, to, DependencyType::AccountConflict)
    }

    /// Build the graph, validating that it's a valid DAG.
    pub fn build(self) -> Result<TransactionGraph> {
        if self.graph.has_cycle() {
            return Err(anyhow!("Transaction graph contains a cycle"));
        }
        info!(
            "Built transaction graph with {} nodes and {} edges",
            self.graph.node_count(),
            self.graph.edge_count()
        );
        Ok(self.graph)
    }

    /// Build without cycle validation (for intermediate constructions).
    pub fn build_unchecked(self) -> TransactionGraph {
        self.graph
    }
}

impl Default for TransactionGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}
