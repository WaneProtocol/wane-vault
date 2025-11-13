//! Comprehensive tests for the ivza-core analyzer module.
//!
//! Tests cover: DependencyAnalyzer, CriticalPathAnalyzer, and ParallelismAnalyzer.

use ivza_core::analyzer::{
    CriticalPathAnalyzer, DependencyAnalyzer, ParallelismAnalyzer,
};
use ivza_core::graph::{GraphEdge, GraphNode, TransactionGraph};
use ivza_core::types::{
    AccountAccess, AccountAccessEntry, AccountAccessTracker, AccountSet, DependencyType,
    InstructionData,
};
use solana_sdk::pubkey::Pubkey;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_pubkey(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn make_ix(program: Pubkey, reads: &[Pubkey], writes: &[Pubkey], label: &str) -> InstructionData {
    let mut accounts = Vec::new();
    for r in reads {
        accounts.push(AccountAccessEntry::read(*r));
    }
    for w in writes {
        accounts.push(AccountAccessEntry::write(*w));
    }
    InstructionData::new(program, accounts, vec![0]).with_label(label)
}

// ---------------------------------------------------------------------------
// DependencyAnalyzer tests
// ---------------------------------------------------------------------------

#[test]
fn test_dependency_no_conflict_read_only() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[shared], &[], "read_1");
    let ix2 = make_ix(program, &[shared], &[], "read_2");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix1]));
    graph.insert_node(GraphNode::new(1, vec![ix2]));

    let analyzer = DependencyAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    // Two reads on the same account: no conflict
    assert_eq!(result.edge_count(), 0);
}

#[test]
fn test_dependency_write_write_conflict() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "write_1");
    let ix2 = make_ix(program, &[], &[shared], "write_2");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix1]));
    graph.insert_node(GraphNode::new(1, vec![ix2]));

    let analyzer = DependencyAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert!(result.edge_count() >= 1);
    // Write-write should be classified as DataDependency
    assert!(result.edges[0].is_data_dependency());
}

#[test]
fn test_dependency_read_write_conflict() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "write_shared");
    let ix2 = make_ix(program, &[shared], &[], "read_shared");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix1]));
    graph.insert_node(GraphNode::new(1, vec![ix2]));

    let analyzer = DependencyAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert!(result.edge_count() >= 1);
}

#[test]
fn test_dependency_excluded_accounts() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "write_shared");
    let ix2 = make_ix(program, &[shared], &[], "read_shared");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix1]));
    graph.insert_node(GraphNode::new(1, vec![ix2]));

    // Exclude the shared account from conflict detection
    let analyzer = DependencyAnalyzer::new().with_excluded_accounts(vec![shared]);
    let result = analyzer.analyze(&graph).unwrap();

    // Should not detect any conflicts
    assert_eq!(result.edge_count(), 0);
}

#[test]
fn test_dependency_does_not_duplicate_existing_edges() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "w");
    let ix2 = make_ix(program, &[shared], &[], "r");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix1]));
    graph.insert_node(GraphNode::new(1, vec![ix2]));
    // Pre-add the edge
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();

    let analyzer = DependencyAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    // Should still have only 1 edge (not duplicated)
    assert_eq!(result.edge_count(), 1);
}

#[test]
fn test_dependency_multiple_conflicts() {
    let program = make_pubkey(0);

    let ix0 = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let ix1 = make_ix(program, &[], &[make_pubkey(1), make_pubkey(2)], "w12");
    let ix2 = make_ix(program, &[make_pubkey(2)], &[], "r2");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix0]));
    graph.insert_node(GraphNode::new(1, vec![ix1]));
    graph.insert_node(GraphNode::new(2, vec![ix2]));

    let analyzer = DependencyAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    // 0 and 1 conflict on account 1 (write-write)
    // 1 and 2 conflict on account 2 (write-read)
    assert!(result.edge_count() >= 2);
}

