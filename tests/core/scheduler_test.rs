//! Comprehensive tests for the ivza-core scheduler module.
//!
//! Tests cover: ExecutionPlanner, PriorityScheduler, ExecutionLane, and the
//! full IvzaEngine pipeline.

use ivza_core::graph::{GraphEdge, GraphNode, TransactionGraph, TransactionGraphBuilder};
use ivza_core::scheduler::{ExecutionLane, ExecutionPlan, ExecutionPlanner};
use ivza_core::scheduler::PriorityScheduler;
use ivza_core::types::{
    AccountAccessEntry, DependencyType, InstructionData,
};
use ivza_core::IvzaEngine;
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
// ExecutionLane tests
// ---------------------------------------------------------------------------

#[test]
fn test_lane_new_is_empty() {
    let lane = ExecutionLane::new(0);
    assert!(lane.is_empty());
    assert_eq!(lane.width(), 0);
    assert_eq!(lane.total_cu, 0);
}

#[test]
fn test_lane_add_node() {
    let program = make_pubkey(0);
    let ix = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let node = GraphNode::new(0, vec![ix]);

    let mut lane = ExecutionLane::new(0);
    lane.add_node(&node);

    assert_eq!(lane.width(), 1);
    assert!(!lane.is_empty());
    assert!(lane.total_cu > 0);
    assert!(lane.combined_writes.contains(&make_pubkey(1)));
}

#[test]
fn test_lane_can_add_no_conflict() {
    let program = make_pubkey(0);

    let ix1 = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let ix2 = make_ix(program, &[], &[make_pubkey(2)], "w2");

    let node1 = GraphNode::new(0, vec![ix1]);
    let node2 = GraphNode::new(1, vec![ix2]);

    let mut lane = ExecutionLane::new(0);
    lane.add_node(&node1);

    // node2 writes to different account: can add
    assert!(lane.can_add(&node2));
}

#[test]
fn test_lane_can_add_write_write_conflict() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "w1");
    let ix2 = make_ix(program, &[], &[shared], "w2");

    let node1 = GraphNode::new(0, vec![ix1]);
    let node2 = GraphNode::new(1, vec![ix2]);

    let mut lane = ExecutionLane::new(0);
    lane.add_node(&node1);

    // Both write to same account: conflict
    assert!(!lane.can_add(&node2));
}

#[test]
fn test_lane_can_add_read_write_conflict() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "w1");
    let ix2 = make_ix(program, &[shared], &[], "r1");

    let node1 = GraphNode::new(0, vec![ix1]);
    let node2 = GraphNode::new(1, vec![ix2]);

    let mut lane = ExecutionLane::new(0);
    lane.add_node(&node1);

    // node1 writes, node2 reads: conflict
    assert!(!lane.can_add(&node2));
}

#[test]
fn test_lane_can_add_write_into_reads() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[shared], &[], "r1");
    let ix2 = make_ix(program, &[], &[shared], "w1");

    let node1 = GraphNode::new(0, vec![ix1]);
    let node2 = GraphNode::new(1, vec![ix2]);

    let mut lane = ExecutionLane::new(0);
    lane.add_node(&node1);

    // lane reads shared, node2 wants to write -> conflict
    assert!(!lane.can_add(&node2));
}

#[test]
fn test_lane_read_read_no_conflict() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[shared], &[], "r1");
    let ix2 = make_ix(program, &[shared], &[], "r2");

    let node1 = GraphNode::new(0, vec![ix1]);
    let node2 = GraphNode::new(1, vec![ix2]);

    let mut lane = ExecutionLane::new(0);
    lane.add_node(&node1);

    // Both read: no conflict
    assert!(lane.can_add(&node2));
}

// ---------------------------------------------------------------------------
// ExecutionPlan tests
// ---------------------------------------------------------------------------

#[test]
fn test_execution_plan_new_empty() {
    let plan = ExecutionPlan::new();
    assert_eq!(plan.num_lanes(), 0);
    assert_eq!(plan.total_transactions, 0);
    assert_eq!(plan.total_cu, 0);
}

#[test]
fn test_execution_plan_finalize() {
    let program = make_pubkey(0);

    let mut plan = ExecutionPlan::new();
    let node1 = GraphNode::new(0, vec![make_ix(program, &[], &[make_pubkey(1)], "w1")]);
    let node2 = GraphNode::new(1, vec![make_ix(program, &[], &[make_pubkey(2)], "w2")]);

    let mut lane = ExecutionLane::new(0);
    lane.add_node(&node1);
    lane.add_node(&node2);
    plan.lanes.push(lane);

    plan.finalize();

    assert_eq!(plan.num_lanes(), 1);
    assert_eq!(plan.total_transactions, 2);
    assert_eq!(plan.max_parallelism, 2);
    assert!(plan.total_cu > 0);
}

