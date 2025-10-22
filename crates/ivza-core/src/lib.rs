//! iVZA Core: Parallel Transaction Execution Engine for Solana
//!
//! This crate provides the core library for decomposing, analyzing, and scheduling
//! transaction graphs for maximum parallelism on Solana.
//!
//! # Architecture
//!
//! - **types**: Core data types for transactions, accounts, and access patterns.
//! - **graph**: Transaction graph construction, node/edge types, and decomposition.
//! - **analyzer**: Dependency analysis, critical path computation, and parallelism metrics.
//! - **scheduler**: Lane-based scheduling, priority assignment, and execution planning.
//! - **intent**: High-level intent parsing and resolution into transaction graphs.

pub mod analyzer;
pub mod graph;
pub mod intent;
pub mod scheduler;
pub mod types;

use anyhow::Result;
use tracing::info;

use analyzer::{
    CriticalPathAnalyzer, CriticalPathResult, DependencyAnalyzer, ParallelismAnalyzer,
    ParallelismResult,
};
use graph::{GraphDecomposer, TransactionGraph};
use intent::{Intent, IntentParser, IntentResolver};
use scheduler::{ExecutionPlan, ExecutionPlanner};
use types::InstructionData;

/// The main engine that ties together all stages of the iVZA pipeline:
/// decompose -> analyze -> schedule.
pub struct IvzaEngine {
    /// Decomposes instructions into a DAG.
    pub decomposer: GraphDecomposer,
    /// Detects account-level dependencies.
    pub dependency_analyzer: DependencyAnalyzer,
    /// Computes critical path timing.
    pub critical_path_analyzer: CriticalPathAnalyzer,
    /// Computes parallelism metrics.
    pub parallelism_analyzer: ParallelismAnalyzer,
    /// Produces execution plans.
    pub planner: ExecutionPlanner,
    /// Parses intent strings/JSON.
    pub intent_parser: IntentParser,
    /// Resolves intents into graphs.
    pub intent_resolver: IntentResolver,
}

/// The result of analyzing a graph, bundling the graph with its analysis.
#[derive(Debug, Clone)]
pub struct AnalyzedGraph {
    /// The transaction graph with dependency edges added.
    pub graph: TransactionGraph,
    /// Critical path analysis results.
    pub critical_path: CriticalPathResult,
    /// Parallelism analysis results.
    pub parallelism: ParallelismResult,
}

impl IvzaEngine {
    /// Create a new engine with default configuration.
    pub fn new() -> Self {
        Self {
            decomposer: GraphDecomposer::new(),
            dependency_analyzer: DependencyAnalyzer::new(),
            critical_path_analyzer: CriticalPathAnalyzer::new(),
            parallelism_analyzer: ParallelismAnalyzer::new(),
            planner: ExecutionPlanner::new(),
            intent_parser: IntentParser::new(),
            intent_resolver: IntentResolver::new(),
        }
    }

    /// Decompose a set of instructions into a transaction graph (DAG).
    pub fn decompose(&self, instructions: Vec<InstructionData>) -> Result<TransactionGraph> {
        info!(
            "IvzaEngine: decomposing {} instructions",
            instructions.len()
        );
        self.decomposer.decompose(instructions)
    }

    /// Analyze a transaction graph: detect dependencies, compute critical path and parallelism.
    pub fn analyze(&self, graph: &TransactionGraph) -> Result<AnalyzedGraph> {
        info!(
            "IvzaEngine: analyzing graph with {} nodes",
            graph.node_count()
        );

        // Step 1: Auto-detect dependencies from account access patterns.
        let graph_with_deps = self.dependency_analyzer.analyze(graph)?;

        // Step 2: Critical path analysis.
        let critical_path = self.critical_path_analyzer.analyze(&graph_with_deps)?;

        // Step 3: Parallelism analysis.
        let parallelism = self.parallelism_analyzer.analyze(&graph_with_deps)?;

        Ok(AnalyzedGraph {
            graph: graph_with_deps,
            critical_path,
            parallelism,
        })
    }

    /// Schedule an analyzed graph into an execution plan.
    pub fn schedule(&self, analyzed: &AnalyzedGraph) -> Result<ExecutionPlan> {
        info!(
            "IvzaEngine: scheduling {} nodes into lanes",
            analyzed.graph.node_count()
        );
        self.planner.plan(&analyzed.graph)
    }

    /// Run the full pipeline: decompose -> analyze -> schedule.
    pub fn process(&self, instructions: Vec<InstructionData>) -> Result<ExecutionPlan> {
        let graph = self.decompose(instructions)?;
        let analyzed = self.analyze(&graph)?;
        let plan = self.schedule(&analyzed)?;
        Ok(plan)
    }

