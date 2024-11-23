//! Comprehensive tests for the ivza-core graph module.
//!
//! Tests cover: TransactionGraphBuilder, TransactionGraph, topological sort,
//! cycle detection, GraphDecomposer, GraphNode, and GraphEdge.

use ivza_core::graph::{
    GraphDecomposer, GraphEdge, GraphNode, GraphNodeBuilder, TransactionGraph,
    TransactionGraphBuilder,
};
use ivza_core::types::{
    AccountAccess, AccountAccessEntry, AccountSet, DependencyType, InstructionData, NodeId,
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
// TransactionGraphBuilder tests
// ---------------------------------------------------------------------------

#[test]
fn test_builder_empty_graph() {
    let builder = TransactionGraphBuilder::new();
    let graph = builder.build().unwrap();
    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn test_builder_single_node() {
    let mut builder = TransactionGraphBuilder::new();
    let program = make_pubkey(0);
    let ix = make_ix(program, &[], &[make_pubkey(1)], "write_1");
    let id = builder.add_node(vec![ix]);
    assert_eq!(id, 0);

    let graph = builder.build().unwrap();
    assert_eq!(graph.node_count(), 1);
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn test_builder_add_labeled_node() {
    let mut builder = TransactionGraphBuilder::new();
    let program = make_pubkey(0);
    let ix = make_ix(program, &[make_pubkey(1)], &[], "read");
    let id = builder.add_labeled_node("my_node", vec![ix]);

    let graph = builder.build().unwrap();
    let node = graph.nodes.get(&id).unwrap();
    assert_eq!(node.label.as_deref(), Some("my_node"));
}

#[test]
fn test_builder_add_node_with_cu() {
    let mut builder = TransactionGraphBuilder::new();
    let id = builder.add_node_with_cu(vec![], 500_000);

    let graph = builder.build().unwrap();
    let node = graph.nodes.get(&id).unwrap();
    assert_eq!(node.estimated_cu, 500_000);
}

#[test]
fn test_builder_add_graph_node() {
    let mut builder = TransactionGraphBuilder::new();
    let node = GraphNode::new(42, vec![]).with_label("custom").with_estimated_cu(999);
    let id = builder.add_graph_node(node);
    assert_eq!(id, 42);

    let graph = builder.build().unwrap();
    assert_eq!(graph.nodes.get(&42).unwrap().estimated_cu, 999);
}

#[test]
fn test_builder_add_edges() {
    let mut builder = TransactionGraphBuilder::new();
    let n0 = builder.add_node(vec![]);
    let n1 = builder.add_node(vec![]);
    let n2 = builder.add_node(vec![]);

    builder.add_data_dependency(n0, n1).unwrap();
    builder.add_order_dependency(n1, n2).unwrap();

    let graph = builder.build().unwrap();
    assert_eq!(graph.edge_count(), 2);
    assert!(graph.edges[0].is_data_dependency());
    assert!(graph.edges[1].is_order_dependency());
}

#[test]
fn test_builder_add_account_conflict_edge() {
    let mut builder = TransactionGraphBuilder::new();
    let n0 = builder.add_node(vec![]);
    let n1 = builder.add_node(vec![]);
    builder.add_account_conflict(n0, n1).unwrap();

    let graph = builder.build().unwrap();
    assert!(graph.edges[0].is_account_conflict());
}

#[test]
fn test_builder_rejects_cycle() {
    let mut builder = TransactionGraphBuilder::new();
    let n0 = builder.add_node(vec![]);
    let n1 = builder.add_node(vec![]);
    builder
        .add_edge(n0, n1, DependencyType::OrderDependency)
        .unwrap();
    builder
        .add_edge(n1, n0, DependencyType::OrderDependency)
        .unwrap();

    let result = builder.build();
    assert!(result.is_err());
}

#[test]
fn test_builder_build_unchecked_allows_cycle() {
    let mut builder = TransactionGraphBuilder::new();
    let n0 = builder.add_node(vec![]);
    let n1 = builder.add_node(vec![]);
    builder
        .add_edge(n0, n1, DependencyType::OrderDependency)
        .unwrap();
    builder
        .add_edge(n1, n0, DependencyType::OrderDependency)
        .unwrap();

    let graph = builder.build_unchecked();
    assert!(graph.has_cycle());
}

#[test]
fn test_builder_edge_to_nonexistent_node_fails() {
    let mut builder = TransactionGraphBuilder::new();
    let n0 = builder.add_node(vec![]);
    let result = builder.add_edge(n0, 999, DependencyType::DataDependency);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Topological sort tests
// ---------------------------------------------------------------------------

#[test]
fn test_topo_sort_simple_chain() {
    // 0 -> 1 -> 2
    let mut graph = TransactionGraph::new();
    for i in 0..3 {
        graph.insert_node(GraphNode::new(i, vec![]));
    }
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::DataDependency))
        .unwrap();

    let topo = graph.topological_sort().unwrap();
    assert_eq!(topo, vec![0, 1, 2]);
}

#[test]
fn test_topo_sort_diamond_dag() {
    // 0 -> 1, 0 -> 2, 1 -> 3, 2 -> 3
    let mut graph = TransactionGraph::new();
    for i in 0..4 {
        graph.insert_node(GraphNode::new(i, vec![]));
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

    let topo = graph.topological_sort().unwrap();
    assert_eq!(topo.len(), 4);
    // 0 must be first, 3 must be last
    assert_eq!(topo[0], 0);
    assert_eq!(topo[3], 3);
    // 1 and 2 must come between 0 and 3
    let pos_1 = topo.iter().position(|&x| x == 1).unwrap();
    let pos_2 = topo.iter().position(|&x| x == 2).unwrap();
    assert!(pos_1 > 0 && pos_1 < 3);
    assert!(pos_2 > 0 && pos_2 < 3);
}

#[test]
fn test_topo_sort_parallel_branches() {
    // 0 -> 2, 1 -> 3 (two independent chains)
    let mut graph = TransactionGraph::new();
    for i in 0..4 {
        graph.insert_node(GraphNode::new(i, vec![]));
    }
    graph
        .add_edge(GraphEdge::new(0, 2, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 3, DependencyType::DataDependency))
        .unwrap();

    let topo = graph.topological_sort().unwrap();
    assert_eq!(topo.len(), 4);

    let pos_0 = topo.iter().position(|&x| x == 0).unwrap();
    let pos_2 = topo.iter().position(|&x| x == 2).unwrap();
    assert!(pos_0 < pos_2);

    let pos_1 = topo.iter().position(|&x| x == 1).unwrap();
    let pos_3 = topo.iter().position(|&x| x == 3).unwrap();
    assert!(pos_1 < pos_3);
}

#[test]
fn test_topo_sort_single_node() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]));
    let topo = graph.topological_sort().unwrap();
    assert_eq!(topo, vec![0]);
}

