import { Connection, PublicKey } from "@solana/web3.js";
import {
  IvzaConfig,
  DEFAULT_CONFIG,
  WalletAdapter,
  Intent,
  IntentType,
  SwapParams,
  MultiHopSwapParams,
  ExecutionPlan,
  ExecutionResult,
  LaneResult,
  TransactionResult,
  GraphStatus,
  GraphState,
  AnalysisResult,
  TransactionNode,
  PriorityLevel,
  DependencyType,
  KNOWN_MINTS,
} from "./types";
import {
  TransactionGraph,
  TransactionGraphBuilder,
  GraphNode,
} from "./graph";
import { IntentParser, IntentValidator } from "./intent";
import {
  ParallelExecutor,
  ExecutorEvent,
  ExecutorEventListener,
  ParallelExecutionOptions,
  BundleBuilder,
  BundleSubmissionResult,
  SerializedBundle,
} from "./executor";
import { ConnectionManager, ConnectionManagerConfig } from "./utils/connection";
import {
  serializeGraph,
  deserializeGraph,
  serializeIntent,
  fingerprintGraph,
} from "./utils/serialization";
import { SerializedGraph } from "./graph/types";

/**
 * Callback for graph settlement events.
 */
export type GraphSettledCallback = (
  graphId: string,
  status: GraphStatus,
  result: ExecutionResult | null
) => void;

/**
 * Options for the solve() method.
 */
export interface SolveOptions {
  /** Maximum number of routes to consider */
  maxRoutes?: number;
  /** Whether to auto-detect dependencies in the graph */
  autoDetectDeps?: boolean;
  /** Priority level for all nodes */
  priority?: PriorityLevel;
  /** Custom compute unit estimate per node */
  computeUnits?: number;
}

/**
 * Options for the processIntent() pipeline.
 */
export interface ProcessOptions extends SolveOptions {
  /** Whether to use Jito bundles */
  useJito?: boolean;
  /** Whether to simulate before executing */
  simulate?: boolean;
  /** Execution options override */
  executionOptions?: Partial<ParallelExecutionOptions>;
}

/**
 * Status of a tracked graph execution.
 */
interface TrackedGraph {
  graphId: string;
  plan: ExecutionPlan;
  status: GraphStatus;
  result: ExecutionResult | null;
  createdAt: number;
  settledAt?: number;
}

/**
 * Main entry point for the IVZA Execution Layer SDK.
 *
 * Provides a unified interface for:
 *   - Parsing user intents (DSL or JSON)
 *   - Building transaction dependency graphs
 *   - Analyzing graphs for parallelism opportunities
 *   - Executing graphs in parallel lanes
 *   - Submitting Jito bundles
 *   - Tracking execution status
 *
 * Usage:
 *   const client = new IvzaClient(connection, wallet, { rpcEndpoints: [...] });
 *   const result = await client.processIntent("swap 100 USDC to SOL");
 */
export class IvzaClient {
  private connectionManager: ConnectionManager;
  private wallet: WalletAdapter;
  private config: Required<IvzaConfig>;
  private parser: IntentParser;
  private validator: IntentValidator;
  private executor: ParallelExecutor;
  private bundleBuilder: BundleBuilder;
  private trackedGraphs: Map<string, TrackedGraph> = new Map();
  private settledCallbacks: Map<string, GraphSettledCallback[]> = new Map();
  private globalListeners: GraphSettledCallback[] = [];

  constructor(
    connection: Connection,
    wallet: WalletAdapter,
    config?: Partial<IvzaConfig>
  ) {
    this.config = { ...DEFAULT_CONFIG, ...config } as Required<IvzaConfig>;
    this.wallet = wallet;
    this.parser = new IntentParser();
    this.validator = new IntentValidator();

    // Set up connection management
    this.connectionManager = new ConnectionManager({
      endpoints: this.config.rpcEndpoints,
      commitment: "confirmed",
      autoFailover: true,
      autoHealthCheck: this.config.rpcEndpoints.length > 1,
    });

    const activeConnection = this.connectionManager.getConnection();

    // Set up executor
    this.executor = new ParallelExecutor(activeConnection, wallet, {
      maxRetries: this.config.maxRetries,
      baseRetryDelayMs: this.config.retryDelayMs,
      commitment: "confirmed",
      confirmTimeoutMs: this.config.confirmationTimeout,
      defaultPriorityFee: this.config.defaultPriority * 1_000,
    });

    // Set up bundle builder
    this.bundleBuilder = new BundleBuilder(activeConnection, wallet, {
      jitoEndpoint: this.config.jitoEndpoint,
      tipLamports: this.config.jitoTipLamports,
    });
  }

