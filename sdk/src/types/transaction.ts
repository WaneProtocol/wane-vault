import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { AccountAccess } from "./account";

/**
 * Dependency type between two transaction nodes.
 */
export enum DependencyType {
  /** Node B reads an account that node A writes */
  ReadAfterWrite = "read_after_write",
  /** Node B writes an account that node A writes */
  WriteAfterWrite = "write_after_write",
  /** Node B writes an account that node A reads */
  WriteAfterRead = "write_after_read",
  /** Explicit ordering constraint set by user */
  Explicit = "explicit",
}

/**
 * Priority level for transaction execution.
 */
export enum PriorityLevel {
  Low = 0,
  Medium = 1,
  High = 2,
  Critical = 3,
}

/**
 * A single node in the transaction dependency graph.
 */
export interface TransactionNode {
  id: string;
  programId: PublicKey;
  instructions: TransactionInstruction[];
  accountAccesses: AccountAccess[];
  estimatedCu: number;
  priority: PriorityLevel;
  label?: string;
  metadata?: Record<string, unknown>;
}

/**
 * A directed edge in the transaction dependency graph.
 */
export interface GraphEdge {
  from: string;
  to: string;
  dependencyType: DependencyType;
}

/**
 * A lane of sequentially-executed transactions within a parallel plan.
 */
export interface ExecutionLane {
  laneIndex: number;
  nodes: TransactionNode[];
  estimatedCu: number;
}

/**
 * A complete execution plan describing how to parallelize a transaction graph.
 */
export interface ExecutionPlan {
  lanes: ExecutionLane[];
  totalEstimatedCu: number;
  parallelismDegree: number;
  createdAt: number;
  graphId: string;
}

/**
 * Result of executing a single transaction.
 */
export interface TransactionResult {
  nodeId: string;
  signature: string | null;
  success: boolean;
  error?: string;
  slot?: number;
  computeUnitsUsed?: number;
  confirmationTime?: number;
}

/**
 * Result of executing an entire plan.
 */
export interface ExecutionResult {
  planId: string;
  laneResults: LaneResult[];
  totalTime: number;
  success: boolean;
  failedNodes: string[];
}

/**
 * Result of executing a single lane.
 */
export interface LaneResult {
  laneIndex: number;
  transactionResults: TransactionResult[];
  totalTime: number;
  success: boolean;
}

/**
 * Status of a graph submitted on-chain.
 */
export enum GraphStatus {
  Pending = "pending",
  Executing = "executing",
  Settled = "settled",
  Failed = "failed",
  PartiallySettled = "partially_settled",
  Cancelled = "cancelled",
}

/**
 * On-chain graph state returned by getGraphStatus.
 */
export interface GraphState {
  graphId: string;
  status: GraphStatus;
  lanesCompleted: number;
  lanesTotal: number;
  failedNodes: string[];
  settledAt?: number;
  slot?: number;
}

/**
 * Analysis result for a transaction graph.
 */
export interface AnalysisResult {
  nodeCount: number;
  edgeCount: number;
  maxParallelism: number;
  criticalPathLength: number;
  criticalPathCu: number;
  totalCu: number;
  averageCuPerLane: number;
  hasCycles: boolean;
  independentGroupCount: number;
  accountConflicts: AccountConflict[];
}

/**
 * A detected conflict between two nodes on a shared account.
 */
export interface AccountConflict {
  account: PublicKey;
  nodeA: string;
  nodeB: string;
  conflictType: DependencyType;
}
