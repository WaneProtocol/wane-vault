export {
  AccountAccess,
  AccountLockState,
  AccountLockEntry,
  AccountLockManager,
} from "./account";

export {
  DependencyType,
  PriorityLevel,
  TransactionNode,
  GraphEdge,
  ExecutionLane,
  ExecutionPlan,
  TransactionResult,
  ExecutionResult,
  LaneResult,
  GraphStatus,
  GraphState,
  AnalysisResult,
  AccountConflict,
} from "./transaction";

/**
 * Intent types supported by the iVZA engine.
 */
export enum IntentType {
  Swap = "swap",
  MultiHopSwap = "multi_hop_swap",
  Stake = "stake",
  Unstake = "unstake",
  ProvideLiquidity = "provide_liquidity",
  Transfer = "transfer",
}

/**
 * Parameters for a swap intent.
 */
export interface SwapParams {
  inputMint: string;
  outputMint: string;
  amount: number;
  slippageBps: number;
  maxAccounts?: number;
}

/**
 * Parameters for a multi-hop swap intent.
 */
export interface MultiHopSwapParams {
  hops: Array<{ inputMint: string; outputMint: string }>;
  amount: number;
  slippageBps: number;
}

/**
 * Parameters for a stake intent.
 */
export interface StakeParams {
  amount: number;
  validatorVote?: string;
}

/**
 * Parameters for an unstake intent.
 */
export interface UnstakeParams {
  amount: number;
  stakeAccount?: string;
}

/**
 * Parameters for a provide-liquidity intent.
 */
export interface ProvideLiquidityParams {
  tokenAMint: string;
  tokenBMint: string;
  amountA: number;
  amountB: number;
  poolAddress?: string;
}

/**
 * Parameters for a transfer intent.
 */
export interface TransferParams {
  mint: string;
  amount: number;
  recipient: string;
}

export type IntentParams =
  | SwapParams
  | MultiHopSwapParams
  | StakeParams
  | UnstakeParams
  | ProvideLiquidityParams
  | TransferParams;

/**
 * A parsed user intent.
 */
export interface Intent {
  type: IntentType;
  params: IntentParams;
  id: string;
  createdAt: number;
  priority: number;
}

/**
 * A single hop in a swap route.
 */
export interface RouteHop {
  inputMint: string;
  outputMint: string;
  ammId: string;
  ammLabel: string;
  inputAmount: number;
  outputAmount: number;
  fee: number;
}

/**
 * A complete route for a swap.
 */
export interface Route {
  hops: RouteHop[];
  inputAmount: number;
  outputAmount: number;
  priceImpact: number;
  totalFee: number;
}

/**
 * Result of solving / routing an intent.
 */
export interface SolverResult {
  intent: Intent;
  routes: Route[];
  selectedRoute: Route;
  estimatedCu: number;
  estimatedTime: number;
}

/**
 * Configuration for the iVZA client.
 */
export interface IvzaConfig {
  rpcEndpoints: string[];
  jitoEndpoint?: string;
  jitoTipLamports?: number;
  maxRetries?: number;
  retryDelayMs?: number;
  confirmationTimeout?: number;
  maxParallelLanes?: number;
  defaultPriority?: number;
  programId?: string;
}

/**
 * Default configuration values.
 */
export const DEFAULT_CONFIG: Required<IvzaConfig> = {
  rpcEndpoints: ["https://api.mainnet-beta.solana.com"],
  jitoEndpoint: "https://mainnet.block-engine.jito.wtf/api/v1/bundles",
  jitoTipLamports: 10_000,
  maxRetries: 3,
  retryDelayMs: 500,
  confirmationTimeout: 60_000,
  maxParallelLanes: 4,
  defaultPriority: 1,
  programId: "ivzaParaExec11111111111111111111111111111",
};

/**
 * Well-known token mints on Solana mainnet.
 */
export const KNOWN_MINTS: Record<string, string> = {
  SOL: "So11111111111111111111111111111111111111112",
  USDC: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  USDT: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
  RAY: "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R",
  SRM: "SRMuApVNdxXokk5GT7XD5cUUgXMBCoAz2LHeuAoKWRt",
  MNGO: "MangoCzJ36AjZyKwVj3VnYU4GTonjfVEnJmvvWaxLac",
  ORCA: "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE",
  BONK: "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",
  JTO: "jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL",
  WIF: "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm",
};

/**
 * Wallet adapter interface used by the SDK.
 */
export interface WalletAdapter {
  publicKey: import("@solana/web3.js").PublicKey;
  signTransaction: (
    tx: import("@solana/web3.js").Transaction
  ) => Promise<import("@solana/web3.js").Transaction>;
  signAllTransactions: (
    txs: import("@solana/web3.js").Transaction[]
  ) => Promise<import("@solana/web3.js").Transaction[]>;
}
