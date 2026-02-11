# API Reference

This document covers the public API surface for both the Rust core library and the TypeScript SDK.

---

## Rust Core Library

### ivza_core::graph

#### TransactionGraph

The central data structure representing a directed acyclic graph of transactions.

```rust
pub struct TransactionGraph {
    pub id: GraphId,
    nodes: Vec<TxNode>,
    edges: Vec<Edge>,
    pub metadata: GraphMetadata,
}

impl TransactionGraph {
    /// Create an empty graph.
    pub fn new() -> Self;

    /// Add a node to the graph. Returns the assigned NodeId.
    pub fn add_node(&mut self, node: TxNode) -> NodeId;

    /// Add a directed edge between two nodes.
    pub fn add_edge(&mut self, edge: Edge);

    /// Get a node by ID.
    pub fn get_node(&self, id: NodeId) -> Option<&TxNode>;

    /// Get all nodes.
    pub fn nodes(&self) -> &[TxNode];

    /// Get all edges.
    pub fn edges(&self) -> &[Edge];

    /// Number of nodes in the graph.
    pub fn node_count(&self) -> usize;

    /// Number of edges in the graph.
    pub fn edge_count(&self) -> usize;

    /// Return nodes that have no incoming edges.
    pub fn root_nodes(&self) -> Vec<NodeId>;

    /// Return nodes that have no outgoing edges.
    pub fn leaf_nodes(&self) -> Vec<NodeId>;
}
```

#### TxNode

A single transaction node within the graph.

```rust
pub struct TxNode {
    pub id: NodeId,
    pub instruction: Instruction,
    pub read_set: Vec<Pubkey>,
    pub write_set: Vec<Pubkey>,
    pub priority: u8,
}

impl TxNode {
    /// Create a new node with the given instruction and account sets.
    pub fn new(
        instruction: Instruction,
        read_set: Vec<Pubkey>,
        write_set: Vec<Pubkey>,
    ) -> Self;

    /// Set the priority (0 = lowest, 255 = highest).
    pub fn with_priority(self, priority: u8) -> Self;
}
```

#### Edge

A directed edge encoding a dependency between two nodes.

```rust
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

pub enum EdgeKind {
    /// Node `to` reads data produced by node `from`.
    DataDependency,
    /// Node `to` must execute after node `from` for correctness.
    OrderingConstraint,
    /// Nodes share a writable account lock.
    AccountLock,
}

impl Edge {
    pub fn new(from: NodeId, to: NodeId, kind: EdgeKind) -> Self;
}
```

---

### ivza_core::analyzer

#### DependencyAnalyzer

Analyzes account-level conflicts between nodes in a transaction graph.

```rust
pub struct DependencyAnalyzer;

impl DependencyAnalyzer {
    pub fn new() -> Self;

    /// Analyze the graph and produce a DependencyAnalysis.
    pub fn analyze(&self, graph: &TransactionGraph) -> Result<DependencyAnalysis>;
}
```

#### DependencyAnalysis

The output of dependency analysis, containing the conflict matrix and merged edge set.

```rust
pub struct DependencyAnalysis {
    conflict_matrix: Vec<Vec<bool>>,
    merged_edges: Vec<Edge>,
}

impl DependencyAnalysis {
    /// Check if two nodes are independent (no conflicts, no edges).
    pub fn are_independent(&self, a: NodeId, b: NodeId) -> bool;

    /// Check if two nodes conflict at the account level.
    pub fn has_conflict(&self, a: NodeId, b: NodeId) -> bool;

    /// Get all edges (explicit + conflict-derived).
    pub fn merged_edges(&self) -> &[Edge];

    /// Get the set of nodes that a given node depends on.
    pub fn dependencies_of(&self, node: NodeId) -> Vec<NodeId>;

    /// Get the set of nodes that depend on a given node.
    pub fn dependents_of(&self, node: NodeId) -> Vec<NodeId>;
}
```

---

### ivza_core::scheduler

#### ParallelScheduler

Assigns nodes to levels and lanes for parallel execution.

```rust
pub struct ParallelScheduler {
    max_parallelism: usize,
}

impl ParallelScheduler {
    /// Create a scheduler with the given max lane count.
    pub fn new(max_parallelism: usize) -> Self;

    /// Produce an execution schedule from a graph and its analysis.
    pub fn schedule(
        &self,
        graph: &TransactionGraph,
        analysis: &DependencyAnalysis,
    ) -> Result<ExecutionSchedule>;
}
```

#### ExecutionSchedule

The output of scheduling: a set of levels, each containing lanes of ordered nodes.

```rust
pub struct ExecutionSchedule {
    levels: Vec<Level>,
}

pub struct Level {
    pub index: usize,
    lanes: Vec<Lane>,
}

pub struct Lane {
    pub id: usize,
    pub nodes: Vec<NodeId>,
}

impl ExecutionSchedule {
    /// Number of levels in the schedule.
    pub fn level_count(&self) -> usize;

    /// Get a specific level by index.
    pub fn level(&self, index: usize) -> &Level;

    /// Iterate over all levels.
    pub fn levels(&self) -> &[Level];

    /// Total number of lanes across all levels.
    pub fn total_lanes(&self) -> usize;
}

impl Level {
    /// Number of lanes at this level.
    pub fn len(&self) -> usize;

    /// Get all lanes at this level.
    pub fn lanes(&self) -> &[Lane];
}
```

---

### ivza_core::engine

#### ExecutionEngine

Dispatches scheduled transactions to Solana.

