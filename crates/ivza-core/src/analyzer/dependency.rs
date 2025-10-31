use std::collections::{HashMap, HashSet};

use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info};

use crate::graph::{GraphEdge, TransactionGraph};
use crate::types::{AccountAccessTracker, AccountSet, DependencyType, NodeId};

/// Analyzes account read/write sets across transaction nodes to detect dependencies.
///
/// Two transactions conflict if they both access the same account and at least one
/// performs a write. The analyzer examines all node pairs and inserts edges where
/// conflicts are found.
pub struct DependencyAnalyzer {
    /// If true, add edges for read-write conflicts (conservative).
    pub detect_read_write: bool,
    /// If true, add edges for write-write conflicts.
    pub detect_write_write: bool,
    /// Accounts to exclude from conflict detection (e.g., system program, rent sysvar).
    pub excluded_accounts: HashSet<Pubkey>,
}

impl DependencyAnalyzer {
    pub fn new() -> Self {
        Self {
            detect_read_write: true,
            detect_write_write: true,
            excluded_accounts: HashSet::new(),
        }
    }

    /// Add a set of accounts to exclude from conflict checking (e.g. well-known programs).
    pub fn with_excluded_accounts(mut self, accounts: impl IntoIterator<Item = Pubkey>) -> Self {
        self.excluded_accounts.extend(accounts);
        self
    }

    /// Analyze the graph and return a new graph with auto-detected dependency edges added.
    pub fn analyze(&self, graph: &TransactionGraph) -> Result<TransactionGraph> {
        info!("Analyzing dependencies for {} nodes", graph.node_count());

        let mut result = graph.clone();

        // Build account access tracker from all nodes.
        let mut tracker = AccountAccessTracker::new();
        let mut node_sets: HashMap<NodeId, AccountSet> = HashMap::new();

        for (&id, node) in &graph.nodes {
            let mut set = AccountSet::new();
            for ix in &node.instructions {
                for entry in &ix.accounts {
                    if self.excluded_accounts.contains(&entry.pubkey) {
                        continue;
                    }
                    set.add(entry.pubkey, entry.access);
                }
            }
            tracker.record_set(id, &set);
            node_sets.insert(id, set);