use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;
use std::fmt;

use crate::types::{AccountSet, InstructionData, NodeId};

/// A node in the execution graph with full metadata for scheduling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    /// Unique identifier within this graph.
    pub id: NodeId,
    /// Instructions to execute in this node.
    pub instructions: Vec<InstructionData>,
    /// Account access pattern summary.
    pub account_set: AccountSet,
    /// Priority (higher = more important, scheduled first).
    pub priority: i64,
    /// Estimated compute units for this node.
    pub estimated_cu: u64,
    /// Depth in the dependency graph (0 = root).
    pub depth: u32,
    /// User label for debugging.
    pub label: Option<String>,
}

impl GraphNode {
    /// Create a new graph node from instructions, automatically computing the account set.
    pub fn new(id: NodeId, instructions: Vec<InstructionData>) -> Self {
        let mut account_set = AccountSet::new();
        for ix in &instructions {
            for entry in &ix.accounts {
                account_set.add(entry.pubkey, entry.access);
            }
        }

        let estimated_cu = instructions.len() as u64 * 200_000;

        Self {
            id,
            instructions,
            account_set,
            priority: 0,
            estimated_cu,
            depth: 0,
            label: None,
        }
    }

    pub fn with_priority(mut self, priority: i64) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_estimated_cu(mut self, cu: u64) -> Self {
        self.estimated_cu = cu;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_depth(mut self, depth: u32) -> Self {
        self.depth = depth;
        self
    }

    /// Returns true if this node's account access pattern conflicts with another node's.
    pub fn conflicts_with(&self, other: &GraphNode) -> bool {
        self.account_set.has_conflict(&other.account_set)
    }

    /// Returns the set of accounts written by this node.
    pub fn write_set(&self) -> &HashSet<Pubkey> {
        &self.account_set.writes
    }

    /// Returns the set of accounts read (not written) by this node.
    pub fn read_set(&self) -> &HashSet<Pubkey> {
        &self.account_set.reads
    }

    /// Returns all unique program IDs referenced by instructions in this node.
    pub fn program_ids(&self) -> Vec<Pubkey> {
        let mut ids: Vec<Pubkey> = self.instructions.iter().map(|ix| ix.program_id).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// Total number of account accesses across all instructions.
    pub fn total_account_accesses(&self) -> usize {
        self.instructions.iter().map(|ix| ix.accounts.len()).sum()
    }
}

impl fmt::Display for GraphNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = self.label.as_deref().unwrap_or("unlabeled");
        write!(
            f,
            "GraphNode(id={}, label={}, pri={}, cu={}, reads={}, writes={})",
            self.id,
            label,
            self.priority,
            self.estimated_cu,
            self.account_set.reads.len(),
            self.account_set.writes.len(),
        )
    }
}

/// Convenience builder for creating GraphNode instances with a fluent API.
pub struct GraphNodeBuilder {
    id: NodeId,
    instructions: Vec<InstructionData>,
    priority: i64,
    estimated_cu: Option<u64>,
    label: Option<String>,
}

impl GraphNodeBuilder {
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            instructions: Vec::new(),
            priority: 0,
            estimated_cu: None,
            label: None,
        }
    }

    pub fn instruction(mut self, ix: InstructionData) -> Self {
        self.instructions.push(ix);
        self
    }

    pub fn instructions(mut self, ixs: Vec<InstructionData>) -> Self {
        self.instructions.extend(ixs);
        self
    }

    pub fn priority(mut self, priority: i64) -> Self {
        self.priority = priority;
        self
    }

    pub fn estimated_cu(mut self, cu: u64) -> Self {
        self.estimated_cu = Some(cu);
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn build(self) -> GraphNode {
        let mut node = GraphNode::new(self.id, self.instructions).with_priority(self.priority);
        if let Some(cu) = self.estimated_cu {
            node = node.with_estimated_cu(cu);
        }
        if let Some(label) = self.label {
            node = node.with_label(label);
        }
        node
    }
}