  /**
   * Parse an intent from a DSL string or JSON object.
   */
  parseIntent(input: string | object): Intent {
    const intent = this.parser.parse(input);
    const validation = this.validator.validate(intent);

    if (!validation.valid) {
      const errorMessages = validation.errors
        .map((e) => `${e.field}: ${e.message}`)
        .join("; ");
      throw new Error(`Invalid intent: ${errorMessages}`);
    }

    return intent;
  }

  /**
   * Build a transaction graph from an intent.
   * This creates placeholder nodes; real instructions would come from
   * on-chain routing/solving.
   */
  buildGraph(intent: Intent): TransactionGraph {
    const builder = new TransactionGraphBuilder();

    switch (intent.type) {
      case IntentType.Swap: {
        const params = intent.params as SwapParams;
        builder
          .swap()
          .from(params.inputMint)
          .to(params.outputMint)
          .amount(params.amount)
          .slippageBps(params.slippageBps)
          .build();
        break;
      }

      case IntentType.MultiHopSwap: {
        const params = intent.params as MultiHopSwapParams;
        let b = builder;
        for (const hop of params.hops) {
          b = builder
            .swap()
            .from(hop.inputMint)
            .to(hop.outputMint)
            .amount(params.amount)
            .slippageBps(params.slippageBps)
            .then();
        }
        b.build();
        break;
      }

      case IntentType.Stake: {
        const params = intent.params as import("./types").StakeParams;
        builder.stake().amount(params.amount).build();
        break;
      }

      case IntentType.Unstake: {
        const params = intent.params as import("./types").UnstakeParams;
        builder.unstake().amount(params.amount).build();
        break;
      }

      case IntentType.Transfer: {
        const params = intent.params as import("./types").TransferParams;
        builder
          .transfer()
          .token(params.mint)
          .amount(params.amount)
          .to(params.recipient)
          .build();
        break;
      }

      case IntentType.ProvideLiquidity: {
        const params =
          intent.params as import("./types").ProvideLiquidityParams;
        builder
          .provideLiquidity()
          .tokenA(params.tokenAMint)
          .tokenB(params.tokenBMint)
          .amounts(params.amountA, params.amountB)
          .build();
        break;
      }

      default:
        throw new Error(`Unsupported intent type: ${intent.type}`);
    }

    const graph = builder.build();
    graph.autoDetectDependencies();
    return graph;
  }

  /**
   * Analyze a transaction graph and return statistics.
   */
  analyzeGraph(graph: TransactionGraph): AnalysisResult {
    return graph.analyze();
  }

  /**
   * Create an execution plan from a graph.
   */
  planGraph(
    graph: TransactionGraph,
    options?: { maxLanes?: number; maxCuPerLane?: number; balanceLoad?: boolean }
  ): ExecutionPlan {
    return graph.schedule({
      maxLanes: options?.maxLanes ?? this.config.maxParallelLanes,
      maxCuPerLane: options?.maxCuPerLane,
      balanceLoad: options?.balanceLoad,
    });
  }

  /**
   * Submit a graph for execution on-chain.
   * Tracks the graph and notifies listeners on settlement.
   */
  async submitGraph(graph: TransactionGraph): Promise<string> {
    const plan = this.planGraph(graph);
    const graphId = graph.id;

    this.trackedGraphs.set(graphId, {
      graphId,
      plan,
      status: GraphStatus.Pending,
      result: null,
      createdAt: Date.now(),
    });

    // Execute asynchronously
    this.executeTrackedGraph(graphId).catch((err) => {
      const tracked = this.trackedGraphs.get(graphId);
      if (tracked) {
        tracked.status = GraphStatus.Failed;
        tracked.settledAt = Date.now();
        this.notifySettled(graphId, GraphStatus.Failed, null);
      }
    });

    return graphId;
  }