#[test]
fn test_execution_plan_avg_parallelism() {
    let mut plan = ExecutionPlan::new();

    // Lane 0 with 3 txs, lane 1 with 1 tx
    let mut lane0 = ExecutionLane::new(0);
    for i in 0..3 {
        lane0.add_node(&GraphNode::new(i, vec![]));
    }
    let mut lane1 = ExecutionLane::new(1);
    lane1.add_node(&GraphNode::new(3, vec![]));

    plan.lanes.push(lane0);
    plan.lanes.push(lane1);
    plan.finalize();

    assert_eq!(plan.total_transactions, 4);
    assert_eq!(plan.num_lanes(), 2);
    assert!((plan.avg_parallelism() - 2.0).abs() < 0.01);
}

#[test]
fn test_execution_plan_summary() {
    let mut plan = ExecutionPlan::new();
    let mut lane = ExecutionLane::new(0);
    lane.add_node(&GraphNode::new(0, vec![]));
    plan.lanes.push(lane);
    plan.finalize();

    let summary = plan.summary();
    assert!(summary.contains("ExecutionPlan"));
    assert!(summary.contains("1 lanes"));
}

// ---------------------------------------------------------------------------
// PriorityScheduler tests
// ---------------------------------------------------------------------------

#[test]
fn test_priority_scheduler_higher_cu_higher_priority() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(500));

    let scheduler = PriorityScheduler::new();
    let priorities = scheduler.compute_priorities(&graph).unwrap();

    // Node 1 has higher CU, so it should have higher priority
    assert!(priorities[&1].cu_score >= priorities[&0].cu_score);
}

#[test]
fn test_priority_scheduler_critical_path_nodes_prioritized() {
    let mut graph = TransactionGraph::new();
    // Chain: 0 -> 1 -> 2. Node 3 is independent with low CU.
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(200));
    graph.insert_node(GraphNode::new(2, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(3, vec![]).with_estimated_cu(10));

    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::DataDependency))
        .unwrap();

    let scheduler = PriorityScheduler::new();
    let priorities = scheduler.compute_priorities(&graph).unwrap();

    // Nodes 0, 1, 2 are on the critical path and should have higher priority than 3
    assert!(priorities[&0].is_critical || priorities[&1].is_critical);
    assert!(priorities[&1].priority > priorities[&3].priority);
}

#[test]
fn test_priority_scheduler_apply_priorities() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(500));

    let scheduler = PriorityScheduler::new();
    scheduler.apply_priorities(&mut graph).unwrap();

    // After applying, node priorities should be set
    let n0_pri = graph.nodes[&0].priority;
    let n1_pri = graph.nodes[&1].priority;
    assert!(n1_pri >= n0_pri);
}

#[test]
fn test_priority_scheduler_sorted_nodes() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(500));
    graph.insert_node(GraphNode::new(2, vec![]).with_estimated_cu(50));

    let scheduler = PriorityScheduler::new();
    let sorted = scheduler.sorted_nodes(&graph).unwrap();

    // Should be sorted by priority descending
    for window in sorted.windows(2) {
        assert!(window[0].1 >= window[1].1);
    }
}

#[test]
fn test_priority_scheduler_custom_weights() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));

    let scheduler = PriorityScheduler::new().with_weights(200.0, 100.0, 50.0);
    let priorities = scheduler.compute_priorities(&graph).unwrap();

    // With higher weights, the scores should reflect the increased weights
    assert!(priorities.contains_key(&0));
}

// ---------------------------------------------------------------------------
// ExecutionPlanner tests
// ---------------------------------------------------------------------------

#[test]
fn test_planner_empty_graph() {
    let graph = TransactionGraph::new();
    let planner = ExecutionPlanner::new();
    let plan = planner.plan(&graph).unwrap();

    assert_eq!(plan.num_lanes(), 0);
    assert_eq!(plan.total_transactions, 0);
}

#[test]
fn test_planner_single_node() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));

    let planner = ExecutionPlanner::new();
    let plan = planner.plan(&graph).unwrap();

    assert_eq!(plan.num_lanes(), 1);
    assert_eq!(plan.total_transactions, 1);
}

#[test]
fn test_planner_chain_produces_sequential_lanes() {
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

    let planner = ExecutionPlanner::new();
    let plan = planner.plan(&graph).unwrap();

    // Each node in its own level/lane
    assert!(plan.num_lanes() >= 3);
    assert_eq!(plan.total_transactions, 3);
}

#[test]
fn test_planner_parallel_nodes_in_same_lane() {
    let program = make_pubkey(0);

    let mut graph = TransactionGraph::new();
    // 3 independent nodes (no shared accounts)
    graph.insert_node(GraphNode::new(
        0,
        vec![make_ix(program, &[], &[make_pubkey(1)], "w1")],
    ));
    graph.insert_node(GraphNode::new(
        1,
        vec![make_ix(program, &[], &[make_pubkey(2)], "w2")],
    ));
    graph.insert_node(GraphNode::new(
        2,
        vec![make_ix(program, &[], &[make_pubkey(3)], "w3")],
    ));

    let planner = ExecutionPlanner::new();
    let plan = planner.plan(&graph).unwrap();

    // All nodes are independent and should be packed into a single lane
    assert_eq!(plan.num_lanes(), 1);
    assert_eq!(plan.total_transactions, 3);
}

