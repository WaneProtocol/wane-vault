import {
  Intent,
  IntentType,
  SwapParams,
  MultiHopSwapParams,
  StakeParams,
  UnstakeParams,
  ProvideLiquidityParams,
  TransferParams,
  IntentParams,
  KNOWN_MINTS,
} from "../types";
import {
  DSL_VERBS,
  DslTokens,
  IntentValidationError,
  IntentValidationResult,
  JsonIntentInput,
} from "./types";

/**
 * Parses user intent from either a DSL string or JSON object.
 *
 * DSL format examples:
 *   "swap 100 USDC to SOL"
 *   "swap 100 USDC to SOL slippage 50"
 *   "stake 10 SOL"
 *   "unstake 5 SOL"
 *   "transfer 50 USDC to <pubkey>"
 *   "provide 100 USDC and 0.5 SOL"
 *   "swap 100 USDC to RAY to SOL" (multi-hop)
 */
export class IntentParser {
  /**
   * Parse an intent from a string (DSL) or object (JSON).
   */
  parse(input: string | object): Intent {
    if (typeof input === "string") {
      return this.parseDsl(input);
    }
    return this.parseJson(input as JsonIntentInput);
  }

  /**
   * Parse a DSL string into an Intent.
   */
  private parseDsl(input: string): Intent {
    const normalized = input.trim().toLowerCase();
    const tokens = this.tokenize(normalized);

    const intentType = DSL_VERBS[tokens.verb];
    if (!intentType) {
      throw new Error(`Unknown intent verb: '${tokens.verb}'`);
    }

    let params: IntentParams;

    switch (intentType) {
      case IntentType.Swap:
        params = this.buildSwapParams(tokens, normalized);
        break;
      case IntentType.Stake:
        params = this.buildStakeParams(tokens);
        break;
      case IntentType.Unstake:
        params = this.buildUnstakeParams(tokens);
        break;
      case IntentType.Transfer:
        params = this.buildTransferParams(tokens);
        break;
      case IntentType.ProvideLiquidity:
        params = this.buildLiquidityParams(tokens);
        break;
      default:
        throw new Error(`Unsupported intent type: ${intentType}`);
    }

    // Check for multi-hop swap (e.g., "swap 100 USDC to RAY to SOL")
    if (intentType === IntentType.Swap) {
      const multiHopParams = this.detectMultiHop(normalized, tokens);
      if (multiHopParams) {
        return {
          type: IntentType.MultiHopSwap,
          params: multiHopParams,
          id: this.generateId(),
          createdAt: Date.now(),
          priority: 1,
        };
      }
    }

    return {
      type: intentType,
      params,
      id: this.generateId(),
      createdAt: Date.now(),
      priority: 1,
    };
  }

  /**
   * Parse a JSON object into an Intent.
   */
  private parseJson(input: JsonIntentInput): Intent {
    const type = this.resolveIntentType(input.type);
    const params = this.buildParamsFromJson(type, input.params);

    return {
      type,
      params,
      id: this.generateId(),
      createdAt: Date.now(),
      priority: input.priority ?? 1,
    };
  }

  /**
   * Tokenize a DSL string.
   */
  private tokenize(input: string): DslTokens {
    const words = input.split(/\s+/);
    if (words.length < 2) {
      throw new Error("Intent string too short; expected at least a verb and amount");
    }

    const verb = words[0];
    const amountStr = words[1];
    const amount = parseFloat(amountStr);
    if (isNaN(amount)) {
      throw new Error(`Invalid amount: '${amountStr}'`);
    }

    const tokenA = words[2]?.toUpperCase() ?? "";
    const preposition = words[3] ?? "";
    const tokenB = words[4]?.toUpperCase() ?? "";

    // Parse extras (key-value pairs after main tokens)
    const extras: Record<string, string> = {};
    for (let i = 5; i < words.length; i += 2) {
      if (i + 1 < words.length) {
        extras[words[i]] = words[i + 1];
      }
    }

    // Check for "and" preposition for liquidity
    let amountB: number | undefined;
    if (preposition === "and" && tokenB) {
      const amountBStr = words[4];
      amountB = parseFloat(amountBStr ?? "0");
      if (isNaN(amountB)) amountB = undefined;
    }

    return { verb, amount, tokenA, preposition, tokenB, amountB, extras };
  }