    /// Process from a pre-built graph (skip decomposition).
    pub fn process_graph(&self, graph: &TransactionGraph) -> Result<ExecutionPlan> {
        let analyzed = self.analyze(graph)?;
        let plan = self.schedule(&analyzed)?;
        Ok(plan)
    }

    /// Process from an intent: parse -> resolve -> analyze -> schedule.
    pub fn process_intent(&self, intent: &Intent) -> Result<ExecutionPlan> {
        info!("IvzaEngine: processing intent {:?}", intent.intent_type);
        let graph = self.intent_resolver.resolve(intent)?;
        self.process_graph(&graph)
    }

    /// Process from a JSON intent string: parse -> resolve -> analyze -> schedule.
    pub fn process_intent_json(&self, json: &str) -> Result<ExecutionPlan> {
        let intent = self.intent_parser.parse_json(json)?;
        self.process_intent(&intent)
    }

    /// Process from a DSL intent string: parse -> resolve -> analyze -> schedule.
    pub fn process_intent_dsl(&self, dsl: &str) -> Result<ExecutionPlan> {
        let intent = self.intent_parser.parse_dsl(dsl)?;
        self.process_intent(&intent)
    }

    /// Process an optimized plan (with lane merging).
    pub fn process_optimized(&self, instructions: Vec<InstructionData>) -> Result<ExecutionPlan> {
        let graph = self.decompose(instructions)?;
        let analyzed = self.analyze(&graph)?;
        self.planner.plan_optimized(&analyzed.graph)
    }

    /// Get a summary of parallelism metrics for a set of instructions.
    pub fn parallelism_summary(
        &self,
        instructions: Vec<InstructionData>,
    ) -> Result<ParallelismSummary> {
        let graph = self.decompose(instructions)?;
        let analyzed = self.analyze(&graph)?;
        let plan = self.schedule(&analyzed)?;

        let (seq_cost, par_cost, ratio) = self
            .parallelism_analyzer
            .parallelism_ratio(&analyzed.graph)?;

        Ok(ParallelismSummary {
            total_nodes: analyzed.graph.node_count(),
            total_edges: analyzed.graph.edge_count(),
            critical_path_length: analyzed.critical_path.critical_path.len(),
            makespan: analyzed.critical_path.makespan,
            max_parallelism: analyzed.parallelism.max_parallelism,
            avg_parallelism: analyzed.parallelism.avg_parallelism,
            independent_subgraphs: analyzed.parallelism.independent_subgraphs.len(),
            num_lanes: plan.num_lanes(),
            sequential_cost: seq_cost,
            parallel_cost: par_cost,
            speedup_ratio: ratio,
        })
    }
}

impl Default for IvzaEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of parallelism analysis for reporting.
#[derive(Debug, Clone)]
pub struct ParallelismSummary {
    pub total_nodes: usize,
    pub total_edges: usize,
    pub critical_path_length: usize,
    pub makespan: f64,
    pub max_parallelism: usize,
    pub avg_parallelism: f64,
    pub independent_subgraphs: usize,
    pub num_lanes: usize,
    pub sequential_cost: f64,
    pub parallel_cost: f64,
    pub speedup_ratio: f64,
}

impl std::fmt::Display for ParallelismSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "=== iVZA Parallelism Summary ===")?;
        writeln!(
            f,
            "Nodes: {}, Edges: {}",
            self.total_nodes, self.total_edges
        )?;
        writeln!(
            f,
            "Critical path: {} nodes, makespan: {:.0}",
            self.critical_path_length, self.makespan
        )?;
        writeln!(
            f,
            "Parallelism: max={}, avg={:.2}",
            self.max_parallelism, self.avg_parallelism
        )?;
        writeln!(f, "Independent subgraphs: {}", self.independent_subgraphs)?;
        writeln!(f, "Execution lanes: {}", self.num_lanes)?;
        writeln!(
            f,
            "Speedup: {:.2}x (seq={:.0}, par={:.0})",
            self.speedup_ratio, self.sequential_cost, self.parallel_cost
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AccountAccess, AccountAccessEntry};
    use solana_sdk::pubkey::Pubkey;

    fn make_pubkey(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    fn make_ix(
        program: Pubkey,
        reads: &[Pubkey],
        writes: &[Pubkey],
        label: &str,
    ) -> InstructionData {
        let mut accounts = Vec::new();
        for r in reads {
            accounts.push(AccountAccessEntry::read(*r));