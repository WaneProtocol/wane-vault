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
 * Main entry point for the iVZA Parallel Execution Engine SDK.
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