#[test]
fn test_dependency_independent_nodes() {
    let program = make_pubkey(0);

    let ix0 = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let ix1 = make_ix(program, &[], &[make_pubkey(2)], "w2");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix0]));
    graph.insert_node(GraphNode::new(1, vec![ix1]));

    let analyzer = DependencyAnalyzer::new();
    let independent = analyzer.independent_nodes(&graph);

    assert!(independent.contains(&0));
    assert!(independent.contains(&1));
}

#[test]
fn test_dependency_conflict_summary() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix0 = make_ix(program, &[], &[shared], "w");
    let ix1 = make_ix(program, &[shared], &[], "r");

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![ix0]));
    graph.insert_node(GraphNode::new(1, vec![ix1]));

    let analyzer = DependencyAnalyzer::new();
    let summary = analyzer.conflict_summary(&graph);

    assert!(summary.contains_key(&shared));
    assert!(*summary.get(&shared).unwrap() >= 1);
}

// ---------------------------------------------------------------------------
// CriticalPathAnalyzer tests
// ---------------------------------------------------------------------------

#[test]
fn test_critical_path_single_node() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));

    let analyzer = CriticalPathAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.makespan, 100.0);
    assert_eq!(result.critical_path, vec![0]);
    assert_eq!(result.critical_cu, 100);
}

#[test]
fn test_critical_path_chain() {
    // 0 -> 1 -> 2, each 100 CU
    let mut graph = TransactionGraph::new();
    for i in 0..3 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::DataDependency))
        .unwrap();

    let analyzer = CriticalPathAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.makespan, 300.0);
    assert_eq!(result.critical_path, vec![0, 1, 2]);
}

#[test]
fn test_critical_path_diamond() {
    // 0 -> 1 (200 CU), 0 -> 2 (50 CU), 1 -> 3, 2 -> 3
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(200));
    graph.insert_node(GraphNode::new(2, vec![]).with_estimated_cu(50));
    graph.insert_node(GraphNode::new(3, vec![]).with_estimated_cu(100));

    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(0, 2, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 3, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(2, 3, DependencyType::DataDependency))
        .unwrap();

    let analyzer = CriticalPathAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    // Critical path: 0 (100) -> 1 (200) -> 3 (100) = 400
    assert_eq!(result.makespan, 400.0);
    assert!(result.critical_path.contains(&0));
    assert!(result.critical_path.contains(&1));
    assert!(result.critical_path.contains(&3));

    // Node 2 should have slack
    assert!(result.timings[&2].slack > 0.0);
    assert!(!result.timings[&2].is_critical());
}

#[test]
fn test_critical_path_parallel_branches_different_weights() {
    // Two independent paths: 0->1 (100+300=400) and 2->3 (200+100=300)
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(300));
    graph.insert_node(GraphNode::new(2, vec![]).with_estimated_cu(200));
    graph.insert_node(GraphNode::new(3, vec![]).with_estimated_cu(100));

    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(2, 3, DependencyType::DataDependency))
        .unwrap();

    let analyzer = CriticalPathAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    // Makespan is max of two paths: 400
    assert_eq!(result.makespan, 400.0);
    // Critical path goes through 0 -> 1
    assert!(result.critical_path.contains(&0));
    assert!(result.critical_path.contains(&1));
}

#[test]
fn test_critical_path_uniform_duration() {
    let mut graph = TransactionGraph::new();
    for i in 0..3 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::DataDependency))
        .unwrap();

    let analyzer = CriticalPathAnalyzer::new().with_uniform_duration();
    let result = analyzer.analyze(&graph).unwrap();

    // With uniform duration, makespan = 3 (one per node)
    assert_eq!(result.makespan, 3.0);
}

#[test]
fn test_critical_path_empty_graph() {
    let graph = TransactionGraph::new();
    let analyzer = CriticalPathAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.makespan, 0.0);
    assert!(result.critical_path.is_empty());
}