  /**
   * Build swap parameters from DSL tokens.
   */
  private buildSwapParams(tokens: DslTokens, raw: string): SwapParams {
    const inputMint = this.resolveMint(tokens.tokenA);
    const outputMint = this.resolveMint(tokens.tokenB);
    const slippageBps = tokens.extras["slippage"]
      ? parseInt(tokens.extras["slippage"], 10)
      : 50;

    return {
      inputMint,
      outputMint,
      amount: tokens.amount,
      slippageBps,
    };
  }

  /**
   * Build stake parameters from DSL tokens.
   */
  private buildStakeParams(tokens: DslTokens): StakeParams {
    return {
      amount: tokens.amount,
      validatorVote: tokens.extras["validator"],
    };
  }

  /**
   * Build unstake parameters from DSL tokens.
   */
  private buildUnstakeParams(tokens: DslTokens): UnstakeParams {
    return {
      amount: tokens.amount,
      stakeAccount: tokens.extras["account"],
    };
  }

  /**
   * Build transfer parameters from DSL tokens.
   */
  private buildTransferParams(tokens: DslTokens): TransferParams {
    const mint = this.resolveMint(tokens.tokenA);
    return {
      mint,
      amount: tokens.amount,
      recipient: tokens.tokenB || tokens.extras["to"] || "",
    };
  }

  /**
   * Build liquidity parameters from DSL tokens.
   */
  private buildLiquidityParams(tokens: DslTokens): ProvideLiquidityParams {
    // "provide 100 USDC and 0.5 SOL"
    const words = tokens.verb === "provide" ? tokens : tokens;
    const tokenAMint = this.resolveMint(tokens.tokenA);

    // Re-parse for the "and X TOKEN" pattern
    const tokenBMint = tokens.extras["tokenb"]
      ? this.resolveMint(tokens.extras["tokenb"])
      : this.resolveMint(tokens.tokenB);
    const amountB = tokens.amountB ?? 0;

    return {
      tokenAMint,
      tokenBMint,
      amountA: tokens.amount,
      amountB,
      poolAddress: tokens.extras["pool"],
    };
  }

  /**
   * Detect multi-hop swaps: "swap 100 USDC to RAY to SOL"
   */
  private detectMultiHop(
    raw: string,
    tokens: DslTokens
  ): MultiHopSwapParams | null {
    const words = raw.split(/\s+/);
    // Find all "to" prepositions after the verb
    const toIndices: number[] = [];
    for (let i = 3; i < words.length; i++) {
      if (words[i] === "to") toIndices.push(i);
    }

    if (toIndices.length < 2) return null;

    // Build hops
    const hops: Array<{ inputMint: string; outputMint: string }> = [];
    let currentInput = tokens.tokenA;

    for (const idx of toIndices) {
      const outputToken = words[idx + 1]?.toUpperCase() ?? "";
      if (!outputToken) continue;
      hops.push({
        inputMint: this.resolveMint(currentInput),
        outputMint: this.resolveMint(outputToken),
      });
      currentInput = outputToken;
    }

    if (hops.length < 2) return null;

    const slippageBps = tokens.extras["slippage"]
      ? parseInt(tokens.extras["slippage"], 10)
      : 50;

    return {
      hops,
      amount: tokens.amount,
      slippageBps,
    };
  }

  /**
   * Resolve a token symbol to its mint address.
   */
  private resolveMint(symbol: string): string {
    const upper = symbol.toUpperCase();
    return KNOWN_MINTS[upper] ?? symbol;
  }

  /**
   * Resolve an intent type string to the IntentType enum.
   */
  private resolveIntentType(typeStr: string): IntentType {
    const lower = typeStr.toLowerCase().replace(/[-_\s]/g, "");
    const mapping: Record<string, IntentType> = {
      swap: IntentType.Swap,
      multihopswap: IntentType.MultiHopSwap,
      multi_hop_swap: IntentType.MultiHopSwap,
      stake: IntentType.Stake,
      unstake: IntentType.Unstake,
      provideliquidity: IntentType.ProvideLiquidity,
      provide_liquidity: IntentType.ProvideLiquidity,
      addliquidity: IntentType.ProvideLiquidity,
      transfer: IntentType.Transfer,
      send: IntentType.Transfer,
    };
    const result = mapping[lower];
    if (!result) throw new Error(`Unknown intent type: '${typeStr}'`);
    return result;
  }

