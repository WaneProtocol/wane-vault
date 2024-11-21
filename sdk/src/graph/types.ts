import { TransactionNode, GraphEdge } from "../types";

/**
 * Serializable representation of a transaction graph.
 */
export interface SerializedGraph {
  nodes: SerializedNode[];
  edges: SerializedEdge[];
  metadata: Record<string, unknown>;
  version: number;
}

/**
 * Serialized form of a TransactionNode with string pubkeys.
 */
export interface SerializedNode {
  id: string;
  programId: string;
  instructionCount: number;
  accountAccesses: Array<{
    pubkey: string;
    isWritable: boolean;
    isSigner: boolean;
  }>;
  estimatedCu: number;
  priority: number;
  label?: string;
  metadata?: Record<string, unknown>;
}

/**
 * Serialized form of a GraphEdge.
 */
export interface SerializedEdge {
  from: string;
  to: string;
  dependencyType: string;
}

/**
 * Options for topological sorting.
 */
export interface TopologicalSortOptions {
  /** If true, break ties by priority (higher first) */
  priorityAware: boolean;
  /** If true, break ties by estimated CU (lower first) */
  cuAware: boolean;
}

/**
 * Result of cycle detection.
 */
export interface CycleDetectionResult {
  hasCycle: boolean;
  /** Nodes involved in the cycle, if found */
  cycleNodes: string[];
}

/**
 * Options for graph scheduling / lane assignment.
 */
export interface SchedulingOptions {
  maxLanes: number;
  maxCuPerLane: number;
  balanceLoad: boolean;
}

/**
 * Default scheduling options.
 */
export const DEFAULT_SCHEDULING_OPTIONS: SchedulingOptions = {
  maxLanes: 4,
  maxCuPerLane: 1_400_000,
  balanceLoad: true,
};

/**
 * Graph traversal visitor callback types.
 */
export type NodeVisitor = (node: TransactionNode, depth: number) => void;
export type EdgeVisitor = (edge: GraphEdge) => void;
