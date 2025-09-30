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