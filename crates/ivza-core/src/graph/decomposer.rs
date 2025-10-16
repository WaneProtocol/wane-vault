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
            .collect();

        for i in 0..instructions.len() {
            for j in (i + 1)..instructions.len() {
                if ix_account_sets[i].has_conflict(&ix_account_sets[j]) {
                    let dep_type =
                        self.classify_dependency(&ix_account_sets[i], &ix_account_sets[j]);
                    pg.add_edge(ix_indices[i], ix_indices[j], dep_type);
                    debug!("Instruction {} -> {} dependency: {:?}", i, j, dep_type);
                }
            }
        }

        // Step 3: Compute topological order and assign levels (longest path from any root).
        let topo_order = match toposort(&pg, None) {
            Ok(order) => order,
            Err(_) => {
                warn!("Cycle detected in instruction dependencies; falling back to single node");
                return self.single_node_graph(instructions);
            }
        };

        // Compute the level (longest path from any root) for each instruction.
        let mut levels: HashMap<NodeIndex, u32> = HashMap::new();
        for &node_idx in &topo_order {
            let mut max_pred_level: Option<u32> = None;
            for pred in pg.neighbors_directed(node_idx, Direction::Incoming) {
                let pred_level = levels.get(&pred).copied().unwrap_or(0);
                max_pred_level =
                    Some(max_pred_level.map_or(pred_level, |m: u32| m.max(pred_level)));
            }
            let my_level = max_pred_level.map_or(0, |l| l + 1);
            levels.insert(node_idx, my_level);
        }

        // Step 4: Group instructions by level.
        let max_level = levels.values().copied().max().unwrap_or(0);
        let mut level_groups: Vec<Vec<usize>> = vec![Vec::new(); (max_level + 1) as usize];
        for (&node_idx, &level) in &levels {
            let ix_idx = pg[node_idx];
            level_groups[level as usize].push(ix_idx);
        }

        // Sort each group for determinism.
        for group in &mut level_groups {
            group.sort();
        }

        // Step 5: Build the final TransactionGraph.
        // Each level becomes one or more graph nodes (split by max_instructions_per_node).
        let mut builder = TransactionGraphBuilder::new();
        // Map from instruction index to the graph node ID it was assigned to.
        let mut ix_to_node: HashMap<usize, NodeId> = HashMap::new();

        for (level, group) in level_groups.iter().enumerate() {
            // Split the group into chunks.
            for chunk in group.chunks(self.max_instructions_per_node) {
                let chunk_ixs: Vec<InstructionData> =
                    chunk.iter().map(|&idx| instructions[idx].clone()).collect();
                let label = if chunk.len() == 1 {
                    format!("level_{}_ix_{}", level, chunk[0])
                } else {
                    format!(
                        "level_{}_ix_{}_to_{}",
                        level,
                        chunk[0],
                        chunk[chunk.len() - 1]
                    )
                };
                let node_id = builder.add_labeled_node(label, chunk_ixs);
                for &ix_idx in chunk {
                    ix_to_node.insert(ix_idx, node_id);
                }
            }
        }
