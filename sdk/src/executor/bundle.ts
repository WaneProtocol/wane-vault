import {
  Connection,
  Transaction,
  PublicKey,
  SystemProgram,
  ComputeBudgetProgram,
  Keypair,
  LAMPORTS_PER_SOL,
  TransactionInstruction,
} from "@solana/web3.js";
import {
  ExecutionPlan,
  ExecutionLane,
  TransactionNode,
  WalletAdapter,
  IvzaConfig,
  DEFAULT_CONFIG,
} from "../types";

/**
 * Well-known Jito tip accounts on mainnet.
 */
const JITO_TIP_ACCOUNTS: string[] = [
  "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
  "HFqU5x63VTqvQss8hp11i4bVqkfRtQ7NmXwkiNPLNiU8",
  "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
  "ADaUMid9yfUytqMBgopwjb2DTLSh9NJpVf5Eg1Pw6Kmi",
  "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
  "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
  "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
  "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

/**
 * Configuration for bundle building.
 */
export interface BundleConfig {
  /** Jito block engine endpoint */
  jitoEndpoint: string;
  /** Tip amount in lamports */
  tipLamports: number;
  /** Maximum transactions per bundle (Jito limit is 5) */
  maxTransactionsPerBundle: number;
  /** Priority fee in micro-lamports per CU */
  priorityFee: number;
  /** Default compute unit limit per transaction */
  defaultComputeUnits: number;
}

const DEFAULT_BUNDLE_CONFIG: BundleConfig = {
  jitoEndpoint: DEFAULT_CONFIG.jitoEndpoint,
  tipLamports: DEFAULT_CONFIG.jitoTipLamports,
  maxTransactionsPerBundle: 5,
  priorityFee: 1_000,
  defaultComputeUnits: 200_000,
};

/**
 * A serialized bundle ready for submission to Jito.
 */
export interface SerializedBundle {
  /** Base64-encoded serialized transactions */
  transactions: string[];
  /** Tip amount included */
  tipLamports: number;
  /** Tip account used */
  tipAccount: string;
  /** Estimated total compute units */
  estimatedCU: number;
  /** Number of transactions in the bundle */
  transactionCount: number;
}

/**
 * Result of submitting a bundle to Jito.
 */
export interface BundleSubmissionResult {
  /** Bundle ID returned by Jito */
  bundleId: string;
  /** Whether submission was accepted */
  accepted: boolean;
  /** Error message if rejected */
  error?: string;
  /** Timestamp of submission */
  submittedAt: number;
}

/**
 * Result of checking bundle status.
 */
export interface BundleStatusResult {
  bundleId: string;
  status: "pending" | "landed" | "failed" | "invalid";
  slot?: number;
  error?: string;
}

/**
 * Builds Jito-compatible transaction bundles from execution plans.
 *
 * Bundles pack multiple transactions together for atomic execution via
 * Jito's block engine. Each bundle includes a tip transaction to
 * incentivize the validator.
 */
export class BundleBuilder {
  private connection: Connection;
  private wallet: WalletAdapter;
  private config: BundleConfig;

  constructor(
    connection: Connection,
    wallet: WalletAdapter,
    config?: Partial<BundleConfig>
  ) {
    this.connection = connection;
    this.wallet = wallet;
    this.config = { ...DEFAULT_BUNDLE_CONFIG, ...config };
  }

  /**
   * Build bundles from an execution plan.
   * Each lane becomes one or more bundles (respecting the 5-tx limit).
   */
  async buildFromPlan(plan: ExecutionPlan): Promise<SerializedBundle[]> {
    const bundles: SerializedBundle[] = [];

    for (const lane of plan.lanes) {
      const laneBundles = await this.buildLaneBundles(lane);
      bundles.push(...laneBundles);
    }

    return bundles;
  }

  /**
   * Build bundles for a single execution lane.
   */
  private async buildLaneBundles(
    lane: ExecutionLane
  ): Promise<SerializedBundle[]> {
    const bundles: SerializedBundle[] = [];
    // Reserve 1 slot for the tip transaction
    const maxTxPerBundle = this.config.maxTransactionsPerBundle - 1;
    const nodes = lane.nodes;

    for (let i = 0; i < nodes.length; i += maxTxPerBundle) {
      const chunk = nodes.slice(i, i + maxTxPerBundle);
      const bundle = await this.buildBundle(chunk);
      bundles.push(bundle);
    }

    return bundles;
  }

  /**
   * Build a single bundle from a set of transaction nodes.
   */
  async buildBundle(nodes: TransactionNode[]): Promise<SerializedBundle> {
    const { blockhash } = await this.connection.getLatestBlockhash("confirmed");
    const tipAccount = this.selectTipAccount();
    const transactions: string[] = [];
    let totalCU = 0;

    // Build each node's transaction
    for (const node of nodes) {
      const tx = new Transaction();

      // Compute budget instructions
      const cuLimit = node.estimatedCu || this.config.defaultComputeUnits;
      tx.add(
        ComputeBudgetProgram.setComputeUnitLimit({ units: cuLimit })
      );
      tx.add(
        ComputeBudgetProgram.setComputeUnitPrice({
          microLamports: this.config.priorityFee,
        })
      );

      // Add the node's instructions
      for (const ix of node.instructions) {
        tx.add(ix);
      }

      tx.feePayer = this.wallet.publicKey;
      tx.recentBlockhash = blockhash;

      const signedTx = await this.wallet.signTransaction(tx);
      transactions.push(signedTx.serialize().toString("base64"));
      totalCU += cuLimit;
    }

    // Build and append the tip transaction
    const tipTx = await this.buildTipTransaction(tipAccount, blockhash);
    const signedTipTx = await this.wallet.signTransaction(tipTx);
    transactions.push(signedTipTx.serialize().toString("base64"));

    return {
      transactions,
      tipLamports: this.config.tipLamports,
      tipAccount,
      estimatedCU: totalCU,
      transactionCount: transactions.length,
    };
  }

  /**
   * Build a tip transaction for the Jito validator.
   */
  private async buildTipTransaction(
    tipAccount: string,
    blockhash: string
  ): Promise<Transaction> {
    const tx = new Transaction();

    tx.add(
      SystemProgram.transfer({
        fromPubkey: this.wallet.publicKey,
        toPubkey: new PublicKey(tipAccount),
        lamports: this.config.tipLamports,
      })
    );

    tx.feePayer = this.wallet.publicKey;
    tx.recentBlockhash = blockhash;

    return tx;
  }

  /**
   * Select a random Jito tip account for load distribution.
   */
  private selectTipAccount(): string {
    const index = Math.floor(Math.random() * JITO_TIP_ACCOUNTS.length);
    return JITO_TIP_ACCOUNTS[index];
  }

  /**
   * Calculate the optimal tip amount based on bundle contents.
   * Higher compute = higher tip for better inclusion probability.
   */
  calculateOptimalTip(totalCU: number, urgencyMultiplier: number = 1): number {
    // Base: 10,000 lamports (0.00001 SOL)
    const baseTip = 10_000;
    // Scale by compute units: ~1 lamport per 100 CU
    const cuComponent = Math.floor(totalCU / 100);
    // Apply urgency multiplier
    const optimalTip = Math.floor((baseTip + cuComponent) * urgencyMultiplier);
    // Clamp between 10,000 and 1,000,000 lamports (0.001 SOL)
    return Math.max(10_000, Math.min(optimalTip, 1_000_000));
  }

  /**
   * Submit a serialized bundle to the Jito block engine.
   */
  async submitBundle(bundle: SerializedBundle): Promise<BundleSubmissionResult> {
    const endpoint = this.config.jitoEndpoint;

    try {
      const response = await fetch(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "sendBundle",
          params: [bundle.transactions],
        }),
      });

      const data = (await response.json()) as {
        result?: string;
        error?: { message: string };
      };

      if (data.error) {
        return {
          bundleId: "",
          accepted: false,
          error: data.error.message,
          submittedAt: Date.now(),
        };
      }

      return {
        bundleId: data.result ?? "",
        accepted: true,
        submittedAt: Date.now(),
      };
    } catch (err: unknown) {
      return {
        bundleId: "",
        accepted: false,
        error: err instanceof Error ? err.message : String(err),
        submittedAt: Date.now(),
      };
    }
  }

  /**
   * Check the status of a submitted bundle.
   */
  async getBundleStatus(bundleId: string): Promise<BundleStatusResult> {
    const endpoint = this.config.jitoEndpoint.replace(
      "/bundles",
      "/bundles"
    );

    try {
      const response = await fetch(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "getBundleStatuses",
          params: [[bundleId]],
        }),
      });

      const data = (await response.json()) as {
        result?: {
          value: Array<{
            bundle_id: string;
            status: string;
            slot?: number;
            err?: { message: string };
          }>;
        };
        error?: { message: string };
      };

      if (data.error || !data.result?.value?.length) {
        return {
          bundleId,
          status: "pending",
          error: data.error?.message,
        };
      }

      const bundleStatus = data.result.value[0];
      const statusMap: Record<string, BundleStatusResult["status"]> = {
        Landed: "landed",
        Failed: "failed",
        Invalid: "invalid",
        Pending: "pending",
      };

      return {
        bundleId,
        status: statusMap[bundleStatus.status] ?? "pending",
        slot: bundleStatus.slot,
        error: bundleStatus.err?.message,
      };
    } catch (err: unknown) {
      return {
        bundleId,
        status: "pending",
        error: err instanceof Error ? err.message : String(err),
      };
    }
  }

  /**
   * Submit all bundles from a plan and return results.
   */
  async submitPlan(
    plan: ExecutionPlan
  ): Promise<BundleSubmissionResult[]> {
    const bundles = await this.buildFromPlan(plan);
    const results: BundleSubmissionResult[] = [];

    for (const bundle of bundles) {
      const result = await this.submitBundle(bundle);
      results.push(result);
    }

    return results;
  }

  /**
   * Wait for a bundle to reach a terminal status.
   */
  async waitForBundle(
    bundleId: string,
    timeoutMs: number = 60_000,
    pollIntervalMs: number = 2_000
  ): Promise<BundleStatusResult> {
    const startTime = Date.now();

    while (Date.now() - startTime < timeoutMs) {
      const status = await this.getBundleStatus(bundleId);

      if (status.status !== "pending") {
        return status;
      }

      await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
    }

    return {
      bundleId,
      status: "failed",
      error: `Timed out after ${timeoutMs}ms`,
    };
  }
}
