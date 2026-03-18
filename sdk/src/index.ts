// === IVZA Parallel Execution Engine SDK ===
// Main entry point: re-exports all public API surface.

// Client
export { IvzaClient } from "./client";
export type {
  GraphSettledCallback,
  SolveOptions,
  ProcessOptions,
} from "./client";

// Types
export {
  DependencyType,
  PriorityLevel,
  GraphStatus,
  IntentType,
  DEFAULT_CONFIG,
  KNOWN_MINTS,
  AccountLockState,
  AccountLockManager,
} from "./types";

export type {
  TransactionNode,
  GraphEdge,
  ExecutionLane,
  ExecutionPlan,
  TransactionResult,
  ExecutionResult,
  LaneResult,
  GraphState,
  AnalysisResult,
  AccountConflict,
  AccountAccess,
  AccountLockEntry,
  IvzaConfig,
  WalletAdapter,
  Intent,
  IntentParams,
  SwapParams,
  MultiHopSwapParams,
  StakeParams,
  UnstakeParams,
  ProvideLiquidityParams,
  TransferParams,
  Route,
  RouteHop,
  SolverResult,
} from "./types";

// Graph
export {
  TransactionGraph,
  TransactionGraphBuilder,
  GraphNode,
} from "./graph";

export type {
  SerializedGraph,
  SerializedNode,
  SerializedEdge,
  TopologicalSortOptions,
  CycleDetectionResult,
  SchedulingOptions,
  NodeVisitor,
  EdgeVisitor,
} from "./graph";

// Intent
export { IntentParser, IntentValidator } from "./intent";

export type {
  DslTokens,
  IntentValidationError,
  IntentValidationResult,
  JsonIntentInput,
} from "./intent";

// Executor
export {
  ParallelExecutor,
  BundleBuilder,
} from "./executor";

export type {
  ExecutorEvent,
  ExecutorEventListener,
  ParallelExecutionOptions,
  BundleConfig,
  SerializedBundle,
  BundleSubmissionResult,
  BundleStatusResult,
} from "./executor";

// Utils
export { ConnectionManager } from "./utils";

export type {
  EndpointHealth,
  ConnectionManagerConfig,
  SerializedIntent,
} from "./utils";

export {
  serializeGraph,
  deserializeGraph,
  serializeGraphToString,
  deserializeGraphFromString,
  serializeIntent,
  deserializeIntent,
  serializeIntents,
  deserializeIntents,
  encodeBase64,
  decodeBase64,
  encodeBase58,
  decodeBase58,
  derivePDA,
  deriveATA,
  resolveTokenMint,
  getTokenSymbol,
  hashBytes,
  fingerprintGraph,
} from "./utils";