#[test]
fn test_compute_depths() {
    let mut graph = TransactionGraph::new();
    for i in 0..4 {
        graph.insert_node(GraphNode::new(i, vec![]));
    }
    // 0 -> 1, 0 -> 2, 1 -> 3
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(0, 2, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 3, DependencyType::DataDependency))
        .unwrap();

    let analyzer = CriticalPathAnalyzer::new();
    let depths = analyzer.compute_depths(&graph).unwrap();

    assert_eq!(depths[&0], 0);
    assert_eq!(depths[&1], 1);
    assert_eq!(depths[&2], 1);
    assert_eq!(depths[&3], 2);
}

#[test]
fn test_nodes_by_slack() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(200));
    graph.insert_node(GraphNode::new(2, vec![]).with_estimated_cu(50));
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(0, 2, DependencyType::DataDependency))
        .unwrap();

    let analyzer = CriticalPathAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();
    let by_slack = result.nodes_by_slack();

    // Nodes on critical path (slack=0) should come first
    assert_eq!(by_slack[0].1, 0.0);
}

// ---------------------------------------------------------------------------
// ParallelismAnalyzer tests
// ---------------------------------------------------------------------------

#[test]
fn test_parallelism_fully_parallel() {
    let mut graph = TransactionGraph::new();
    for i in 0..5 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }
    // No edges: all nodes are independent
    let analyzer = ParallelismAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.levels.len(), 1);
    assert_eq!(result.max_parallelism, 5);
    assert_eq!(result.depth, 1);
    assert_eq!(result.avg_parallelism, 5.0);
}

#[test]
fn test_parallelism_fully_sequential() {
    let mut graph = TransactionGraph::new();
    for i in 0..4 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }
    for i in 0..3 {
        graph
            .add_edge(GraphEdge::new(i, i + 1, DependencyType::DataDependency))
            .unwrap();
    }

    let analyzer = ParallelismAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.levels.len(), 4);
    assert_eq!(result.max_parallelism, 1);
    assert_eq!(result.depth, 4);
    assert_eq!(result.avg_parallelism, 1.0);
}

#[test]
fn test_parallelism_diamond() {
    // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3
    let mut graph = TransactionGraph::new();
    for i in 0..4 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(0, 2, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 3, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(2, 3, DependencyType::DataDependency))
        .unwrap();

    let analyzer = ParallelismAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.levels.len(), 3);
    assert_eq!(result.max_parallelism, 2);
    assert_eq!(result.levels[1].width(), 2);
}

#[test]
fn test_parallelism_independent_subgraphs() {
    let mut graph = TransactionGraph::new();
    for i in 0..6 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }
    // Subgraph 1: 0 -> 1 -> 2
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::DataDependency))
        .unwrap();
    // Subgraph 2: 3 -> 4
    graph
        .add_edge(GraphEdge::new(3, 4, DependencyType::DataDependency))
        .unwrap();
    // Node 5 is standalone

    let analyzer = ParallelismAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.independent_subgraphs.len(), 3);
}

#[test]
fn test_parallelism_ratio_parallel() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(100));
    // No edges: fully parallel

    let analyzer = ParallelismAnalyzer::new();
    let (seq, par, ratio) = analyzer.parallelism_ratio(&graph).unwrap();

    assert_eq!(seq, 200.0);
    assert_eq!(par, 100.0);
    assert_eq!(ratio, 2.0);
}

#[test]
fn test_parallelism_ratio_sequential() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(100));
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();

    let analyzer = ParallelismAnalyzer::new();
    let (seq, par, ratio) = analyzer.parallelism_ratio(&graph).unwrap();

    assert_eq!(seq, 200.0);
    assert_eq!(par, 200.0); // Each level has only 1 node, max is 100 per level, sum is 200
    assert_eq!(ratio, 1.0);
}