#[test]
fn test_topo_sort_empty_graph() {
    let graph = TransactionGraph::new();
    let topo = graph.topological_sort().unwrap();
    assert!(topo.is_empty());
}

// ---------------------------------------------------------------------------
// Cycle detection tests
// ---------------------------------------------------------------------------

#[test]
fn test_no_cycle_simple_dag() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]));
    graph.insert_node(GraphNode::new(1, vec![]));
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    assert!(!graph.has_cycle());
}

#[test]
fn test_cycle_two_nodes() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]));
    graph.insert_node(GraphNode::new(1, vec![]));
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::OrderDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 0, DependencyType::OrderDependency))
        .unwrap();

    assert!(graph.has_cycle());
    assert!(graph.topological_sort().is_none());
}

#[test]
fn test_cycle_three_nodes() {
    let mut graph = TransactionGraph::new();
    for i in 0..3 {
        graph.insert_node(GraphNode::new(i, vec![]));
    }
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::OrderDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::OrderDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(2, 0, DependencyType::OrderDependency))
        .unwrap();

    assert!(graph.has_cycle());
}

#[test]
fn test_self_loop_is_cycle() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]));
    graph
        .add_edge(GraphEdge::new(0, 0, DependencyType::OrderDependency))
        .unwrap();
    assert!(graph.has_cycle());
}

#[test]
fn test_disconnected_acyclic() {
    let mut graph = TransactionGraph::new();
    for i in 0..4 {
        graph.insert_node(GraphNode::new(i, vec![]));
    }
    // No edges at all -- fully disconnected
    assert!(!graph.has_cycle());
}

// ---------------------------------------------------------------------------
// Graph structure queries
// ---------------------------------------------------------------------------

#[test]
fn test_root_and_leaf_nodes() {
    let mut graph = TransactionGraph::new();
    for i in 0..3 {
        graph.insert_node(GraphNode::new(i, vec![]));
    }
    graph
        .add_edge(GraphEdge::new(0, 1, DependencyType::DataDependency))
        .unwrap();
    graph
        .add_edge(GraphEdge::new(1, 2, DependencyType::DataDependency))
        .unwrap();

    let mut roots = graph.root_nodes();
    roots.sort();
    assert_eq!(roots, vec![0]);

    let mut leaves = graph.leaf_nodes();
    leaves.sort();
    assert_eq!(leaves, vec![2]);
}

