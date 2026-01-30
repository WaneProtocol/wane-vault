import {
  TransactionNode,
  GraphEdge,
  DependencyType,
  ExecutionPlan,
  ExecutionLane,
  PriorityLevel,
  AccountLockManager,
  AnalysisResult,
  AccountConflict,
} from "../types";
import { GraphNode } from "./node";
import {
  TopologicalSortOptions,
  CycleDetectionResult,
  SchedulingOptions,
  DEFAULT_SCHEDULING_OPTIONS,
  NodeVisitor,
  SerializedGraph,
} from "./types";

export { TransactionGraphBuilder } from "./builder";
export { GraphNode } from "./node";
export * from "./types";

/**
 * Core transaction dependency graph.
 *
 * Manages nodes (transactions) and directed edges (dependencies).
 * Provides algorithms for topological sorting, cycle detection,
 * independent group identification, and scheduling into parallel lanes.
 */
export class TransactionGraph {
  private nodes: Map<string, GraphNode> = new Map();
  private edges: GraphEdge[] = [];
  private adjacencyList: Map<string, Set<string>> = new Map();
  private reverseAdjacencyList: Map<string, Set<string>> = new Map();
  private _id: string;

  constructor(id?: string) {
    this._id = id ?? `graph_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
  }

  get id(): string {
    return this._id;
  }

  get nodeCount(): number {
    return this.nodes.size;
  }

  get edgeCount(): number {
    return this.edges.length;
  }

  /**
   * Add a node to the graph.
   */
  addNode(node: TransactionNode | GraphNode): void {
    const graphNode =
      node instanceof GraphNode ? node : GraphNode.fromPlain(node);
    if (this.nodes.has(graphNode.id)) {
      throw new Error(`Node with id '${graphNode.id}' already exists`);
    }
    this.nodes.set(graphNode.id, graphNode);
    if (!this.adjacencyList.has(graphNode.id)) {
      this.adjacencyList.set(graphNode.id, new Set());
    }
    if (!this.reverseAdjacencyList.has(graphNode.id)) {
      this.reverseAdjacencyList.set(graphNode.id, new Set());
    }
  }

  /**
   * Add a directed edge (dependency) to the graph.
   */
  addEdge(edge: GraphEdge): void {
    if (!this.nodes.has(edge.from)) {
      throw new Error(`Source node '${edge.from}' not found`);
    }
    if (!this.nodes.has(edge.to)) {
      throw new Error(`Target node '${edge.to}' not found`);
    }
    // Avoid duplicate edges
    const existing = this.edges.find(
      (e) => e.from === edge.from && e.to === edge.to
    );
    if (existing) return;

    this.edges.push(edge);
    this.adjacencyList.get(edge.from)!.add(edge.to);
    this.reverseAdjacencyList.get(edge.to)!.add(edge.from);
  }

  /**
   * Remove a node and all its connected edges.
   */
  removeNode(nodeId: string): void {
    if (!this.nodes.has(nodeId)) return;
    this.nodes.delete(nodeId);
    this.edges = this.edges.filter(
      (e) => e.from !== nodeId && e.to !== nodeId
    );
    this.adjacencyList.delete(nodeId);
    this.reverseAdjacencyList.delete(nodeId);
    for (const [, neighbors] of this.adjacencyList) {
      neighbors.delete(nodeId);
    }
    for (const [, neighbors] of this.reverseAdjacencyList) {
      neighbors.delete(nodeId);
    }
  }

  /**
   * Get a node by ID.
   */
  getNode(nodeId: string): GraphNode | undefined {
    return this.nodes.get(nodeId);
  }

  /**
   * Get all nodes.
   */
  getNodes(): GraphNode[] {
    return Array.from(this.nodes.values());
  }

  /**
   * Get all edges.
   */
  getEdges(): GraphEdge[] {
    return [...this.edges];
  }

  /**
   * Get the outgoing neighbors of a node.
   */
  getNeighbors(nodeId: string): GraphNode[] {
    const neighborIds = this.adjacencyList.get(nodeId);
    if (!neighborIds) return [];
    return Array.from(neighborIds)
      .map((id) => this.nodes.get(id)!)
      .filter(Boolean);
  }

  /**
   * Get the incoming neighbors (predecessors) of a node.
   */
  getPredecessors(nodeId: string): GraphNode[] {
    const predIds = this.reverseAdjacencyList.get(nodeId);
    if (!predIds) return [];
    return Array.from(predIds)
      .map((id) => this.nodes.get(id)!)
      .filter(Boolean);
  }

  /**
   * Get the in-degree of a node.
   */
  getInDegree(nodeId: string): number {
    return this.reverseAdjacencyList.get(nodeId)?.size ?? 0;
  }

  /**
   * Get the out-degree of a node.
   */
  getOutDegree(nodeId: string): number {
    return this.adjacencyList.get(nodeId)?.size ?? 0;
  }

  /**
   * Perform topological sort using Kahn's algorithm.
   * Returns nodes in dependency-respecting order.
   * Throws if the graph has cycles.
   */
  topologicalSort(options?: TopologicalSortOptions): GraphNode[] {
    const opts = options ?? { priorityAware: false, cuAware: false };
    const inDegree = new Map<string, number>();
    for (const [id] of this.nodes) {
      inDegree.set(id, this.getInDegree(id));
    }

    // Initialize queue with all zero-in-degree nodes
    let queue: string[] = [];
    for (const [id, deg] of inDegree) {
      if (deg === 0) queue.push(id);
    }

    const result: GraphNode[] = [];

    while (queue.length > 0) {
      // Sort queue by priority / CU if requested
      if (opts.priorityAware || opts.cuAware) {
        queue.sort((a, b) => {
          const nodeA = this.nodes.get(a)!;
          const nodeB = this.nodes.get(b)!;
          if (opts.priorityAware && nodeA.priority !== nodeB.priority) {
            return nodeB.priority - nodeA.priority; // Higher priority first
          }
          if (opts.cuAware) {
            return nodeA.estimatedCu - nodeB.estimatedCu; // Lower CU first
          }
          return 0;
        });
      }

      const current = queue.shift()!;
      result.push(this.nodes.get(current)!);

      const neighbors = this.adjacencyList.get(current) ?? new Set();
      for (const neighbor of neighbors) {
        const newDeg = (inDegree.get(neighbor) ?? 1) - 1;
        inDegree.set(neighbor, newDeg);
        if (newDeg === 0) {
          queue.push(neighbor);
        }
      }
    }

    if (result.length !== this.nodes.size) {
      throw new Error(
        "Graph contains a cycle; topological sort is not possible"
      );
    }

    return result;
  }

  /**
   * Detect if the graph contains any cycles using DFS.
   */
  hasCycle(): boolean {
    return this.detectCycle().hasCycle;
  }

  /**
   * Detect cycles and return the nodes involved.
   */
  detectCycle(): CycleDetectionResult {
    const WHITE = 0,
      GRAY = 1,
      BLACK = 2;
    const color = new Map<string, number>();
    const parent = new Map<string, string | null>();
    const cycleNodes: string[] = [];

    for (const [id] of this.nodes) {
      color.set(id, WHITE);
    }

    const dfs = (nodeId: string): boolean => {
      color.set(nodeId, GRAY);
      const neighbors = this.adjacencyList.get(nodeId) ?? new Set();
      for (const neighbor of neighbors) {
        if (color.get(neighbor) === GRAY) {
          // Found cycle, trace back
          cycleNodes.push(neighbor);
          let cur = nodeId;
          while (cur !== neighbor) {
            cycleNodes.push(cur);
            cur = parent.get(cur) ?? neighbor;
          }
          cycleNodes.reverse();
          return true;
        }
        if (color.get(neighbor) === WHITE) {
          parent.set(neighbor, nodeId);
          if (dfs(neighbor)) return true;
        }
      }
      color.set(nodeId, BLACK);
      return false;
    };

    for (const [id] of this.nodes) {
      if (color.get(id) === WHITE) {
        parent.set(id, null);
        if (dfs(id)) {
          return { hasCycle: true, cycleNodes };
        }
      }
    }

    return { hasCycle: false, cycleNodes: [] };
  }

  /**
   * Find groups of independent nodes (no direct or transitive dependencies).
   * Uses connected components on the undirected version of the graph.
   */
  getIndependentGroups(): GraphNode[][] {
    const visited = new Set<string>();
    const groups: GraphNode[][] = [];

    const bfs = (startId: string): GraphNode[] => {
      const group: GraphNode[] = [];
      const queue: string[] = [startId];
      visited.add(startId);

      while (queue.length > 0) {
        const current = queue.shift()!;
        group.push(this.nodes.get(current)!);

        // Traverse both directions (undirected connectivity)
        const forward = this.adjacencyList.get(current) ?? new Set();
        const backward = this.reverseAdjacencyList.get(current) ?? new Set();

        for (const neighbor of forward) {
          if (!visited.has(neighbor)) {
            visited.add(neighbor);
            queue.push(neighbor);
          }
        }
        for (const neighbor of backward) {
          if (!visited.has(neighbor)) {
            visited.add(neighbor);
            queue.push(neighbor);
          }
        }
      }

      return group;
    };

    for (const [id] of this.nodes) {
      if (!visited.has(id)) {
        groups.push(bfs(id));
      }
    }

    return groups;
  }

  /**
   * Automatically detect and add dependency edges based on account access overlaps.
   */
  autoDetectDependencies(): void {
    const nodeList = this.getNodes();
    for (let i = 0; i < nodeList.length; i++) {
      for (let j = i + 1; j < nodeList.length; j++) {
        const a = nodeList[i];
        const b = nodeList[j];
        const depType = a.getDependencyWith(b);
        if (depType !== null) {
          this.addEdge({ from: a.id, to: b.id, dependencyType: depType });
        }
      }
    }
  }

  /**
   * Compute the critical path (longest path by estimated CU).
   */
  getCriticalPath(): { nodes: GraphNode[]; totalCu: number } {
    const sorted = this.topologicalSort();
    const dist = new Map<string, number>();
    const predecessor = new Map<string, string | null>();

    for (const node of sorted) {
      dist.set(node.id, node.estimatedCu);
      predecessor.set(node.id, null);
    }

    for (const node of sorted) {
      const neighbors = this.adjacencyList.get(node.id) ?? new Set();
      for (const neighborId of neighbors) {
        const neighborNode = this.nodes.get(neighborId)!;
        const newDist = dist.get(node.id)! + neighborNode.estimatedCu;
        if (newDist > (dist.get(neighborId) ?? 0)) {
          dist.set(neighborId, newDist);
          predecessor.set(neighborId, node.id);
        }
      }
    }

    // Find the node with the longest distance
    let maxDist = 0;
    let endNodeId = sorted[0]?.id ?? "";
    for (const [id, d] of dist) {
      if (d > maxDist) {
        maxDist = d;
        endNodeId = id;
      }
    }

    // Trace back the path
    const path: GraphNode[] = [];
    let cur: string | null = endNodeId;
    while (cur !== null) {
      path.push(this.nodes.get(cur)!);
      cur = predecessor.get(cur) ?? null;
    }
    path.reverse();

    return { nodes: path, totalCu: maxDist };
  }

  /**
   * Schedule the graph into parallel execution lanes.
   */
  schedule(options?: Partial<SchedulingOptions>): ExecutionPlan {
    const opts: SchedulingOptions = {
      ...DEFAULT_SCHEDULING_OPTIONS,
      ...options,
    };

    if (this.hasCycle()) {
      throw new Error("Cannot schedule a graph with cycles");
    }

    const sorted = this.topologicalSort({ priorityAware: true, cuAware: true });
    const lockManager = new AccountLockManager();
    const lanes: ExecutionLane[] = [];
    const nodeToLane = new Map<string, number>();

    for (const node of sorted) {
      // Wait for all predecessors to be assigned
      const predLanes = new Set<number>();
      const preds = this.reverseAdjacencyList.get(node.id) ?? new Set();
      for (const predId of preds) {
        const lane = nodeToLane.get(predId);
        if (lane !== undefined) predLanes.add(lane);
      }

      // Find a lane that can accommodate this node
      let assigned = false;
      for (let i = 0; i < lanes.length; i++) {
        const lane = lanes[i];
        // Check CU limit
        if (lane.estimatedCu + node.estimatedCu > opts.maxCuPerLane) continue;
        // Check that all predecessors are in earlier positions or same lane
        // (i.e., we don't create a cycle across lanes)
        const canFit = this.canFitInLane(node, lane, nodeToLane);
        if (canFit) {
          lane.nodes.push(node);
          lane.estimatedCu += node.estimatedCu;
          nodeToLane.set(node.id, i);
          assigned = true;
          break;
        }
      }

      if (!assigned) {
        if (lanes.length >= opts.maxLanes) {
          // Find the lane with least CU and force-add
          let minCuLane = 0;
          for (let i = 1; i < lanes.length; i++) {
            if (lanes[i].estimatedCu < lanes[minCuLane].estimatedCu) {
              minCuLane = i;
            }
          }
          lanes[minCuLane].nodes.push(node);
          lanes[minCuLane].estimatedCu += node.estimatedCu;
          nodeToLane.set(node.id, minCuLane);
        } else {
          const newLane: ExecutionLane = {
            laneIndex: lanes.length,
            nodes: [node],
            estimatedCu: node.estimatedCu,
          };
          nodeToLane.set(node.id, lanes.length);
          lanes.push(newLane);
        }
      }
    }

    // Balance loads if requested
    if (opts.balanceLoad && lanes.length > 1) {
      this.balanceLanes(lanes, opts.maxCuPerLane, nodeToLane);
    }

    const totalCu = lanes.reduce((sum, l) => sum + l.estimatedCu, 0);

    return {
      lanes,
      totalEstimatedCu: totalCu,
      parallelismDegree: lanes.length,
      createdAt: Date.now(),
      graphId: this._id,
    };
  }

  /**
   * Check if a node can be placed into a given lane without violating ordering.
   */
  private canFitInLane(
    node: GraphNode,
    lane: ExecutionLane,
    nodeToLane: Map<string, number>
  ): boolean {
    // A node can fit in a lane if none of its successors are already in this lane
    // ahead of where the node would be inserted
    const successors = this.adjacencyList.get(node.id) ?? new Set();
    for (const succId of successors) {
      const succLane = nodeToLane.get(succId);
      if (succLane === lane.laneIndex) {
        return false; // successor already in this lane
      }
    }

    // Also check: the node doesn't conflict with the last node in the lane
    // (account-level conflict check)
    if (lane.nodes.length > 0) {
      const lastNode = lane.nodes[lane.nodes.length - 1];
      if (lastNode instanceof GraphNode && node instanceof GraphNode) {
        // Conflicts are fine within a lane since they execute sequentially
        // Only cross-lane conflicts matter
      }
    }

    return true;
  }

  /**
   * Attempt to balance CU across lanes by moving nodes from heavy to light lanes.
   */
  private balanceLanes(
    lanes: ExecutionLane[],
    maxCuPerLane: number,
    nodeToLane: Map<string, number>
  ): void {
    const avgCu =
      lanes.reduce((sum, l) => sum + l.estimatedCu, 0) / lanes.length;
    const threshold = avgCu * 0.2; // 20% tolerance

    for (let iteration = 0; iteration < 10; iteration++) {
      let moved = false;

      // Sort lanes by CU descending
      const sortedIndices = lanes
        .map((_, i) => i)
        .sort((a, b) => lanes[b].estimatedCu - lanes[a].estimatedCu);

      const heaviest = sortedIndices[0];
      const lightest = sortedIndices[sortedIndices.length - 1];

      if (lanes[heaviest].estimatedCu - lanes[lightest].estimatedCu < threshold) {
        break;
      }

      // Try to move the last node from heaviest to lightest
      const heavyLane = lanes[heaviest];
      if (heavyLane.nodes.length <= 1) break;

      const candidate = heavyLane.nodes[heavyLane.nodes.length - 1];

      // Check if moving this node would violate dependencies
      const preds = this.reverseAdjacencyList.get(candidate.id) ?? new Set();
      let canMove = true;
      for (const predId of preds) {
        if (nodeToLane.get(predId) === lightest) {
          // Predecessor is in the target lane -- need to check ordering
          const predIndex = lanes[lightest].nodes.findIndex(
            (n) => n.id === predId
          );
          if (predIndex === -1) canMove = false;
        }
      }

      if (
        canMove &&
        lanes[lightest].estimatedCu + candidate.estimatedCu <= maxCuPerLane
      ) {
        heavyLane.nodes.pop();
        heavyLane.estimatedCu -= candidate.estimatedCu;
        lanes[lightest].nodes.push(candidate);
        lanes[lightest].estimatedCu += candidate.estimatedCu;
        nodeToLane.set(candidate.id, lightest);
        moved = true;
      }

      if (!moved) break;
    }
  }

  /**
   * Breadth-first traversal of the graph.
   */
  bfs(startId: string, visitor: NodeVisitor): void {
    const visited = new Set<string>();
    const queue: Array<{ id: string; depth: number }> = [
      { id: startId, depth: 0 },
    ];
    visited.add(startId);

    while (queue.length > 0) {
      const { id, depth } = queue.shift()!;
      const node = this.nodes.get(id);
      if (!node) continue;
      visitor(node, depth);

      const neighbors = this.adjacencyList.get(id) ?? new Set();
      for (const neighbor of neighbors) {
        if (!visited.has(neighbor)) {
          visited.add(neighbor);
          queue.push({ id: neighbor, depth: depth + 1 });
        }
      }
    }
  }

  /**
   * Depth-first traversal of the graph.
   */
  dfs(startId: string, visitor: NodeVisitor): void {
    const visited = new Set<string>();

    const recurse = (id: string, depth: number) => {
      if (visited.has(id)) return;
      visited.add(id);
      const node = this.nodes.get(id);
      if (!node) return;
      visitor(node, depth);
      const neighbors = this.adjacencyList.get(id) ?? new Set();
      for (const neighbor of neighbors) {
        recurse(neighbor, depth + 1);
      }
    };

    recurse(startId, 0);
  }

  /**
   * Analyze the graph and return statistics.
   */
  analyze(): AnalysisResult {
    const nodes = this.getNodes();
    const criticalPath = this.nodeCount > 0 && !this.hasCycle()
      ? this.getCriticalPath()
      : { nodes: [], totalCu: 0 };
    const groups = this.getIndependentGroups();
    const totalCu = nodes.reduce((sum, n) => sum + n.estimatedCu, 0);

    // Detect account conflicts
    const conflicts: AccountConflict[] = [];
    for (const edge of this.edges) {
      if (edge.dependencyType !== DependencyType.Explicit) {
        const nodeA = this.nodes.get(edge.from);
        const nodeB = this.nodes.get(edge.to);
        if (nodeA && nodeB) {
          // Find the conflicting account
          const aWritable = new Set(
            nodeA.getWritableAccounts().map((p) => p.toBase58())
          );
          const bWritable = new Set(
            nodeB.getWritableAccounts().map((p) => p.toBase58())
          );
          const bReadOnly = new Set(
            nodeB.getReadOnlyAccounts().map((p) => p.toBase58())
          );

          for (const acc of aWritable) {
            if (bWritable.has(acc) || bReadOnly.has(acc)) {
              const { PublicKey } = require("@solana/web3.js");
              conflicts.push({
                account: new PublicKey(acc),
                nodeA: edge.from,
                nodeB: edge.to,
                conflictType: edge.dependencyType,
              });
              break;
            }
          }
        }
      }
    }

    const maxParallelism = groups.length > 0
      ? Math.max(...groups.map((g) => g.length))
      : 0;

    return {
      nodeCount: this.nodeCount,
      edgeCount: this.edgeCount,
      maxParallelism,
      criticalPathLength: criticalPath.nodes.length,
      criticalPathCu: criticalPath.totalCu,
      totalCu,
      averageCuPerLane: groups.length > 0 ? totalCu / groups.length : 0,
      hasCycles: this.hasCycle(),
      independentGroupCount: groups.length,
      accountConflicts: conflicts,
    };
  }

  /**
   * Serialize the graph to a plain JSON-compatible object.
   */
  serialize(): SerializedGraph {
    return {
      nodes: this.getNodes().map((n) => ({
        id: n.id,
        programId: n.programId.toBase58(),
        instructionCount: n.instructions.length,
        accountAccesses: n.accountAccesses.map((a) => ({
          pubkey: a.pubkey.toBase58(),
          isWritable: a.isWritable,
          isSigner: a.isSigner,
        })),
        estimatedCu: n.estimatedCu,
        priority: n.priority,
        label: n.label,
        metadata: n.metadata,
      })),
      edges: this.edges.map((e) => ({
        from: e.from,
        to: e.to,
        dependencyType: e.dependencyType,
      })),
      metadata: { graphId: this._id },
      version: 1,
    };
  }

  /**
   * Deserialize from a SerializedGraph.
   */
  static deserialize(data: SerializedGraph): TransactionGraph {
    const { PublicKey } = require("@solana/web3.js");
    const graphId =
      (data.metadata?.graphId as string) ??
      `graph_${Date.now()}`;
    const graph = new TransactionGraph(graphId);

    for (const sn of data.nodes) {
      const node = new GraphNode({
        id: sn.id,
        programId: new PublicKey(sn.programId),
        accountAccesses: sn.accountAccesses.map(
          (a: { pubkey: string; isWritable: boolean; isSigner: boolean }) => ({
            pubkey: new PublicKey(a.pubkey),
            isWritable: a.isWritable,
            isSigner: a.isSigner,
          })
        ),
        estimatedCu: sn.estimatedCu,
        priority: sn.priority as PriorityLevel,
        label: sn.label,
        metadata: sn.metadata,
      });
      graph.addNode(node);
    }

    for (const se of data.edges) {
      graph.addEdge({
        from: se.from,
        to: se.to,
        dependencyType: se.dependencyType as DependencyType,
      });
    }

    return graph;
  }
}
