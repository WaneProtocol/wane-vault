import {
  Connection,
  Transaction,
  TransactionSignature,
  PublicKey,
  ComputeBudgetProgram,
  SendOptions,
  Commitment,
  TransactionMessage,
  VersionedTransaction,
  TransactionInstruction,
} from "@solana/web3.js";
import {
  ExecutionPlan,
  ExecutionLane,
  TransactionResult,
  LaneResult,
  ExecutionResult,
  IvzaConfig,
  DEFAULT_CONFIG,
  WalletAdapter,
  TransactionNode,
} from "../types";
import { GraphNode } from "../graph/node";

/**
 * Event types emitted by the ParallelExecutor during execution.
 */
export type ExecutorEvent =
  | { type: "lane_start"; laneIndex: number; nodeCount: number }
  | { type: "lane_complete"; laneIndex: number; success: boolean; timeMs: number }
  | { type: "tx_submitted"; nodeId: string; signature: string }
  | { type: "tx_confirmed"; nodeId: string; signature: string; slot: number }
  | { type: "tx_failed"; nodeId: string; error: string; retriesLeft: number }
  | { type: "execution_complete"; success: boolean; totalTimeMs: number };

/**
 * Listener callback for executor events.
 */
export type ExecutorEventListener = (event: ExecutorEvent) => void;

/**
 * Options for configuring parallel execution behavior.
 */
export interface ParallelExecutionOptions {
  /** Maximum retries per transaction */
  maxRetries: number;
  /** Base delay for exponential backoff in ms */
  baseRetryDelayMs: number;
  /** Maximum backoff delay in ms */
  maxRetryDelayMs: number;
  /** Commitment level for confirmations */
  commitment: Commitment;
  /** Timeout for transaction confirmation in ms */
  confirmTimeoutMs: number;
  /** Skip preflight simulation */
  skipPreflight: boolean;
  /** Abort all remaining on first failure */
  failFast: boolean;
  /** Default priority fee in micro-lamports per CU */
  defaultPriorityFee: number;
  /** Default compute unit limit */
  defaultComputeUnits: number;
  /** Whether to simulate before sending */
  simulateFirst: boolean;
  /** Minimum context slot for send */
  minContextSlot?: number;
}

export const DEFAULT_PARALLEL_OPTIONS: ParallelExecutionOptions = {
  maxRetries: 3,
  baseRetryDelayMs: 500,
  maxRetryDelayMs: 10_000,
  commitment: "confirmed",
  confirmTimeoutMs: 30_000,
  skipPreflight: false,
  failFast: false,
  defaultPriorityFee: 1_000,
  defaultComputeUnits: 200_000,
  simulateFirst: true,
};

/**
 * Executes transaction graphs in parallel lanes.
 *
 * Lanes represent groups of independent transactions. Within each lane,
 * transactions may depend on each other and are executed sequentially.
 * Lanes themselves are executed in parallel using Promise.all.
 *
 * Retry logic uses exponential backoff with jitter.
 */
export class ParallelExecutor {
  private connection: Connection;
  private wallet: WalletAdapter;
  private options: ParallelExecutionOptions;
  private listeners: ExecutorEventListener[] = [];
  private aborted = false;

  constructor(
    connection: Connection,
    wallet: WalletAdapter,
    options?: Partial<ParallelExecutionOptions>
  ) {
    this.connection = connection;
    this.wallet = wallet;
    this.options = { ...DEFAULT_PARALLEL_OPTIONS, ...options };
  }

  /**
   * Register an event listener.
   */
  on(listener: ExecutorEventListener): () => void {
    this.listeners.push(listener);
    return () => {
      this.listeners = this.listeners.filter((l) => l !== listener);
    };
  }

  /**
   * Emit an event to all registered listeners.
   */
  private emit(event: ExecutorEvent): void {
    for (const listener of this.listeners) {
      try {
        listener(event);
      } catch {
        // Listener errors should not break execution
      }
    }
  }

