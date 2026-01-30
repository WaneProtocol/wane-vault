import { Intent, IntentType } from "../types";

/**
 * Supported DSL tokens for intent parsing.
 */
export const DSL_VERBS: Record<string, IntentType> = {
  swap: IntentType.Swap,
  exchange: IntentType.Swap,
  trade: IntentType.Swap,
  convert: IntentType.Swap,
  stake: IntentType.Stake,
  delegate: IntentType.Stake,
  unstake: IntentType.Unstake,
  undelegate: IntentType.Unstake,
  withdraw: IntentType.Unstake,
  transfer: IntentType.Transfer,
  send: IntentType.Transfer,
  provide: IntentType.ProvideLiquidity,
  "add-liquidity": IntentType.ProvideLiquidity,
  "add_liquidity": IntentType.ProvideLiquidity,
};

/**
 * Parsed DSL token stream.
 */
export interface DslTokens {
  verb: string;
  amount: number;
  tokenA: string;
  preposition: string;
  tokenB: string;
  amountB?: number;
  extras: Record<string, string>;
}

/**
 * Validation error for an intent.
 */
export interface IntentValidationError {
  field: string;
  message: string;
  value?: unknown;
}

/**
 * Result of intent validation.
 */
export interface IntentValidationResult {
  valid: boolean;
  errors: IntentValidationError[];
  warnings: string[];
}

/**
 * JSON-format intent input (alternative to DSL string).
 */
export interface JsonIntentInput {
  type: string;
  params: Record<string, unknown>;
  priority?: number;
}