  /**
   * Execute a tracked graph and update its status.
   */
  private async executeTrackedGraph(graphId: string): Promise<void> {
    const tracked = this.trackedGraphs.get(graphId);
    if (!tracked) throw new Error(`Graph ${graphId} not found`);

    tracked.status = GraphStatus.Executing;

    try {
      const result = await this.executor.execute(tracked.plan);
      tracked.result = result;
      tracked.settledAt = Date.now();

      if (result.success) {
        tracked.status = GraphStatus.Settled;
      } else if (result.failedNodes.length < tracked.plan.lanes.flatMap((l) => l.nodes).length) {
        tracked.status = GraphStatus.PartiallySettled;
      } else {
        tracked.status = GraphStatus.Failed;
      }

      this.notifySettled(graphId, tracked.status, result);
    } catch (err) {
      tracked.status = GraphStatus.Failed;
      tracked.settledAt = Date.now();
      this.notifySettled(graphId, GraphStatus.Failed, null);
      throw err;
    }
  }

  /**
   * Get the status of a previously submitted graph.
   */
  getGraphStatus(graphId: string): GraphState {
    const tracked = this.trackedGraphs.get(graphId);
    if (!tracked) {
      throw new Error(`Graph ${graphId} not found`);
    }

    const lanesTotal = tracked.plan.lanes.length;
    let lanesCompleted = 0;
    const failedNodes: string[] = [];

    if (tracked.result) {
      for (const laneResult of tracked.result.laneResults) {
        if (laneResult.success) lanesCompleted++;
        for (const txResult of laneResult.transactionResults) {
          if (!txResult.success) failedNodes.push(txResult.nodeId);
        }
      }
    }

    return {
      graphId,
      status: tracked.status,
      lanesCompleted,
      lanesTotal,
      failedNodes,
      settledAt: tracked.settledAt,
    };
  }

  /**
   * Execute a graph locally (without on-chain submission tracking).
   * Blocks until execution completes.
   */
  async executeLocally(graph: TransactionGraph): Promise<ExecutionResult> {
    const plan = this.planGraph(graph);
    return this.executor.execute(plan);
  }

  /**
   * Execute a graph via Jito bundles.
   */
  async executeViaJito(graph: TransactionGraph): Promise<BundleSubmissionResult[]> {
    const plan = this.planGraph(graph);
    return this.bundleBuilder.submitPlan(plan);
  }

  /**
   * Solve: parse an intent, build a graph, analyze, and plan.
   * Does not execute.
   */
  solve(
    input: string | object,
    options?: SolveOptions
  ): {
    intent: Intent;
    graph: TransactionGraph;
    analysis: AnalysisResult;
    plan: ExecutionPlan;
    fingerprint: string;
  } {
    const intent = this.parseIntent(input);
    const graph = this.buildGraph(intent);

    if (options?.autoDetectDeps !== false) {
      graph.autoDetectDependencies();
    }

    const analysis = this.analyzeGraph(graph);
    const plan = this.planGraph(graph, {
      maxLanes: this.config.maxParallelLanes,
    });
    const fingerprint = fingerprintGraph(graph);

    return { intent, graph, analysis, plan, fingerprint };
  }

  /**
   * Full pipeline: parse intent, build graph, plan, and execute.
   */
  async processIntent(
    input: string | object,
    options?: ProcessOptions
  ): Promise<{
    intent: Intent;
    graph: TransactionGraph;
    plan: ExecutionPlan;
    result: ExecutionResult | BundleSubmissionResult[];
  }> {
    const { intent, graph, plan } = this.solve(input, options);

    let result: ExecutionResult | BundleSubmissionResult[];

    if (options?.useJito) {
      result = await this.bundleBuilder.submitPlan(plan);
    } else {
      if (options?.simulate) {
        const simResults = await this.executor.simulatePlan(plan);
        const failures = Array.from(simResults.entries()).filter(
          ([, r]) => !r.success
        );
        if (failures.length > 0) {
          const failureMessages = failures
            .map(([id, r]) => `${id}: ${r.error}`)
            .join("; ");
          throw new Error(`Simulation failed: ${failureMessages}`);
        }
      }

      result = await this.executor.execute(plan);
    }

    return { intent, graph, plan, result };
  }

