use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::fmt;

/// Describes whether an account is accessed for reading or writing.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
pub enum AccountAccess {
    Read,
    Write,
}

impl AccountAccess {
    /// Returns true if this access mode conflicts with another.
    /// Two reads do not conflict; any write conflicts with any other access.
    pub fn conflicts_with(&self, other: &AccountAccess) -> bool {
        matches!(
            (self, other),
            (AccountAccess::Write, _) | (_, AccountAccess::Write)
        )
    }

    pub fn is_write(&self) -> bool {
        matches!(self, AccountAccess::Write)
    }

    pub fn is_read(&self) -> bool {
        matches!(self, AccountAccess::Read)
    }
}

/// A single account access entry within a transaction node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountAccessEntry {
    pub pubkey: Pubkey,
    pub access: AccountAccess,
}

impl AccountAccessEntry {
    pub fn new(pubkey: Pubkey, access: AccountAccess) -> Self {
        Self { pubkey, access }
    }

    pub fn read(pubkey: Pubkey) -> Self {
        Self::new(pubkey, AccountAccess::Read)
    }

    pub fn write(pubkey: Pubkey) -> Self {
        Self::new(pubkey, AccountAccess::Write)
    }
}

/// Data for a single instruction within a transaction node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionData {
    /// The program that processes this instruction.
    pub program_id: Pubkey,
    /// Accounts required by this instruction, with access modes.
    pub accounts: Vec<AccountAccessEntry>,
    /// Opaque instruction data bytes.
    pub data: Vec<u8>,
    /// Human-readable label for debugging.
    pub label: Option<String>,
}

impl InstructionData {
    pub fn new(program_id: Pubkey, accounts: Vec<AccountAccessEntry>, data: Vec<u8>) -> Self {
        Self {
            program_id,
            accounts,
            data,
            label: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Returns all account pubkeys accessed by this instruction.
    pub fn accessed_accounts(&self) -> Vec<Pubkey> {
        self.accounts.iter().map(|a| a.pubkey).collect()
    }

    /// Returns all accounts written by this instruction.
    pub fn write_accounts(&self) -> Vec<Pubkey> {
        self.accounts
            .iter()
            .filter(|a| a.access.is_write())
            .map(|a| a.pubkey)
            .collect()
    }

    /// Returns all accounts read by this instruction.
    pub fn read_accounts(&self) -> Vec<Pubkey> {
        self.accounts
            .iter()
            .filter(|a| a.access.is_read())
            .map(|a| a.pubkey)
            .collect()
    }
}

/// Unique identifier for a transaction node within a graph.
pub type NodeId = u64;

/// A node in the transaction graph representing one or more instructions
/// to be executed atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionNode {
    /// Unique ID within the graph.
    pub id: NodeId,
    /// Instructions in this transaction node.
    pub instructions: Vec<InstructionData>,
    /// Estimated compute units for execution.
    pub estimated_cu: u64,
    /// Priority fee in micro-lamports.
    pub priority_fee: u64,
    /// User-assigned label.
    pub label: Option<String>,
    /// Metadata for extensions.
    pub metadata: HashMap<String, String>,
}

impl TransactionNode {
    pub fn new(id: NodeId, instructions: Vec<InstructionData>) -> Self {
        let estimated_cu = instructions.len() as u64 * 200_000;
        Self {
            id,
            instructions,
            estimated_cu,
            priority_fee: 0,
            label: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_estimated_cu(mut self, cu: u64) -> Self {
        self.estimated_cu = cu;
        self
    }

    pub fn with_priority_fee(mut self, fee: u64) -> Self {
        self.priority_fee = fee;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Collects all account access entries across all instructions.
    pub fn all_account_accesses(&self) -> Vec<AccountAccessEntry> {
        self.instructions
            .iter()
            .flat_map(|ix| ix.accounts.iter().cloned())
            .collect()
    }

    /// Returns a deduplicated set of all accessed pubkeys.
    pub fn accessed_pubkeys(&self) -> Vec<Pubkey> {
        let mut keys: Vec<Pubkey> = self
            .instructions