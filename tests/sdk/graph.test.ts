/**
 * Comprehensive tests for the iVZA SDK TransactionGraph, GraphNode,
 * TransactionGraphBuilder, topological sort, cycle detection,
 * independent groups, scheduling, and serialization.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { PublicKey } from '@solana/web3.js';
import {
  TransactionGraph,
  TransactionGraphBuilder,
  GraphNode,
} from '../../sdk/src/graph';
import {
  DependencyType,
  PriorityLevel,
  AccountAccess,
} from '../../sdk/src/types';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makePubkey(seed: number): PublicKey {
  const bytes = new Uint8Array(32).fill(seed);
  return new PublicKey(bytes);
}

function makeGraphNode(
  id: string,
  writes: PublicKey[] = [],
  reads: PublicKey[] = [],
  cu = 200_000
): GraphNode {
  const accesses: AccountAccess[] = [
    ...writes.map((p) => ({ pubkey: p, isWritable: true, isSigner: false })),
    ...reads.map((p) => ({ pubkey: p, isWritable: false, isSigner: false })),
  ];
  return new GraphNode({
    id,
    programId: makePubkey(0),
    accountAccesses: accesses,
    estimatedCu: cu,
    priority: PriorityLevel.Medium,
    label: id,
  });
}

// ---------------------------------------------------------------------------
// TransactionGraph: basic operations
// ---------------------------------------------------------------------------

describe('TransactionGraph', () => {
  let graph: TransactionGraph;

  beforeEach(() => {
    graph = new TransactionGraph('test_graph');
  });

  it('starts empty', () => {
    expect(graph.nodeCount).toBe(0);
    expect(graph.edgeCount).toBe(0);
    expect(graph.id).toBe('test_graph');
  });

  it('adds nodes', () => {
    const node = makeGraphNode('n1');
    graph.addNode(node);
    expect(graph.nodeCount).toBe(1);
    expect(graph.getNode('n1')).toBeDefined();
  });

  it('throws on duplicate node', () => {
    graph.addNode(makeGraphNode('n1'));
    expect(() => graph.addNode(makeGraphNode('n1'))).toThrow();
  });

  it('adds edges', () => {
    graph.addNode(makeGraphNode('n1'));
    graph.addNode(makeGraphNode('n2'));
    graph.addEdge({
      from: 'n1',
      to: 'n2',
      dependencyType: DependencyType.Explicit,
    });
    expect(graph.edgeCount).toBe(1);
  });

  it('throws on edge with missing node', () => {
    graph.addNode(makeGraphNode('n1'));
    expect(() =>
      graph.addEdge({
        from: 'n1',
        to: 'missing',
        dependencyType: DependencyType.Explicit,
      })
    ).toThrow();
  });

  it('skips duplicate edges', () => {
    graph.addNode(makeGraphNode('n1'));
    graph.addNode(makeGraphNode('n2'));
    const edge = {
      from: 'n1',
      to: 'n2',
      dependencyType: DependencyType.Explicit,
    };
    graph.addEdge(edge);
    graph.addEdge(edge);
    expect(graph.edgeCount).toBe(1);
  });

  it('removes nodes and their edges', () => {
    graph.addNode(makeGraphNode('n1'));
    graph.addNode(makeGraphNode('n2'));
    graph.addEdge({
      from: 'n1',
      to: 'n2',
      dependencyType: DependencyType.Explicit,
    });
    graph.removeNode('n1');
    expect(graph.nodeCount).toBe(1);
    expect(graph.edgeCount).toBe(0);
  });

  it('getNeighbors returns outgoing neighbors', () => {
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'a', to: 'c', dependencyType: DependencyType.Explicit });

    const neighbors = graph.getNeighbors('a');
    expect(neighbors.length).toBe(2);
  });

  it('getPredecessors returns incoming neighbors', () => {
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });

    const preds = graph.getPredecessors('b');
    expect(preds.length).toBe(1);
    expect(preds[0].id).toBe('a');
  });

  it('getInDegree and getOutDegree', () => {
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'a', to: 'c', dependencyType: DependencyType.Explicit });

    expect(graph.getInDegree('a')).toBe(0);
    expect(graph.getOutDegree('a')).toBe(2);
    expect(graph.getInDegree('b')).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// Topological sort
// ---------------------------------------------------------------------------

describe('Topological Sort', () => {
  it('sorts a simple chain', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'b', to: 'c', dependencyType: DependencyType.Explicit });

    const sorted = graph.topologicalSort();
    const ids = sorted.map((n) => n.id);
    expect(ids.indexOf('a')).toBeLessThan(ids.indexOf('b'));
    expect(ids.indexOf('b')).toBeLessThan(ids.indexOf('c'));
  });

  it('sorts a diamond DAG', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('root'));
    graph.addNode(makeGraphNode('left'));
    graph.addNode(makeGraphNode('right'));
    graph.addNode(makeGraphNode('sink'));

    graph.addEdge({ from: 'root', to: 'left', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'root', to: 'right', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'left', to: 'sink', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'right', to: 'sink', dependencyType: DependencyType.Explicit });

    const sorted = graph.topologicalSort();
    const ids = sorted.map((n) => n.id);
    expect(ids[0]).toBe('root');
    expect(ids[3]).toBe('sink');
  });

  it('sorts with priority awareness', () => {
    const graph = new TransactionGraph();
    const nodeHigh = new GraphNode({
      id: 'high',
      programId: makePubkey(0),
      estimatedCu: 100,
      priority: PriorityLevel.High,
    });
    const nodeLow = new GraphNode({
      id: 'low',
      programId: makePubkey(0),
      estimatedCu: 100,
      priority: PriorityLevel.Low,
    });
    graph.addNode(nodeHigh);
    graph.addNode(nodeLow);

    const sorted = graph.topologicalSort({ priorityAware: true, cuAware: false });
    // High priority should come first when no deps constrain order
    expect(sorted[0].id).toBe('high');
  });

  it('throws on cyclic graph', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'b', to: 'a', dependencyType: DependencyType.Explicit });

    expect(() => graph.topologicalSort()).toThrow();
  });

  it('handles single node', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('only'));
    const sorted = graph.topologicalSort();
    expect(sorted.length).toBe(1);
    expect(sorted[0].id).toBe('only');
  });
});

// ---------------------------------------------------------------------------
// Cycle detection
// ---------------------------------------------------------------------------

describe('Cycle Detection', () => {
  it('detects no cycle in DAG', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });

    expect(graph.hasCycle()).toBe(false);
    const result = graph.detectCycle();
    expect(result.hasCycle).toBe(false);
    expect(result.cycleNodes.length).toBe(0);
  });

  it('detects cycle in two-node loop', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'b', to: 'a', dependencyType: DependencyType.Explicit });

    expect(graph.hasCycle()).toBe(true);
    const result = graph.detectCycle();
    expect(result.hasCycle).toBe(true);
    expect(result.cycleNodes.length).toBeGreaterThan(0);
  });

  it('detects cycle in three-node ring', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'b', to: 'c', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'c', to: 'a', dependencyType: DependencyType.Explicit });

    expect(graph.hasCycle()).toBe(true);
  });

  it('disconnected acyclic graph has no cycle', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    // No edges
    expect(graph.hasCycle()).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Independent groups
// ---------------------------------------------------------------------------

describe('Independent Groups', () => {
  it('finds single group for connected graph', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });

    const groups = graph.getIndependentGroups();
    expect(groups.length).toBe(1);
    expect(groups[0].length).toBe(2);
  });

  it('finds multiple groups for disconnected graph', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));
    graph.addNode(makeGraphNode('d'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'c', to: 'd', dependencyType: DependencyType.Explicit });

    const groups = graph.getIndependentGroups();
    expect(groups.length).toBe(2);
  });

  it('each isolated node is its own group', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));

    const groups = graph.getIndependentGroups();
    expect(groups.length).toBe(3);
  });
});

// ---------------------------------------------------------------------------
// Auto-detect dependencies
// ---------------------------------------------------------------------------

describe('Auto-detect Dependencies', () => {
  it('detects write-after-write', () => {
    const graph = new TransactionGraph();
    const shared = makePubkey(1);
    graph.addNode(makeGraphNode('a', [shared]));
    graph.addNode(makeGraphNode('b', [shared]));

    graph.autoDetectDependencies();
    expect(graph.edgeCount).toBe(1);
    const edges = graph.getEdges();
    expect(edges[0].dependencyType).toBe(DependencyType.WriteAfterWrite);
  });

  it('detects read-after-write', () => {
    const graph = new TransactionGraph();
    const shared = makePubkey(1);
    graph.addNode(makeGraphNode('a', [shared]));
    graph.addNode(makeGraphNode('b', [], [shared]));

    graph.autoDetectDependencies();
    expect(graph.edgeCount).toBe(1);
    expect(graph.getEdges()[0].dependencyType).toBe(DependencyType.ReadAfterWrite);
  });

  it('no dependency for read-read', () => {
    const graph = new TransactionGraph();
    const shared = makePubkey(1);
    graph.addNode(makeGraphNode('a', [], [shared]));
    graph.addNode(makeGraphNode('b', [], [shared]));

    graph.autoDetectDependencies();
    expect(graph.edgeCount).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// Critical path
// ---------------------------------------------------------------------------

describe('Critical Path', () => {
  it('computes critical path for chain', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a', [], [], 100));
    graph.addNode(makeGraphNode('b', [], [], 200));
    graph.addNode(makeGraphNode('c', [], [], 50));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'b', to: 'c', dependencyType: DependencyType.Explicit });

    const cp = graph.getCriticalPath();
    expect(cp.totalCu).toBe(350);
    expect(cp.nodes.length).toBe(3);
  });

  it('critical path goes through heaviest branch', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('root', [], [], 100));
    graph.addNode(makeGraphNode('heavy', [], [], 500));
    graph.addNode(makeGraphNode('light', [], [], 10));
    graph.addEdge({ from: 'root', to: 'heavy', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'root', to: 'light', dependencyType: DependencyType.Explicit });

    const cp = graph.getCriticalPath();
    expect(cp.totalCu).toBe(600);
    const ids = cp.nodes.map((n) => n.id);
    expect(ids).toContain('root');
    expect(ids).toContain('heavy');
  });
});

// ---------------------------------------------------------------------------
// Scheduling
// ---------------------------------------------------------------------------

describe('Scheduling', () => {
  it('schedules single node into one lane', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    const plan = graph.schedule();
    expect(plan.lanes.length).toBe(1);
    expect(plan.lanes[0].nodes.length).toBe(1);
  });

  it('schedules chain into lanes', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a', [], [], 100_000));
    graph.addNode(makeGraphNode('b', [], [], 100_000));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });

    const plan = graph.schedule();
    expect(plan.parallelismDegree).toBeGreaterThanOrEqual(1);
    expect(plan.totalEstimatedCu).toBe(200_000);
  });

  it('respects maxLanes', () => {
    const graph = new TransactionGraph();
    for (let i = 0; i < 10; i++) {
      graph.addNode(makeGraphNode(`n${i}`, [], [], 10_000));
    }
    const plan = graph.schedule({ maxLanes: 2 });
    expect(plan.lanes.length).toBeLessThanOrEqual(2);
  });

  it('throws on cyclic graph', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'b', to: 'a', dependencyType: DependencyType.Explicit });

    expect(() => graph.schedule()).toThrow();
  });
});

// ---------------------------------------------------------------------------
// Serialization round-trip
// ---------------------------------------------------------------------------

describe('Serialization', () => {
  it('round-trips graph through serialize/deserialize', () => {
    const graph = new TransactionGraph('roundtrip');
    graph.addNode(makeGraphNode('n1', [makePubkey(1)], [makePubkey(2)], 300_000));
    graph.addNode(makeGraphNode('n2', [makePubkey(3)], [], 150_000));
    graph.addEdge({ from: 'n1', to: 'n2', dependencyType: DependencyType.Explicit });

    const serialized = graph.serialize();
    expect(serialized.version).toBe(1);
    expect(serialized.nodes.length).toBe(2);
    expect(serialized.edges.length).toBe(1);

    const deserialized = TransactionGraph.deserialize(serialized);
    expect(deserialized.nodeCount).toBe(2);
    expect(deserialized.edgeCount).toBe(1);
    expect(deserialized.getNode('n1')).toBeDefined();
    expect(deserialized.getNode('n2')).toBeDefined();
  });

  it('preserves node metadata', () => {
    const graph = new TransactionGraph();
    const node = new GraphNode({
      id: 'meta',
      programId: makePubkey(0),
      estimatedCu: 42_000,
      priority: PriorityLevel.High,
      label: 'test_label',
      metadata: { foo: 'bar' },
    });
    graph.addNode(node);

    const serialized = graph.serialize();
    const sn = serialized.nodes[0];
    expect(sn.label).toBe('test_label');
    expect(sn.estimatedCu).toBe(42_000);
    expect(sn.metadata?.foo).toBe('bar');
  });
});

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

describe('Analysis', () => {
  it('returns correct statistics', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a', [makePubkey(1)], [], 100_000));
    graph.addNode(makeGraphNode('b', [makePubkey(1)], [], 200_000));
    graph.autoDetectDependencies();

    const result = graph.analyze();
    expect(result.nodeCount).toBe(2);
    expect(result.edgeCount).toBe(1);
    expect(result.totalCu).toBe(300_000);
    expect(result.hasCycles).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// TransactionGraphBuilder
// ---------------------------------------------------------------------------

describe('TransactionGraphBuilder', () => {
  it('builds a swap node', () => {
    const graph = new TransactionGraphBuilder()
      .swap()
        .from('USDC')
        .to('SOL')
        .amount(100)
      .build();

    expect(graph.nodeCount).toBe(1);
    const nodes = graph.getNodes();
    expect(nodes[0].metadata?.type).toBe('swap');
  });

  it('chains swap then stake with explicit dependency', () => {
    const graph = new TransactionGraphBuilder()
      .swap()
        .from('USDC')
        .to('SOL')
        .amount(100)
      .then()
      .stake()
        .amount(1)
      .build();

    expect(graph.nodeCount).toBe(2);
    expect(graph.edgeCount).toBe(1);
  });

  it('builds transfer node', () => {
    const graph = new TransactionGraphBuilder()
      .transfer()
        .token('SOL')
        .amount(5)
        .to('11111111111111111111111111111111')
      .build();

    expect(graph.nodeCount).toBe(1);
    const nodes = graph.getNodes();
    expect(nodes[0].metadata?.type).toBe('transfer');
  });

  it('builds provide-liquidity node', () => {
    const graph = new TransactionGraphBuilder()
      .provideLiquidity()
        .tokenA('USDC')
        .tokenB('SOL')
        .amounts(100, 0.5)
      .build();

    expect(graph.nodeCount).toBe(1);
    const nodes = graph.getNodes();
    expect(nodes[0].metadata?.type).toBe('provide_liquidity');
  });

  it('auto-detects dependencies', () => {
    const builder = new TransactionGraphBuilder();
    builder.swap().from('USDC').to('SOL').amount(100).build();
    // autoDetectDependencies is called on the graph after building
    const graph = new TransactionGraphBuilder()
      .swap().from('USDC').to('SOL').amount(100)
      .then()
      .swap().from('SOL').to('RAY').amount(50)
      .build();

    // Should have an explicit dependency edge from chaining
    expect(graph.edgeCount).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// GraphNode
// ---------------------------------------------------------------------------

describe('GraphNode', () => {
  it('detects write-after-write conflict', () => {
    const shared = makePubkey(1);
    const a = makeGraphNode('a', [shared]);
    const b = makeGraphNode('b', [shared]);

    expect(a.conflictsWith(b)).toBe(true);
    expect(a.getDependencyWith(b)).toBe(DependencyType.WriteAfterWrite);
  });

  it('detects read-after-write conflict', () => {
    const shared = makePubkey(1);
    const a = makeGraphNode('a', [shared]);
    const b = makeGraphNode('b', [], [shared]);

    expect(a.conflictsWith(b)).toBe(true);
    expect(a.getDependencyWith(b)).toBe(DependencyType.ReadAfterWrite);
  });

  it('detects write-after-read conflict', () => {
    const shared = makePubkey(1);
    const a = makeGraphNode('a', [], [shared]);
    const b = makeGraphNode('b', [shared]);

    expect(a.getDependencyWith(b)).toBe(DependencyType.WriteAfterRead);
  });

  it('returns null for read-read', () => {
    const shared = makePubkey(1);
    const a = makeGraphNode('a', [], [shared]);
    const b = makeGraphNode('b', [], [shared]);

    expect(a.getDependencyWith(b)).toBeNull();
    expect(a.conflictsWith(b)).toBe(false);
  });

  it('clones with new id', () => {
    const original = makeGraphNode('original', [makePubkey(1)]);
    const clone = original.clone('cloned');
    expect(clone.id).toBe('cloned');
    expect(clone.estimatedCu).toBe(original.estimatedCu);
  });

  it('getWritableAccounts and getReadOnlyAccounts', () => {
    const w = makePubkey(1);
    const r = makePubkey(2);
    const node = makeGraphNode('test', [w], [r]);

    expect(node.getWritableAccounts().length).toBe(1);
    expect(node.getReadOnlyAccounts().length).toBe(1);
    expect(node.getWritableAccounts()[0].equals(w)).toBe(true);
    expect(node.getReadOnlyAccounts()[0].equals(r)).toBe(true);
  });

  it('getAccountCount', () => {
    const node = makeGraphNode('test', [makePubkey(1)], [makePubkey(2), makePubkey(3)]);
    expect(node.getAccountCount()).toBe(3);
  });

  it('toPlain and fromPlain round-trip', () => {
    const node = makeGraphNode('test', [makePubkey(1)]);
    const plain = node.toPlain();
    const restored = GraphNode.fromPlain(plain);
    expect(restored.id).toBe('test');
    expect(restored.estimatedCu).toBe(node.estimatedCu);
  });
});

// ---------------------------------------------------------------------------
// BFS and DFS traversal
// ---------------------------------------------------------------------------

describe('Graph Traversal', () => {
  it('BFS visits all reachable nodes in order', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'b', to: 'c', dependencyType: DependencyType.Explicit });

    const visited: string[] = [];
    graph.bfs('a', (node, depth) => {
      visited.push(node.id);
    });
    expect(visited).toEqual(['a', 'b', 'c']);
  });

  it('DFS visits all reachable nodes', () => {
    const graph = new TransactionGraph();
    graph.addNode(makeGraphNode('a'));
    graph.addNode(makeGraphNode('b'));
    graph.addNode(makeGraphNode('c'));
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.Explicit });
    graph.addEdge({ from: 'a', to: 'c', dependencyType: DependencyType.Explicit });

    const visited: string[] = [];
    graph.dfs('a', (node) => {
      visited.push(node.id);
    });
    expect(visited.length).toBe(3);
    expect(visited[0]).toBe('a');
  });
});