  /**
   * Execute a full execution plan.
   *
   * Lanes are dispatched in parallel. Within each lane, nodes are
   * executed sequentially (respecting intra-lane dependencies).
   */
  async execute(plan: ExecutionPlan): Promise<ExecutionResult> {
    this.aborted = false;
    const startTime = Date.now();
    const laneResults: LaneResult[] = [];

    // Execute all lanes in parallel
    const lanePromises = plan.lanes.map((lane) => this.executeLane(lane));
    const results = await Promise.allSettled(lanePromises);

    for (const result of results) {
      if (result.status === "fulfilled") {
        laneResults.push(result.value);
      } else {
        // Should not happen since executeLane catches errors internally
        laneResults.push({
          laneIndex: laneResults.length,
          transactionResults: [],
          totalTime: 0,
          success: false,
        });
      }
    }

    const totalTime = Date.now() - startTime;
    const allSuccess = laneResults.every((lr) => lr.success);
    const failedNodes = laneResults.flatMap((lr) =>
      lr.transactionResults.filter((tr) => !tr.success).map((tr) => tr.nodeId)
    );

    this.emit({
      type: "execution_complete",
      success: allSuccess,
      totalTimeMs: totalTime,
    });

    return {
      planId: plan.graphId,
      laneResults,
      totalTime,
      success: allSuccess,
      failedNodes,
    };
  }

  /**
   * Execute a single lane sequentially.
   */
  private async executeLane(lane: ExecutionLane): Promise<LaneResult> {
    const startTime = Date.now();
    const txResults: TransactionResult[] = [];

    this.emit({
      type: "lane_start",
      laneIndex: lane.laneIndex,
      nodeCount: lane.nodes.length,
    });

    let laneSuccess = true;

    for (const node of lane.nodes) {
      if (this.aborted) {
        txResults.push({
          nodeId: node.id,
          signature: null,
          success: false,
          error: "Execution aborted",
        });
        laneSuccess = false;
        continue;
      }

      const result = await this.executeNode(node);
      txResults.push(result);

      if (!result.success) {
        laneSuccess = false;
        if (this.options.failFast) {
          this.aborted = true;
          // Mark remaining nodes as skipped
          const nodeIndex = lane.nodes.indexOf(node);
          for (let i = nodeIndex + 1; i < lane.nodes.length; i++) {
            txResults.push({
              nodeId: lane.nodes[i].id,
              signature: null,
              success: false,
              error: "Skipped due to upstream failure (failFast)",
            });
          }
          break;
        }
      }
    }

    const totalTime = Date.now() - startTime;

    this.emit({
      type: "lane_complete",
      laneIndex: lane.laneIndex,
      success: laneSuccess,
      timeMs: totalTime,
    });

    return {
      laneIndex: lane.laneIndex,
      transactionResults: txResults,
      totalTime,
      success: laneSuccess,
    };
  }

  /**
   * Execute a single transaction node with retry logic.
   */
  private async executeNode(node: TransactionNode): Promise<TransactionResult> {
    let lastError = "";
    const maxRetries = this.options.maxRetries;

    for (let attempt = 0; attempt <= maxRetries; attempt++) {
      try {
        // Build the transaction
        const tx = await this.buildTransaction(node);

        // Simulate if configured
        if (this.options.simulateFirst && attempt === 0) {
          const simResult = await this.connection.simulateTransaction(tx);
          if (simResult.value.err) {
            const errMsg =
              typeof simResult.value.err === "string"
                ? simResult.value.err
                : JSON.stringify(simResult.value.err);
            throw new Error(`Simulation failed: ${errMsg}`);
          }
        }

        // Sign the transaction
        const signedTx = await this.wallet.signTransaction(tx);

        // Send the transaction
        const sendOptions: SendOptions = {
          skipPreflight: this.options.skipPreflight,
          preflightCommitment: this.options.commitment,