use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, HashSet};
use std::fmt;

use super::transaction::AccountAccess;

/// Wrapper around Solana account metadata with access tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountMeta {
    pub pubkey: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

impl AccountMeta {
    pub fn new(pubkey: Pubkey, is_signer: bool, is_writable: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable,
        }
    }

    pub fn readonly(pubkey: Pubkey) -> Self {
        Self::new(pubkey, false, false)
    }

    pub fn writable(pubkey: Pubkey) -> Self {
        Self::new(pubkey, false, true)
    }

    pub fn signer(pubkey: Pubkey) -> Self {
        Self::new(pubkey, true, false)
    }

    pub fn signer_writable(pubkey: Pubkey) -> Self {
        Self::new(pubkey, true, true)
    }

    pub fn access_mode(&self) -> AccountAccess {
        if self.is_writable {
            AccountAccess::Write
        } else {
            AccountAccess::Read
        }
    }
}

impl fmt::Display for AccountMeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Account({}, signer={}, writable={})",
            &self.pubkey.to_string()[..8],
            self.is_signer,
            self.is_writable,
        )
    }
}

/// Tracks the read and write sets of accounts for a transaction or group of transactions.
/// Used to detect conflicts between transaction nodes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountSet {
    /// Accounts that are read (but not written).
    pub reads: HashSet<Pubkey>,
    /// Accounts that are written.
    pub writes: HashSet<Pubkey>,
}

impl AccountSet {
    pub fn new() -> Self {
        Self {
            reads: HashSet::new(),
            writes: HashSet::new(),
        }
    }

    /// Add a read access for the given account.
    pub fn add_read(&mut self, pubkey: Pubkey) {
        // If the account is already in the write set, it stays as a write.
        if !self.writes.contains(&pubkey) {
            self.reads.insert(pubkey);
        }
    }

    /// Add a write access for the given account.
    pub fn add_write(&mut self, pubkey: Pubkey) {
        // Promote from read to write if necessary.
        self.reads.remove(&pubkey);
        self.writes.insert(pubkey);
    }

    /// Add an access entry.
    pub fn add(&mut self, pubkey: Pubkey, access: AccountAccess) {
        match access {
            AccountAccess::Read => self.add_read(pubkey),
            AccountAccess::Write => self.add_write(pubkey),
        }
    }

    /// Returns all accounts accessed (read or write).
    pub fn all_accounts(&self) -> HashSet<Pubkey> {
        self.reads.union(&self.writes).copied().collect()
    }

    /// Returns the total number of unique accounts.
    pub fn len(&self) -> usize {
        self.reads.len() + self.writes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reads.is_empty() && self.writes.is_empty()
    }

    /// Merge another AccountSet into this one.
    pub fn merge(&mut self, other: &AccountSet) {
        for key in &other.writes {
            self.add_write(*key);
        }
        for key in &other.reads {
            self.add_read(*key);
        }
    }

    /// Check if this AccountSet conflicts with another.
    /// Conflict occurs when both sets access the same account and at least one is a write.
    pub fn conflicts_with(&self, other: &AccountSet) -> AccountConflict {
        let mut conflicting = Vec::new();

        // Our writes vs their reads or writes
        for w in &self.writes {
            if other.reads.contains(w) || other.writes.contains(w) {
                conflicting.push(*w);
            }
        }

        // Our reads vs their writes (don't double-count)
        for r in &self.reads {
            if other.writes.contains(r) && !conflicting.contains(r) {
                conflicting.push(*r);
            }
        }

        if conflicting.is_empty() {
            AccountConflict::None
        } else {
            conflicting.sort();
            AccountConflict::Conflict(conflicting)
        }
    }

    /// Returns true if there is any conflict with the other set.
    pub fn has_conflict(&self, other: &AccountSet) -> bool {
        !matches!(self.conflicts_with(other), AccountConflict::None)
    }
}

impl fmt::Display for AccountSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AccountSet(reads={}, writes={})",
            self.reads.len(),
            self.writes.len()
        )
    }
}

/// Result of conflict detection between two account sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountConflict {
    /// No conflict detected.
    None,
    /// Conflict on the listed accounts.
    Conflict(Vec<Pubkey>),
}

impl AccountConflict {
    pub fn is_conflict(&self) -> bool {
        matches!(self, AccountConflict::Conflict(_))
    }

    pub fn conflicting_accounts(&self) -> &[Pubkey] {
        match self {
            AccountConflict::None => &[],
            AccountConflict::Conflict(accounts) => accounts,
        }
    }
}

/// Tracks account access across multiple transaction nodes.
/// Used by the dependency analyzer to build the conflict graph.
#[derive(Debug, Clone, Default)]
pub struct AccountAccessTracker {
    /// Maps each account to the list of (node_id, access_mode) entries.
    accesses: HashMap<Pubkey, Vec<(u64, AccountAccess)>>,
}

impl AccountAccessTracker {
    pub fn new() -> Self {
        Self {
            accesses: HashMap::new(),
        }
    }

    /// Record that a node accesses a given account.
    pub fn record(&mut self, pubkey: Pubkey, node_id: u64, access: AccountAccess) {
        self.accesses
            .entry(pubkey)
            .or_default()
            .push((node_id, access));
    }

    /// Record all accesses from an AccountSet for a given node.
    pub fn record_set(&mut self, node_id: u64, set: &AccountSet) {
        for r in &set.reads {
            self.record(*r, node_id, AccountAccess::Read);
        }
        for w in &set.writes {
            self.record(*w, node_id, AccountAccess::Write);
        }
    }

    /// Find all pairs of conflicting nodes.
    /// Returns (node_a, node_b, conflicting_account) triples where node_a < node_b.
    pub fn find_conflicts(&self) -> Vec<(u64, u64, Pubkey)> {
        let mut conflicts = Vec::new();

        for (pubkey, accesses) in &self.accesses {
            let has_write = accesses.iter().any(|(_, a)| a.is_write());
            if !has_write {
                // All reads, no conflict.
                continue;
            }

            // For each pair of accesses to this account where at least one is a write
            for i in 0..accesses.len() {
                for j in (i + 1)..accesses.len() {
                    let (node_a, access_a) = &accesses[i];
                    let (node_b, access_b) = &accesses[j];

                    if node_a == node_b {
                        continue;
                    }

                    if access_a.conflicts_with(access_b) {
                        let (lo, hi) = if node_a < node_b {
                            (*node_a, *node_b)
                        } else {
                            (*node_b, *node_a)
                        };
                        conflicts.push((lo, hi, *pubkey));
                    }
                }
            }
        }

        // Deduplicate
        conflicts.sort();
        conflicts.dedup();
        conflicts
    }

    /// Returns all node IDs that have accessed a given account.
    pub fn nodes_accessing(&self, pubkey: &Pubkey) -> Vec<u64> {
        self.accesses
            .get(pubkey)
            .map(|v| v.iter().map(|(id, _)| *id).collect())
            .unwrap_or_default()
    }

    /// Returns all node IDs that write to a given account.
    pub fn nodes_writing(&self, pubkey: &Pubkey) -> Vec<u64> {
        self.accesses
            .get(pubkey)
            .map(|v| {
                v.iter()
                    .filter(|(_, a)| a.is_write())
                    .map(|(id, _)| *id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the number of tracked accounts.
    pub fn tracked_account_count(&self) -> usize {
        self.accesses.len()
    }
}