#[test]
fn test_planner_conflicting_nodes_separate_lanes() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(
        0,
        vec![make_ix(program, &[], &[shared], "w1")],
    ));
    graph.insert_node(GraphNode::new(
        1,
        vec![make_ix(program, &[], &[shared], "w2")],
    ));

    // Add dependency edge
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();

    let planner = ExecutionPlanner::new();
    let plan = planner.plan(&graph).unwrap();

    // Should be in separate lanes since they conflict
    assert!(plan.num_lanes() >= 2);
}

#[test]
fn test_planner_diamond_graph() {
    let program = make_pubkey(0);

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(
        0,
        vec![make_ix(program, &[], &[make_pubkey(1)], "w1")],
    ));
    graph.insert_node(GraphNode::new(
        1,
        vec![make_ix(program, &[], &[make_pubkey(2)], "w2")],
    ));
    graph.insert_node(GraphNode::new(
        2,
        vec![make_ix(program, &[], &[make_pubkey(3)], "w3")],
    ));
    graph.insert_node(GraphNode::new(
        3,
        vec![make_ix(program, &[make_pubkey(2), make_pubkey(3)], &[], "r23")],
    ));

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

    let planner = ExecutionPlanner::new();
    let plan = planner.plan(&graph).unwrap();

    assert_eq!(plan.total_transactions, 4);
    // Nodes 1 and 2 can run in parallel, so expect >= 2 and <= 4 lanes
    assert!(plan.num_lanes() >= 2);
}

#[test]
fn test_planner_max_lane_width() {
    let planner = ExecutionPlanner::new().with_max_lane_width(1);

    let mut graph = TransactionGraph::new();
    for i in 0..3 {
        graph.insert_node(GraphNode::new(i, vec![]).with_estimated_cu(100));
    }

    let plan = planner.plan(&graph).unwrap();
    // With max width 1, each node gets its own lane
    assert_eq!(plan.num_lanes(), 3);
}

#[test]
fn test_planner_optimized() {
    let program = make_pubkey(0);

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(
        0,
        vec![make_ix(program, &[], &[make_pubkey(1)], "w1")],
    ));
    graph.insert_node(GraphNode::new(
        1,
        vec![make_ix(program, &[], &[make_pubkey(2)], "w2")],
    ));

    let planner = ExecutionPlanner::new();
    let plan = planner.plan_optimized(&graph).unwrap();

    assert_eq!(plan.total_transactions, 2);
    // Optimization may merge lanes
    assert!(plan.num_lanes() >= 1);
}

// ---------------------------------------------------------------------------
// Full IvzaEngine pipeline tests
// ---------------------------------------------------------------------------

#[test]
fn test_engine_process_independent() {
    let engine = IvzaEngine::new();
    let program = make_pubkey(0);

    let ix1 = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let ix2 = make_ix(program, &[], &[make_pubkey(2)], "w2");

    let plan = engine.process(vec![ix1, ix2]).unwrap();

    assert!(plan.total_transactions >= 2);
    assert!(plan.num_lanes() >= 1);
}

#[test]
fn test_engine_process_conflicting() {
    let engine = IvzaEngine::new();
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "w_shared_1");
    let ix2 = make_ix(program, &[], &[shared], "w_shared_2");

    let plan = engine.process(vec![ix1, ix2]).unwrap();

    assert_eq!(plan.total_transactions, 2);
    // Conflicting instructions should produce multiple lanes
    assert!(plan.num_lanes() >= 2);
}

#[test]
fn test_engine_process_graph() {
    let engine = IvzaEngine::new();

    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(200));
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();

    let plan = engine.process_graph(&graph).unwrap();
    assert_eq!(plan.total_transactions, 2);
}

#[test]
fn test_engine_process_optimized() {
    let engine = IvzaEngine::new();
    let program = make_pubkey(0);

    let ix1 = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let ix2 = make_ix(program, &[], &[make_pubkey(2)], "w2");

    let plan = engine.process_optimized(vec![ix1, ix2]).unwrap();
    assert!(plan.total_transactions >= 2);
}

#[test]
fn test_engine_parallelism_summary() {
    let engine = IvzaEngine::new();
    let program = make_pubkey(0);

    let ix1 = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let ix2 = make_ix(program, &[], &[make_pubkey(2)], "w2");
    let ix3 = make_ix(program, &[make_pubkey(1)], &[make_pubkey(3)], "rw");

    let summary = engine.parallelism_summary(vec![ix1, ix2, ix3]).unwrap();

    assert!(summary.total_nodes >= 2);
    assert!(summary.speedup_ratio >= 1.0);
    assert!(summary.num_lanes >= 1);

    // Test Display impl
    let display = format!("{}", summary);
    assert!(display.contains("iVZA Parallelism Summary"));
}

#[test]
fn test_engine_default() {
    let engine = IvzaEngine::default();
    // Should work the same as ::new()
    let program = make_pubkey(0);
    let ix = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let plan = engine.process(vec![ix]).unwrap();
    assert_eq!(plan.total_transactions, 1);
}
