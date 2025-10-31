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