  /**
   * Build IntentParams from JSON input based on type.
   */
  private buildParamsFromJson(
    type: IntentType,
    raw: Record<string, unknown>
  ): IntentParams {
    switch (type) {
      case IntentType.Swap:
        return {
          inputMint: this.resolveMint(String(raw.inputMint ?? raw.from ?? "")),
          outputMint: this.resolveMint(String(raw.outputMint ?? raw.to ?? "")),
          amount: Number(raw.amount ?? 0),
          slippageBps: Number(raw.slippageBps ?? raw.slippage ?? 50),
          maxAccounts: raw.maxAccounts ? Number(raw.maxAccounts) : undefined,
        } as SwapParams;

      case IntentType.MultiHopSwap: {
        const hops = (raw.hops as Array<Record<string, string>>) ?? [];
        return {
          hops: hops.map((h) => ({
            inputMint: this.resolveMint(String(h.inputMint ?? h.from ?? "")),
            outputMint: this.resolveMint(String(h.outputMint ?? h.to ?? "")),
          })),
          amount: Number(raw.amount ?? 0),
          slippageBps: Number(raw.slippageBps ?? 50),
        } as MultiHopSwapParams;
      }

      case IntentType.Stake:
        return {
          amount: Number(raw.amount ?? 0),
          validatorVote: raw.validatorVote
            ? String(raw.validatorVote)
            : undefined,
        } as StakeParams;

      case IntentType.Unstake:
        return {
          amount: Number(raw.amount ?? 0),
          stakeAccount: raw.stakeAccount
            ? String(raw.stakeAccount)
            : undefined,
        } as UnstakeParams;

      case IntentType.ProvideLiquidity:
        return {
          tokenAMint: this.resolveMint(String(raw.tokenAMint ?? raw.tokenA ?? "")),
          tokenBMint: this.resolveMint(String(raw.tokenBMint ?? raw.tokenB ?? "")),
          amountA: Number(raw.amountA ?? 0),
          amountB: Number(raw.amountB ?? 0),
          poolAddress: raw.poolAddress ? String(raw.poolAddress) : undefined,
        } as ProvideLiquidityParams;

      case IntentType.Transfer:
        return {
          mint: this.resolveMint(String(raw.mint ?? raw.token ?? "")),
          amount: Number(raw.amount ?? 0),
          recipient: String(raw.recipient ?? raw.to ?? ""),
        } as TransferParams;

      default:
        throw new Error(`Cannot build params for type: ${type}`);
    }
  }

  /**
   * Generate a unique intent ID.
   */
  private generateId(): string {
    return `intent_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`;
  }
}

/**
 * Validates parsed intents for correctness.
 */
export class IntentValidator {
  /**
   * Validate an intent.
   */
  validate(intent: Intent): IntentValidationResult {
    const errors: IntentValidationError[] = [];
    const warnings: string[] = [];

    // Common validations
    if (!intent.id) {
      errors.push({ field: "id", message: "Intent ID is required" });
    }
    if (!intent.type) {
      errors.push({ field: "type", message: "Intent type is required" });
    }

    switch (intent.type) {
      case IntentType.Swap:
        this.validateSwap(intent.params as SwapParams, errors, warnings);
        break;
      case IntentType.MultiHopSwap:
        this.validateMultiHopSwap(
          intent.params as MultiHopSwapParams,
          errors,
          warnings
        );
        break;
      case IntentType.Stake:
        this.validateStake(intent.params as StakeParams, errors, warnings);
        break;
      case IntentType.Unstake:
        this.validateUnstake(intent.params as UnstakeParams, errors, warnings);
        break;
      case IntentType.ProvideLiquidity:
        this.validateLiquidity(
          intent.params as ProvideLiquidityParams,
          errors,
          warnings
        );
        break;
      case IntentType.Transfer:
        this.validateTransfer(
          intent.params as TransferParams,
          errors,
          warnings
        );
        break;
    }

    return {
      valid: errors.length === 0,
      errors,
      warnings,
    };
  }

