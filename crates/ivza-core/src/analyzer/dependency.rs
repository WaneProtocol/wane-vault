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
        }

        // Collect existing edges so we don't create duplicates.
        let existing_edges: HashSet<(NodeId, NodeId)> =
            graph.edges.iter().map(|e| (e.from, e.to)).collect();

        // Find all conflicting node pairs using the tracker.
        let conflicts = tracker.find_conflicts();

        // Group conflicts by node pair and collect conflicting accounts.
        let mut pair_conflicts: HashMap<(NodeId, NodeId), Vec<Pubkey>> = HashMap::new();
        for (a, b, account) in &conflicts {
            pair_conflicts.entry((*a, *b)).or_default().push(*account);
        }

        let mut added = 0;

        for ((node_a, node_b), accounts) in &pair_conflicts {
            // Skip if edge already exists.
            if existing_edges.contains(&(*node_a, *node_b))
                || existing_edges.contains(&(*node_b, *node_a))
            {
                continue;
            }

            let set_a = &node_sets[node_a];
            let set_b = &node_sets[node_b];

            // Determine the dependency type.
            let dep_type = self.classify_conflict(set_a, set_b, accounts);

            if let Some(dep_type) = dep_type {
                // Determine edge direction: lower ID -> higher ID (respecting original ordering).
                let (from, to) = if node_a < node_b {
                    (*node_a, *node_b)
                } else {
                    (*node_b, *node_a)
                };

                let edge = GraphEdge::new(from, to, dep_type)
                    .with_auto_detected(true)
                    .with_conflicting_accounts(accounts.clone());

                if let Err(e) = result.add_edge(edge) {
                    debug!("Could not add edge {} -> {}: {}", from, to, e);
                } else {
                    added += 1;
                }
            }
        }

        info!("Dependency analysis added {} edges", added);
        Ok(result)
    }

    /// Classify the conflict type between two sets given the conflicting accounts.
    fn classify_conflict(
        &self,
        set_a: &AccountSet,
        set_b: &AccountSet,
        conflicting_accounts: &[Pubkey],
    ) -> Option<DependencyType> {
        let mut has_write_write = false;
        let mut has_read_write = false;

        for account in conflicting_accounts {
            let a_writes = set_a.writes.contains(account);
            let b_writes = set_b.writes.contains(account);

            if a_writes && b_writes {
                has_write_write = true;
            } else if a_writes || b_writes {
                has_read_write = true;
            }
        }

        if has_write_write && self.detect_write_write {
            Some(DependencyType::DataDependency)
        } else if has_read_write && self.detect_read_write {
            Some(DependencyType::AccountConflict)
        } else {
            None
        }
    }

    /// Returns a summary of how many conflicts exist per account.
    pub fn conflict_summary(&self, graph: &TransactionGraph) -> HashMap<Pubkey, usize> {
        let mut tracker = AccountAccessTracker::new();
        for (&id, node) in &graph.nodes {
            let mut set = AccountSet::new();
            for ix in &node.instructions {