#[test]
fn test_parallelism_empty_graph() {
    let graph = TransactionGraph::new();
    let analyzer = ParallelismAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.max_parallelism, 0);
    assert_eq!(result.depth, 0);
    assert!(result.levels.is_empty());
}

#[test]
fn test_parallelism_efficiency() {
    let mut graph = TransactionGraph::new();
    // 3 nodes: 0 parallel with 1, both before 2
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(2, vec![]).with_estimated_cu(100));
    graph
        .add_edge(GraphEdge::new(0, 2, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::DataDependency))
        .unwrap();

    let analyzer = ParallelismAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    // Levels: {0,1}, {2} -> avg_par = 3/2 = 1.5, max_par = 2
    assert_eq!(result.max_parallelism, 2);
    assert_eq!(result.depth, 2);
    assert!((result.avg_parallelism - 1.5).abs() < 0.01);
    assert!((result.efficiency() - 0.75).abs() < 0.01);
}

#[test]
fn test_parallelism_speedup() {
    let mut graph = TransactionGraph::new();
    for i in 0..4 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }
    // No edges: all parallel, depth = 1
    let analyzer = ParallelismAnalyzer::new();
    let result = analyzer.analyze(&graph).unwrap();

    assert_eq!(result.speedup(4), 4.0);
}

// ---------------------------------------------------------------------------
// AccountAccessTracker tests
// ---------------------------------------------------------------------------

#[test]
fn test_tracker_no_conflicts_all_reads() {
    let mut tracker = AccountAccessTracker::new();
    let account = make_pubkey(1);

    tracker.record(account, 0, AccountAccess::Read);
    tracker.record(account, 1, AccountAccess::Read);
    tracker.record(account, 2, AccountAccess::Read);

    let conflicts = tracker.find_conflicts();
    assert!(conflicts.is_empty());
}

#[test]
fn test_tracker_write_read_conflicts() {
    let mut tracker = AccountAccessTracker::new();
    let account = make_pubkey(1);

    tracker.record(account, 0, AccountAccess::Write);
    tracker.record(account, 1, AccountAccess::Read);
    tracker.record(account, 2, AccountAccess::Read);

    let conflicts = tracker.find_conflicts();
    assert!(conflicts.len() >= 2); // (0,1) and (0,2)
}

#[test]
fn test_tracker_write_write_conflicts() {
    let mut tracker = AccountAccessTracker::new();
    let account = make_pubkey(1);

    tracker.record(account, 0, AccountAccess::Write);
    tracker.record(account, 1, AccountAccess::Write);

    let conflicts = tracker.find_conflicts();
    assert_eq!(conflicts.len(), 1);
}

#[test]
fn test_tracker_nodes_accessing() {
    let mut tracker = AccountAccessTracker::new();
    let account = make_pubkey(1);

    tracker.record(account, 0, AccountAccess::Read);
    tracker.record(account, 5, AccountAccess::Write);

    let nodes = tracker.nodes_accessing(&account);
    assert_eq!(nodes.len(), 2);
    assert!(nodes.contains(&0));
    assert!(nodes.contains(&5));
}

#[test]
fn test_tracker_nodes_writing() {
    let mut tracker = AccountAccessTracker::new();
    let account = make_pubkey(1);

    tracker.record(account, 0, AccountAccess::Read);
    tracker.record(account, 1, AccountAccess::Write);
    tracker.record(account, 2, AccountAccess::Read);

    let writers = tracker.nodes_writing(&account);
    assert_eq!(writers, vec![1]);
}

#[test]
fn test_tracker_record_set() {
    let mut tracker = AccountAccessTracker::new();
    let mut set = AccountSet::new();
    set.add_read(make_pubkey(1));
    set.add_write(make_pubkey(2));

    tracker.record_set(0, &set);

    assert_eq!(tracker.tracked_account_count(), 2);
    assert_eq!(tracker.nodes_writing(&make_pubkey(2)), vec![0]);
}
