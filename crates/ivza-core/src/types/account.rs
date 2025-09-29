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