  /**
   * Register a callback for when a specific graph settles.
   */
  onGraphSettled(graphId: string, callback: GraphSettledCallback): () => void {
    const existing = this.settledCallbacks.get(graphId) ?? [];
    existing.push(callback);
    this.settledCallbacks.set(graphId, existing);

    // If already settled, fire immediately
    const tracked = this.trackedGraphs.get(graphId);
    if (
      tracked &&
      (tracked.status === GraphStatus.Settled ||
        tracked.status === GraphStatus.Failed ||
        tracked.status === GraphStatus.PartiallySettled)
    ) {
      callback(graphId, tracked.status, tracked.result);
    }

    return () => {
      const callbacks = this.settledCallbacks.get(graphId) ?? [];
      this.settledCallbacks.set(
        graphId,
        callbacks.filter((cb) => cb !== callback)
      );
    };
  }

  /**
   * Register a global listener for all graph settlements.
   */
  onAnyGraphSettled(callback: GraphSettledCallback): () => void {
    this.globalListeners.push(callback);
    return () => {
      this.globalListeners = this.globalListeners.filter(
        (cb) => cb !== callback
      );
    };
  }

  /**
   * Notify all registered listeners about graph settlement.
   */
  private notifySettled(
    graphId: string,
    status: GraphStatus,
    result: ExecutionResult | null
  ): void {
    // Graph-specific callbacks
    const callbacks = this.settledCallbacks.get(graphId) ?? [];
    for (const cb of callbacks) {
      try {
        cb(graphId, status, result);
      } catch {
        // Listener errors are non-fatal
      }
    }

    // Global callbacks
    for (const cb of this.globalListeners) {
      try {
        cb(graphId, status, result);
      } catch {
        // Listener errors are non-fatal
      }
    }
  }

  /**
   * Register an executor event listener (tx_submitted, tx_confirmed, etc.)
   */
  onExecutorEvent(listener: ExecutorEventListener): () => void {
    return this.executor.on(listener);
  }

  /**
   * Create a new TransactionGraphBuilder for manual graph construction.
   */
  createGraphBuilder(): TransactionGraphBuilder {
    return new TransactionGraphBuilder();
  }

  /**
   * Get the connection manager for advanced endpoint management.
   */
  getConnectionManager(): ConnectionManager {
    return this.connectionManager;
  }

  /**
   * Get the underlying ParallelExecutor.
   */
  getExecutor(): ParallelExecutor {
    return this.executor;
  }

  /**
   * Get the underlying BundleBuilder.
   */
  getBundleBuilder(): BundleBuilder {
    return this.bundleBuilder;
  }

  /**
   * Get the active Solana connection.
   */
  getConnection(): Connection {
    return this.connectionManager.getConnection();
  }

  /**
   * Get the wallet adapter.
   */
  getWallet(): WalletAdapter {
    return this.wallet;
  }

  /**
   * Get the current configuration.
   */
  getConfig(): Required<IvzaConfig> {
    return { ...this.config };
  }

  /**
   * Get the program ID for the IVZA on-chain program.
   */
  getProgramId(): PublicKey {
    return new PublicKey(this.config.programId);
  }

  /**
   * Get all tracked graph IDs.
   */
  getTrackedGraphIds(): string[] {
    return Array.from(this.trackedGraphs.keys());
  }

  /**
   * Get a summary of all tracked graphs.
   */
  getTrackedGraphsSummary(): Array<{
    graphId: string;
    status: GraphStatus;
    laneCount: number;
    createdAt: number;
    settledAt?: number;
  }> {
    return Array.from(this.trackedGraphs.values()).map((t) => ({
      graphId: t.graphId,
      status: t.status,
      laneCount: t.plan.lanes.length,
      createdAt: t.createdAt,
      settledAt: t.settledAt,
    }));
  }

  /**
   * Clear all tracked graphs and callbacks.
   */
  clearTrackedGraphs(): void {
    this.trackedGraphs.clear();
    this.settledCallbacks.clear();
  }

  /**
   * Serialize a graph for storage or wire transfer.
   */
  serializeGraph(graph: TransactionGraph): SerializedGraph {
    return serializeGraph(graph);
  }

  /**
   * Deserialize a graph from storage or wire transfer.
   */
  deserializeGraph(data: SerializedGraph): TransactionGraph {
    return deserializeGraph(data);
  }

  /**
   * Destroy the client and release all resources.
   */
  destroy(): void {
    this.connectionManager.destroy();
    this.trackedGraphs.clear();
    this.settledCallbacks.clear();
    this.globalListeners = [];
  }
}