#[test]
fn test_successors_and_predecessors() {
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

    let mut succs = graph.successors(0);
    succs.sort();
    assert_eq!(succs, vec![1, 2]);
    assert_eq!(graph.predecessors(3), vec![1]);
    assert_eq!(graph.in_degree(0), 0);
    assert_eq!(graph.out_degree(0), 2);
    assert_eq!(graph.in_degree(3), 1);
}

#[test]
fn test_total_estimated_cu() {
    let mut graph = TransactionGraph::new();
    graph.insert_node(GraphNode::new(0, vec![]).with_estimated_cu(100));
    graph.insert_node(GraphNode::new(1, vec![]).with_estimated_cu(250));
    graph.insert_node(GraphNode::new(2, vec![]).with_estimated_cu(50));
    assert_eq!(graph.total_estimated_cu(), 400);
}

// ---------------------------------------------------------------------------
// GraphNode tests
// ---------------------------------------------------------------------------

#[test]
fn test_graph_node_conflicts_with() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix_a = make_ix(program, &[], &[shared], "write_shared");
    let ix_b = make_ix(program, &[shared], &[], "read_shared");

    let node_a = GraphNode::new(0, vec![ix_a]);
    let node_b = GraphNode::new(1, vec![ix_b]);
    assert!(node_a.conflicts_with(&node_b));
}

#[test]
fn test_graph_node_no_conflict_read_read() {
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix_a = make_ix(program, &[shared], &[], "read_a");
    let ix_b = make_ix(program, &[shared], &[], "read_b");

    let node_a = GraphNode::new(0, vec![ix_a]);
    let node_b = GraphNode::new(1, vec![ix_b]);
    assert!(!node_a.conflicts_with(&node_b));
}

#[test]
fn test_graph_node_write_set_and_read_set() {
    let program = make_pubkey(0);
    let read_acct = make_pubkey(1);
    let write_acct = make_pubkey(2);

    let ix = make_ix(program, &[read_acct], &[write_acct], "mixed");
    let node = GraphNode::new(0, vec![ix]);

    assert!(node.write_set().contains(&write_acct));
    assert!(node.read_set().contains(&read_acct));
    assert!(!node.write_set().contains(&read_acct));
}

#[test]
fn test_graph_node_program_ids() {
    let prog_a = make_pubkey(10);
    let prog_b = make_pubkey(20);
    let ix_a = InstructionData::new(prog_a, vec![], vec![]);
    let ix_b = InstructionData::new(prog_b, vec![], vec![]);

    let node = GraphNode::new(0, vec![ix_a, ix_b]);
    let ids = node.program_ids();
    assert_eq!(ids.len(), 2);
}

#[test]
fn test_graph_node_builder() {
    let program = make_pubkey(0);
    let ix = make_ix(program, &[], &[make_pubkey(1)], "test");
    let node = GraphNodeBuilder::new(5)
        .instruction(ix)
        .priority(10)
        .estimated_cu(42_000)
        .label("my_node")
        .build();

    assert_eq!(node.id, 5);
    assert_eq!(node.priority, 10);
    assert_eq!(node.estimated_cu, 42_000);
    assert_eq!(node.label.as_deref(), Some("my_node"));
    assert_eq!(node.instructions.len(), 1);
}

// ---------------------------------------------------------------------------
// GraphEdge tests
// ---------------------------------------------------------------------------

#[test]
fn test_edge_constructors() {
    let e1 = GraphEdge::data_dependency(0, 1);
    assert!(e1.is_data_dependency());
    assert!(!e1.is_order_dependency());

    let e2 = GraphEdge::order_dependency(1, 2);
    assert!(e2.is_order_dependency());

    let account = make_pubkey(1);
    let e3 = GraphEdge::account_conflict(2, 3, vec![account]);
    assert!(e3.is_account_conflict());
    assert_eq!(e3.conflicting_accounts.len(), 1);
}

#[test]
fn test_edge_with_weight() {
    let edge = GraphEdge::new(0, 1, DependencyType::DataDependency).with_weight(5.0);
    assert_eq!(edge.weight, 5.0);
}