  private validateSwap(
    params: SwapParams,
    errors: IntentValidationError[],
    warnings: string[]
  ): void {
    if (!params.inputMint) {
      errors.push({
        field: "inputMint",
        message: "Input mint is required",
      });
    }
    if (!params.outputMint) {
      errors.push({
        field: "outputMint",
        message: "Output mint is required",
      });
    }
    if (params.inputMint === params.outputMint) {
      errors.push({
        field: "outputMint",
        message: "Input and output mints must be different",
        value: params.outputMint,
      });
    }
    if (params.amount <= 0) {
      errors.push({
        field: "amount",
        message: "Amount must be greater than 0",
        value: params.amount,
      });
    }
    if (params.slippageBps < 0 || params.slippageBps > 10_000) {
      errors.push({
        field: "slippageBps",
        message: "Slippage must be between 0 and 10000 bps",
        value: params.slippageBps,
      });
    }
    if (params.slippageBps > 500) {
      warnings.push(
        `High slippage tolerance: ${params.slippageBps} bps (${params.slippageBps / 100}%)`
      );
    }
    this.validateMintAddress(params.inputMint, "inputMint", errors);
    this.validateMintAddress(params.outputMint, "outputMint", errors);
  }

  private validateMultiHopSwap(
    params: MultiHopSwapParams,
    errors: IntentValidationError[],
    warnings: string[]
  ): void {
    if (!params.hops || params.hops.length < 2) {
      errors.push({
        field: "hops",
        message: "Multi-hop swap requires at least 2 hops",
      });
    }
    if (params.amount <= 0) {
      errors.push({
        field: "amount",
        message: "Amount must be greater than 0",
        value: params.amount,
      });
    }
    // Validate hop chain continuity
    if (params.hops && params.hops.length >= 2) {
      for (let i = 0; i < params.hops.length - 1; i++) {
        if (params.hops[i].outputMint !== params.hops[i + 1].inputMint) {
          errors.push({
            field: `hops[${i}]`,
            message: `Hop chain broken: output of hop ${i} doesn't match input of hop ${i + 1}`,
          });
        }
      }
    }
    if (params.hops && params.hops.length > 4) {
      warnings.push(
        `${params.hops.length} hops may result in high slippage and fees`
      );
    }
  }

  private validateStake(
    params: StakeParams,
    errors: IntentValidationError[],
    _warnings: string[]
  ): void {
    if (params.amount <= 0) {
      errors.push({
        field: "amount",
        message: "Stake amount must be greater than 0",
        value: params.amount,
      });
    }
    if (params.amount < 0.01) {
      errors.push({
        field: "amount",
        message: "Minimum stake is 0.01 SOL",
        value: params.amount,
      });
    }
  }

  private validateUnstake(
    params: UnstakeParams,
    errors: IntentValidationError[],
    _warnings: string[]
  ): void {
    if (params.amount <= 0) {
      errors.push({
        field: "amount",
        message: "Unstake amount must be greater than 0",
        value: params.amount,
      });
    }
  }

  private validateLiquidity(
    params: ProvideLiquidityParams,
    errors: IntentValidationError[],
    _warnings: string[]
  ): void {
    if (!params.tokenAMint) {
      errors.push({ field: "tokenAMint", message: "Token A mint is required" });
    }
    if (!params.tokenBMint) {
      errors.push({ field: "tokenBMint", message: "Token B mint is required" });
    }
    if (params.amountA <= 0) {
      errors.push({
        field: "amountA",
        message: "Amount A must be greater than 0",
      });
    }
    if (params.amountB <= 0) {
      errors.push({
        field: "amountB",
        message: "Amount B must be greater than 0",
      });
    }
  }

  private validateTransfer(
    params: TransferParams,
    errors: IntentValidationError[],
    _warnings: string[]
  ): void {
    if (!params.mint) {
      errors.push({ field: "mint", message: "Token mint is required" });
    }
    if (params.amount <= 0) {
      errors.push({
        field: "amount",
        message: "Transfer amount must be greater than 0",
      });
    }
    if (!params.recipient) {
      errors.push({
        field: "recipient",
        message: "Recipient address is required",
      });
    }
    this.validateMintAddress(params.recipient, "recipient", errors);
  }

  /**
   * Basic validation that a string looks like a valid base58 Solana address.
   */
  private validateMintAddress(
    address: string,
    field: string,
    errors: IntentValidationError[]
  ): void {
    if (!address) return;
    // Base58 characters only, 32-44 chars
    if (!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(address)) {
      errors.push({
        field,
        message: `Invalid Solana address format: '${address}'`,
        value: address,
      });
    }
  }
}
