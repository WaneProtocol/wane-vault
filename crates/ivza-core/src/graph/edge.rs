use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::fmt;

use crate::types::{DependencyType, NodeId};

/// An edge in the execution graph representing a dependency between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    /// Source node ID (must complete before destination can start).
    pub from: NodeId,
    /// Destination node ID.
    pub to: NodeId,
    /// Type of dependency this edge represents.
    pub dependency_type: DependencyType,
    /// Edge weight, typically representing latency or cost.
    pub weight: f64,
    /// Accounts that caused this dependency (for AccountConflict and DataDependency types).
    pub conflicting_accounts: Vec<Pubkey>,
    /// Whether this edge was added automatically by the dependency analyzer.
    pub auto_detected: bool,
}

impl GraphEdge {
    /// Create a new edge with a given dependency type.
    pub fn new(from: NodeId, to: NodeId, dep_type: DependencyType) -> Self {
        Self {
            from,
            to,
            dependency_type: dep_type,
            weight: 1.0,
            conflicting_accounts: Vec::new(),
            auto_detected: false,
        }
    }

    /// Create a data dependency edge.
    pub fn data_dependency(from: NodeId, to: NodeId) -> Self {
        Self::new(from, to, DependencyType::DataDependency)
    }

    /// Create an ordering dependency edge.
    pub fn order_dependency(from: NodeId, to: NodeId) -> Self {
        Self::new(from, to, DependencyType::OrderDependency)
    }

    /// Create an account conflict edge.
    pub fn account_conflict(from: NodeId, to: NodeId, accounts: Vec<Pubkey>) -> Self {
        let mut edge = Self::new(from, to, DependencyType::AccountConflict);