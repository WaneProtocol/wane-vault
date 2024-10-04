use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant};

/// Describes whether an account is accessed for reading or writing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
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
            .iter()
            .flat_map(|ix| ix.accounts.iter().map(|a| a.pubkey))
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Returns all pubkeys written across all instructions.
    pub fn write_set(&self) -> Vec<Pubkey> {
        let mut keys: Vec<Pubkey> = self
            .instructions
            .iter()
            .flat_map(|ix| ix.write_accounts())
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }

    /// Returns all pubkeys read (but not written) across all instructions.
    pub fn read_set(&self) -> Vec<Pubkey> {
        let write_set = self.write_set();
        let mut keys: Vec<Pubkey> = self
            .instructions
            .iter()
            .flat_map(|ix| ix.read_accounts())
            .filter(|k| !write_set.contains(k))
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }
}

impl fmt::Display for TransactionNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TxNode(id={}, ixs={}, cu={})",
            self.id,
            self.instructions.len(),
            self.estimated_cu
        )
    }
}

/// Dependency type between two transaction nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DependencyType {
    /// Write-after-read or write-after-write on the same account.
    DataDependency,
    /// Explicit ordering constraint (not derived from account access).
    OrderDependency,
    /// Both transactions access the same account and at least one writes.
    AccountConflict,
}

/// An edge in the transaction graph representing a dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEdge {
    /// Source node (must execute first).
    pub from: NodeId,
    /// Destination node (executes after source).
    pub to: NodeId,
    /// Type of dependency.
    pub dependency_type: DependencyType,
    /// Weight representing the cost/latency of the dependency.
    pub weight: f64,
    /// The conflicting accounts, if any.
    pub conflicting_accounts: Vec<Pubkey>,
}

impl TransactionEdge {
    pub fn new(from: NodeId, to: NodeId, dependency_type: DependencyType) -> Self {
        Self {
            from,
            to,
            dependency_type,
            weight: 1.0,
            conflicting_accounts: Vec::new(),
        }
    }

    pub fn with_weight(mut self, weight: f64) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_conflicting_accounts(mut self, accounts: Vec<Pubkey>) -> Self {
        self.conflicting_accounts = accounts;
        self
    }
}

impl fmt::Display for TransactionEdge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Edge({} -> {}, {:?}, w={})",
            self.from, self.to, self.dependency_type, self.weight
        )
    }
}

/// Status of a transaction execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// Not yet executed.
    Pending,
    /// Currently being executed.
    Running,
    /// Successfully executed.
    Success,
    /// Failed with an error message.
    Failed(String),
    /// Skipped due to a failed dependency.
    Skipped,
}

impl ExecutionStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ExecutionStatus::Success | ExecutionStatus::Failed(_) | ExecutionStatus::Skipped
        )
    }

    pub fn is_success(&self) -> bool {
        matches!(self, ExecutionStatus::Success)
    }
}

/// Result of executing a single transaction node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub node_id: NodeId,
    pub status: ExecutionStatus,
    pub signature: Option<String>,
    pub compute_units_consumed: Option<u64>,
    pub execution_time_ms: u64,
    pub slot: Option<u64>,
    pub error_message: Option<String>,
}

impl ExecutionResult {
    pub fn success(node_id: NodeId, signature: String, cu: u64, time_ms: u64) -> Self {
        Self {
            node_id,
            status: ExecutionStatus::Success,
            signature: Some(signature),
            compute_units_consumed: Some(cu),
            execution_time_ms: time_ms,
            slot: None,
            error_message: None,
        }
    }

    pub fn failure(node_id: NodeId, error: String, time_ms: u64) -> Self {
        Self {
            node_id,
            status: ExecutionStatus::Failed(error.clone()),
            signature: None,
            compute_units_consumed: None,
            execution_time_ms: time_ms,
            slot: None,
            error_message: Some(error),
        }
    }

    pub fn skipped(node_id: NodeId) -> Self {
        Self {
            node_id,
            status: ExecutionStatus::Skipped,
            signature: None,
            compute_units_consumed: None,
            execution_time_ms: 0,
            slot: None,
            error_message: None,
        }
    }
}