#[test]
fn test_edge_auto_detected_flag() {
    let edge = GraphEdge::new(0, 1, DependencyType::AccountConflict).with_auto_detected(true);
    assert!(edge.auto_detected);
}

// ---------------------------------------------------------------------------
// GraphDecomposer tests
// ---------------------------------------------------------------------------

#[test]
fn test_decomposer_single_instruction() {
    let decomposer = GraphDecomposer::new();
    let program = make_pubkey(0);
    let ix = make_ix(program, &[], &[make_pubkey(1)], "w1");

    let graph = decomposer.decompose(vec![ix]).unwrap();
    assert_eq!(graph.node_count(), 1);
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn test_decomposer_independent_instructions() {
    let decomposer = GraphDecomposer::new();
    let program = make_pubkey(0);

    let ix1 = make_ix(program, &[], &[make_pubkey(1)], "w1");
    let ix2 = make_ix(program, &[], &[make_pubkey(2)], "w2");
    let ix3 = make_ix(program, &[], &[make_pubkey(3)], "w3");

    let graph = decomposer.decompose(vec![ix1, ix2, ix3]).unwrap();
    // All independent -- should have no edges
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn test_decomposer_conflicting_write_write() {
    let decomposer = GraphDecomposer::new();
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[], &[shared], "w_shared_1");
    let ix2 = make_ix(program, &[], &[shared], "w_shared_2");

    let graph = decomposer.decompose(vec![ix1, ix2]).unwrap();
    // Conflicting on the same account -- must have an edge
    assert!(graph.edge_count() >= 1);
}

#[test]
fn test_decomposer_read_write_conflict() {
    let decomposer = GraphDecomposer::new();
    let program = make_pubkey(0);
    let shared = make_pubkey(1);

    let ix1 = make_ix(program, &[shared], &[], "read_shared");
    let ix2 = make_ix(program, &[], &[shared], "write_shared");

    let graph = decomposer.decompose(vec![ix1, ix2]).unwrap();
    assert!(graph.edge_count() >= 1);
}

#[test]
fn test_decomposer_mixed_parallel_and_sequential() {
    let decomposer = GraphDecomposer::new();
    let program = make_pubkey(0);

    // ix0 writes A, ix1 writes B (parallel), ix2 reads A and B (depends on both)
    let ix0 = make_ix(program, &[], &[make_pubkey(1)], "write_a");
    let ix1 = make_ix(program, &[], &[make_pubkey(2)], "write_b");
    let ix2 = make_ix(program, &[make_pubkey(1), make_pubkey(2)], &[], "read_ab");

    let graph = decomposer.decompose(vec![ix0, ix1, ix2]).unwrap();

    // ix2 depends on both ix0 and ix1; ix0 and ix1 are independent
    assert!(!graph.has_cycle());
    let topo = graph.topological_sort().unwrap();
    assert_eq!(topo.len(), graph.node_count());
}

#[test]
fn test_decomposer_max_instructions_per_node() {
    let decomposer = GraphDecomposer::new().with_max_instructions_per_node(2);
    let program = make_pubkey(0);

    // 4 independent instructions should split into 2 nodes of 2
    let ixs: Vec<InstructionData> = (1..=4)
        .map(|i| make_ix(program, &[], &[make_pubkey(i as u8)], &format!("w{}", i)))
        .collect();

    let graph = decomposer.decompose(ixs).unwrap();
    // With 4 independent instructions and max 2 per node, expect 2 nodes
    assert!(graph.node_count() >= 2);
}

#[test]
fn test_decomposer_decompose_multiple() {
    let decomposer = GraphDecomposer::new();
    let program = make_pubkey(0);

    let set1 = vec![make_ix(program, &[], &[make_pubkey(1)], "a")];
    let set2 = vec![make_ix(program, &[], &[make_pubkey(2)], "b")];

    let graph = decomposer.decompose_multiple(vec![set1, set2]).unwrap();
    assert!(graph.node_count() >= 2);
    // Two independent sets should have no edges between them
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn test_decomposer_preserves_labels() {
    let decomposer = GraphDecomposer::new();
    let program = make_pubkey(0);
    let ix = make_ix(program, &[], &[make_pubkey(1)], "my_label");

    let graph = decomposer.decompose(vec![ix]).unwrap();
    // The decomposer assigns level-based labels, and the original ix label is preserved in the ix
    let node = graph.nodes.values().next().unwrap();
    assert!(node.label.is_some());
    assert_eq!(node.instructions[0].label.as_deref(), Some("my_label"));
}
