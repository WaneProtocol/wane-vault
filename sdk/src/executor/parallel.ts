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
          minContextSlot: this.options.minContextSlot,
        };

        const signature = await this.connection.sendRawTransaction(
          signedTx.serialize(),
          sendOptions
        );

        this.emit({ type: "tx_submitted", nodeId: node.id, signature });

        // Confirm the transaction
        const confirmation = await this.confirmTransaction(signature);

        if (confirmation.error) {
          throw new Error(`Confirmation failed: ${confirmation.error}`);
        }

        this.emit({
          type: "tx_confirmed",
          nodeId: node.id,
          signature,
          slot: confirmation.slot,
        });

        return {
          nodeId: node.id,
          signature,
          success: true,
          slot: confirmation.slot,
          computeUnitsUsed: confirmation.computeUnitsUsed,
          confirmationTime: confirmation.timeMs,
        };
      } catch (err: unknown) {
        const errorMessage =
          err instanceof Error ? err.message : String(err);
        lastError = errorMessage;

        const retriesLeft = maxRetries - attempt;

        this.emit({
          type: "tx_failed",
          nodeId: node.id,
          error: errorMessage,
          retriesLeft,
        });

        // Don't retry on certain errors
        if (this.isNonRetryableError(errorMessage)) {
          break;
        }

        if (attempt < maxRetries) {
          const delay = this.calculateBackoff(attempt);
          await this.sleep(delay);
        }
      }
    }

    return {
      nodeId: node.id,
      signature: null,
      success: false,
      error: lastError,
    };
  }

  /**
   * Build a Transaction from a node, adding compute budget instructions.
   */
  private async buildTransaction(node: TransactionNode): Promise<Transaction> {
    const tx = new Transaction();

    // Set compute unit limit
    const cuLimit = node.estimatedCu || this.options.defaultComputeUnits;
    tx.add(
      ComputeBudgetProgram.setComputeUnitLimit({
        units: cuLimit,
      })
    );

    // Set priority fee
    tx.add(
      ComputeBudgetProgram.setComputeUnitPrice({
        microLamports: this.options.defaultPriorityFee,
      })
    );

    // Add the node's instructions
    for (const ix of node.instructions) {
      tx.add(ix);
    }

    // Set fee payer and recent blockhash
    tx.feePayer = this.wallet.publicKey;
    const { blockhash, lastValidBlockHeight } =
      await this.connection.getLatestBlockhash(this.options.commitment);
    tx.recentBlockhash = blockhash;

    return tx;
  }

  /**
   * Confirm a transaction with timeout.
   */
  private async confirmTransaction(
    signature: TransactionSignature
  ): Promise<{
    error: string | null;
    slot: number;
    computeUnitsUsed?: number;
    timeMs: number;
  }> {
    const startTime = Date.now();
    const timeout = this.options.confirmTimeoutMs;

    return new Promise((resolve) => {
      let resolved = false;
      const timer = setTimeout(() => {
        if (!resolved) {
          resolved = true;
          resolve({
            error: `Confirmation timed out after ${timeout}ms`,
            slot: 0,
            timeMs: Date.now() - startTime,
          });
        }
      }, timeout);

      this.connection
        .confirmTransaction(
          {
            signature,
            blockhash: "", // Will use the one from tx
            lastValidBlockHeight: 0, // Will poll until timeout
          },
          this.options.commitment
        )
        .then((result) => {
          if (!resolved) {
            resolved = true;
            clearTimeout(timer);

            if (result.value.err) {
              resolve({
                error: JSON.stringify(result.value.err),
                slot: result.context.slot,
                timeMs: Date.now() - startTime,
              });
            } else {
              resolve({
                error: null,
                slot: result.context.slot,
                timeMs: Date.now() - startTime,
              });
            }
          }
        })
        .catch((err: unknown) => {
          if (!resolved) {
            resolved = true;
            clearTimeout(timer);
            resolve({
              error: err instanceof Error ? err.message : String(err),
              slot: 0,
              timeMs: Date.now() - startTime,
            });
          }
        });
    });
  }

  /**
   * Check if an error is non-retryable.
   */
  private isNonRetryableError(error: string): boolean {
    const nonRetryable = [
      "Blockhash not found",
      "insufficient funds",
      "Account not found",
      "invalid account data",
      "Transaction simulation failed: Error processing Instruction",
      "already been processed",
      "Program failed to complete",
    ];
    return nonRetryable.some((msg) => error.includes(msg));
  }

  /**
   * Calculate exponential backoff delay with jitter.
   */
  private calculateBackoff(attempt: number): number {
    const base = this.options.baseRetryDelayMs;
    const max = this.options.maxRetryDelayMs;
    const exponential = Math.min(base * Math.pow(2, attempt), max);
    // Add jitter: random value between 0 and exponential
    const jitter = Math.random() * exponential * 0.5;
    return Math.floor(exponential + jitter);
  }

  /**
   * Sleep for a given number of milliseconds.
   */
  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  /**
   * Abort the current execution. In-flight transactions will
   * complete, but no new transactions will be submitted.
   */
  abort(): void {
    this.aborted = true;
  }

  /**
   * Check if execution has been aborted.
   */
  get isAborted(): boolean {
    return this.aborted;
  }

  /**
   * Execute a batch of independent transactions in parallel (no lanes).
   * Useful for simple cases where all transactions are independent.
   */
  async executeBatch(
    nodes: TransactionNode[]
  ): Promise<TransactionResult[]> {
    const promises = nodes.map((node) => this.executeNode(node));
    return Promise.all(promises);
  }

  /**
   * Simulate all transactions in a plan without sending them.
   * Returns simulation results for each node.
   */
  async simulatePlan(
    plan: ExecutionPlan
  ): Promise<
    Map<string, { success: boolean; error?: string; unitsConsumed?: number }>
  > {
    const results = new Map<
      string,
      { success: boolean; error?: string; unitsConsumed?: number }
    >();

    for (const lane of plan.lanes) {
      for (const node of lane.nodes) {
        try {
          const tx = await this.buildTransaction(node);
          const signedTx = await this.wallet.signTransaction(tx);
          const simResult = await this.connection.simulateTransaction(signedTx);

          if (simResult.value.err) {
            results.set(node.id, {
              success: false,
              error: JSON.stringify(simResult.value.err),
              unitsConsumed: simResult.value.unitsConsumed ?? undefined,
            });
          } else {
            results.set(node.id, {
              success: true,
              unitsConsumed: simResult.value.unitsConsumed ?? undefined,
            });
          }
        } catch (err: unknown) {
          results.set(node.id, {
            success: false,
            error: err instanceof Error ? err.message : String(err),
          });
        }
      }
    }

    return results;
  }
}