```rust
pub struct ExecutionEngine {
    config: EngineConfig,
    rpc_client: RpcClient,
}

impl ExecutionEngine {
    /// Create a new engine with the given configuration.
    pub async fn new(config: EngineConfig) -> Result<Self>;

    /// Execute a schedule and return results for each node.
    pub async fn execute(
        &self,
        schedule: ExecutionSchedule,
    ) -> Result<Vec<ExecutionResult>>;
}
```

#### EngineConfig

```rust
pub struct EngineConfig {
    pub max_parallelism: usize,
    pub retry_attempts: u32,
    pub confirmation_commitment: String,
    pub rpc_url: String,
    pub tpu_enabled: bool,
    pub compute_unit_price: u64,
    pub timeout_ms: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_parallelism: 8,
            retry_attempts: 3,
            confirmation_commitment: "confirmed".to_string(),
            rpc_url: String::new(),
            tpu_enabled: false,
            compute_unit_price: 0,
            timeout_ms: 30_000,
        }
    }
}
```

#### ExecutionResult

```rust
pub struct ExecutionResult {
    pub node_id: NodeId,
    pub signature: Signature,
    pub status: ExecutionStatus,
    pub slot: u64,
    pub elapsed_ms: u64,
}

pub enum ExecutionStatus {
    Confirmed,
    Failed { error: String },
    Cancelled,
    Skipped,
}
```

---

### ivza_core::intent

#### Intent

High-level operation descriptions.

```rust
pub enum Intent {
    Swap {
        input_mint: Pubkey,
        output_mint: Pubkey,
        amount: u64,
        slippage_bps: u16,
    },
    BatchTransfer {
        transfers: Vec<Transfer>,
    },
    CompositeOperation {
        steps: Vec<Intent>,
        atomic: bool,
    },
    ProgramInvocation {
        program_id: Pubkey,
        accounts: Vec<AccountMeta>,
        data: Vec<u8>,
    },
}

pub struct Transfer {
    pub destination: Pubkey,
    pub amount: u64,
    pub mint: Option<Pubkey>,
}
```

#### IntentProcessor

Decomposes intents into transaction graphs and executes them.

```rust
pub struct IntentProcessor {
    config: EngineConfig,
}

impl IntentProcessor {
    pub fn new(config: EngineConfig) -> Self;

    /// Decompose an intent into a transaction graph.
    pub fn decompose(&self, intent: Intent) -> Result<TransactionGraph>;

    /// Decompose, analyze, schedule, and execute an intent.
    pub async fn execute(
        &self,
        intent: Intent,
    ) -> Result<Vec<ExecutionResult>>;
}
```

---

## TypeScript SDK

### IvzaClient

The main client class for the TypeScript SDK.

```typescript
interface IvzaClientConfig {
  connection: Connection;
  wallet: Keypair;
  maxParallelism?: number;      // default: 8
  retryAttempts?: number;       // default: 3
  commitment?: Commitment;      // default: "confirmed"
  tpuEnabled?: boolean;         // default: false
  computeUnitPrice?: bigint;    // default: 0n
  timeoutMs?: number;           // default: 30000
}

class IvzaClient {
  constructor(config: IvzaClientConfig);

  /** Execute an intent: decompose, analyze, schedule, and dispatch. */
  execute(intent: Intent): Promise<ExecutionResult[]>;

  /** Decompose an intent into a transaction graph without executing. */
  decompose(intent: Intent): TransactionGraph;

  /** Analyze a graph and return the dependency analysis. */
  analyze(graph: TransactionGraph): DependencyAnalysis;

  /** Schedule an analyzed graph into parallel lanes. */
  schedule(
    graph: TransactionGraph,
    analysis: DependencyAnalysis,
  ): ExecutionSchedule;
}
```

### Intent Types

```typescript
type Intent =
  | SwapIntent
  | BatchTransferIntent
  | CompositeIntent
  | ProgramInvocationIntent;

interface SwapIntent {
  type: "swap";
  inputMint: PublicKey;
  outputMint: PublicKey;
  amount: bigint;
  slippageBps: number;
}

interface BatchTransferIntent {
  type: "batch_transfer";
  transfers: TransferEntry[];
  mint: PublicKey;
}

interface TransferEntry {
  destination: PublicKey;
  amount: bigint;
}

interface CompositeIntent {
  type: "composite";
  steps: Intent[];
  atomic: boolean;
}

interface ProgramInvocationIntent {
  type: "program_invocation";
  programId: PublicKey;
  accounts: AccountMeta[];
  data: Buffer;
}
```

### Result Types

```typescript
interface ExecutionResult {
  nodeId: string;
  signature: string;
  status: "confirmed" | "failed" | "cancelled" | "skipped";
  error?: string;
  slot: number;
  elapsedMs: number;
}
```

### Graph Types

```typescript
interface TransactionGraph {
  id: string;
  nodes: TxNode[];
  edges: GraphEdge[];
}

interface TxNode {
  id: string;
  instruction: TransactionInstruction;
  readSet: PublicKey[];
  writeSet: PublicKey[];
  priority: number;
}

interface GraphEdge {
  from: string;
  to: string;
  kind: "data_dependency" | "ordering_constraint" | "account_lock";
}
```

### Analysis Types

```typescript
interface DependencyAnalysis {
  conflictMatrix: boolean[][];
  mergedEdges: GraphEdge[];
  areIndependent(a: string, b: string): boolean;
  dependenciesOf(nodeId: string): string[];
  dependentsOf(nodeId: string): string[];
}

interface ExecutionSchedule {
  levels: ScheduleLevel[];
  levelCount(): number;
  totalLanes(): number;
}

interface ScheduleLevel {
  index: number;
  lanes: ScheduleLane[];
}

interface ScheduleLane {
  id: number;
  nodeIds: string[];
}
```
