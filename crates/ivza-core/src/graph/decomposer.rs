use std::collections::{HashMap, HashSet};

use anyhow::Result;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use tracing::{debug, info, warn};

use crate::types::{AccountSet, DependencyType, InstructionData, NodeId};

use super::builder::{TransactionGraph, TransactionGraphBuilder};
use super::edge::GraphEdge;

/// Decomposes a complex transaction (a sequence of instructions) into a DAG of
/// sub-transactions based on account access patterns. Instructions that don't
/// conflict can be placed in different nodes and executed in parallel.
pub struct GraphDecomposer {
    /// Minimum number of instructions to attempt decomposition.
    pub min_instructions: usize,
    /// Maximum number of instructions per sub-transaction node.
    pub max_instructions_per_node: usize,
}

impl GraphDecomposer {
    pub fn new() -> Self {
        Self {
            min_instructions: 1,
            max_instructions_per_node: 4,
        }
    }

    pub fn with_min_instructions(mut self, min: usize) -> Self {
        self.min_instructions = min;
        self
    }

    pub fn with_max_instructions_per_node(mut self, max: usize) -> Self {
        self.max_instructions_per_node = max;
        self
    }

    /// Decompose a list of instructions into a DAG of sub-transaction nodes.
    ///
    /// Algorithm:
    /// 1. Build a petgraph where each instruction is a node.
    /// 2. Add edges between instructions that have account conflicts, respecting
    ///    the original ordering (earlier instruction -> later instruction).
    /// 3. Compute connected components of independent instruction chains.
    /// 4. For each chain, group instructions into sub-transaction nodes
    ///    (respecting max_instructions_per_node).
    /// 5. Build the final TransactionGraph with appropriate edges.
    pub fn decompose(&self, instructions: Vec<InstructionData>) -> Result<TransactionGraph> {
        if instructions.len() < self.min_instructions {
            return self.single_node_graph(instructions);
        }

        info!(
            "Decomposing {} instructions into parallel DAG",
            instructions.len()
        );

        // Step 1: Build instruction-level dependency graph using petgraph.
        let mut pg: DiGraph<usize, DependencyType> = DiGraph::new();
        let ix_indices: Vec<NodeIndex> = (0..instructions.len()).map(|i| pg.add_node(i)).collect();

        // Step 2: Detect conflicts between instructions.
        // For each pair (i, j) where i < j, if they conflict on any account, add edge i -> j.
        let ix_account_sets: Vec<AccountSet> = instructions
            .iter()
            .map(|ix| {
                let mut set = AccountSet::new();
                for entry in &ix.accounts {
                    set.add(entry.pubkey, entry.access);
                }
                set
            })