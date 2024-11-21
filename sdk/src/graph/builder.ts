import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { TransactionGraph } from "./index";
import { GraphNode } from "./node";
import {
  PriorityLevel,
  DependencyType,
  AccountAccess,
  KNOWN_MINTS,
} from "../types";

/**
 * Fluent API for building transaction graphs.
 *
 * Usage:
 *   const graph = new TransactionGraphBuilder()
 *     .swap()
 *       .from("USDC")
 *       .to("SOL")
 *       .amount(100)
 *     .then()
 *     .stake()
 *       .amount(1)
 *     .build();
 */
export class TransactionGraphBuilder {
  private graph: TransactionGraph;
  private currentNode: GraphNode | null = null;
  private nodeCounter = 0;
  private previousNodeId: string | null = null;
  private chainMode = false;

  constructor() {
    this.graph = new TransactionGraph();
  }

  /**
   * Start building a swap node.
   */
  swap(): SwapNodeBuilder {
    return new SwapNodeBuilder(this);
  }

  /**
   * Start building a stake node.
   */
  stake(): StakeNodeBuilder {
    return new StakeNodeBuilder(this);
  }

  /**
   * Start building an unstake node.
   */
  unstake(): UnstakeNodeBuilder {
    return new UnstakeNodeBuilder(this);
  }

  /**
   * Start building a transfer node.
   */
  transfer(): TransferNodeBuilder {
    return new TransferNodeBuilder(this);
  }

  /**
   * Start building a provide-liquidity node.
   */
  provideLiquidity(): LiquidityNodeBuilder {
    return new LiquidityNodeBuilder(this);
  }

  /**
   * Start building a custom instruction node.
   */
  custom(programId: PublicKey): CustomNodeBuilder {
    return new CustomNodeBuilder(this, programId);
  }

  /**
   * Chain the next node after the previous one (explicit dependency).
   */
  then(): this {
    this.chainMode = true;
    return this;
  }

  /**
   * Add a parallel group (nodes within have no dependencies on each other).
   */
  parallel(
    builderFn: (group: ParallelGroupBuilder) => void
  ): this {
    const group = new ParallelGroupBuilder(this);
    builderFn(group);
    group.finalize();
    return this;
  }

  /**
   * Internal: register a completed node.
   */
  _registerNode(node: GraphNode): void {
    this.graph.addNode(node);
    if (this.chainMode && this.previousNodeId) {
      this.graph.addEdge({
        from: this.previousNodeId,
        to: node.id,
        dependencyType: DependencyType.Explicit,
      });
      this.chainMode = false;
    }
    this.previousNodeId = node.id;
    this.currentNode = node;
  }

  /**
   * Internal: generate a unique node ID.
   */
  _nextId(prefix: string): string {
    return `${prefix}_${this.nodeCounter++}`;
  }

  /**
   * Internal: get the previous node ID for chaining.
   */
  _getPreviousNodeId(): string | null {
    return this.previousNodeId;
  }

  /**
   * Internal: get the underlying graph for parallel groups.
   */
  _getGraph(): TransactionGraph {
    return this.graph;
  }

  /**
   * Auto-detect dependencies by analyzing account overlaps.
   */
  autoDetectDependencies(): this {
    this.graph.autoDetectDependencies();
    return this;
  }

  /**
   * Add an explicit dependency edge.
   */
  addDependency(fromId: string, toId: string, type?: DependencyType): this {
    this.graph.addEdge({
      from: fromId,
      to: toId,
      dependencyType: type ?? DependencyType.Explicit,
    });
    return this;
  }

  /**
   * Build and return the transaction graph.
   */
  build(): TransactionGraph {
    return this.graph;
  }
}

/**
 * Builder for swap transaction nodes.
 */
class SwapNodeBuilder {
  private parent: TransactionGraphBuilder;
  private inputMint: string = "";
  private outputMint: string = "";
  private inputAmount: number = 0;
  private slippage: number = 50;
  private priorityLevel: PriorityLevel = PriorityLevel.Medium;
  private nodeLabel?: string;

  constructor(parent: TransactionGraphBuilder) {
    this.parent = parent;
  }

  from(mint: string): this {
    this.inputMint = KNOWN_MINTS[mint.toUpperCase()] ?? mint;
    return this;
  }

  to(mint: string): this {
    this.outputMint = KNOWN_MINTS[mint.toUpperCase()] ?? mint;
    return this;
  }

  amount(value: number): this {
    this.inputAmount = value;
    return this;
  }

  slippageBps(bps: number): this {
    this.slippage = bps;
    return this;
  }

  priority(level: PriorityLevel): this {
    this.priorityLevel = level;
    return this;
  }

  label(l: string): this {
    this.nodeLabel = l;
    return this;
  }

  then(): TransactionGraphBuilder {
    this.finalize();
    return this.parent.then();
  }

  build(): TransactionGraph {
    this.finalize();
    return this.parent.build();
  }

  private finalize(): void {
    const id = this.parent._nextId("swap");
    const programId = new PublicKey(
      "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" // Raydium AMM
    );
    const inputMintKey = new PublicKey(this.inputMint);
    const outputMintKey = new PublicKey(this.outputMint);

    const accesses: AccountAccess[] = [
      { pubkey: inputMintKey, isWritable: true, isSigner: false },
      { pubkey: outputMintKey, isWritable: true, isSigner: false },
    ];

    const node = new GraphNode({
      id,
      programId,
      accountAccesses: accesses,
      estimatedCu: 300_000,
      priority: this.priorityLevel,
      label: this.nodeLabel ?? `Swap ${this.inputAmount}`,
      metadata: {
        type: "swap",
        inputMint: this.inputMint,
        outputMint: this.outputMint,
        amount: this.inputAmount,
        slippageBps: this.slippage,
      },
    });

    this.parent._registerNode(node);
  }
}

/**
 * Builder for stake transaction nodes.
 */
class StakeNodeBuilder {
  private parent: TransactionGraphBuilder;
  private stakeAmount: number = 0;
  private validator?: string;
  private priorityLevel: PriorityLevel = PriorityLevel.Medium;

  constructor(parent: TransactionGraphBuilder) {
    this.parent = parent;
  }

  amount(value: number): this {
    this.stakeAmount = value;
    return this;
  }

  withValidator(vote: string): this {
    this.validator = vote;
    return this;
  }

  priority(level: PriorityLevel): this {
    this.priorityLevel = level;
    return this;
  }

  then(): TransactionGraphBuilder {
    this.finalize();
    return this.parent.then();
  }

  build(): TransactionGraph {
    this.finalize();
    return this.parent.build();
  }

  private finalize(): void {
    const id = this.parent._nextId("stake");
    const programId = new PublicKey("Stake11111111111111111111111111111111111111");

    const solMint = new PublicKey(KNOWN_MINTS["SOL"]);
    const accesses: AccountAccess[] = [
      { pubkey: solMint, isWritable: true, isSigner: false },
    ];

    const node = new GraphNode({
      id,
      programId,
      accountAccesses: accesses,
      estimatedCu: 150_000,
      priority: this.priorityLevel,
      label: `Stake ${this.stakeAmount} SOL`,
      metadata: {
        type: "stake",
        amount: this.stakeAmount,
        validator: this.validator,
      },
    });

    this.parent._registerNode(node);
  }
}

/**
 * Builder for unstake transaction nodes.
 */
class UnstakeNodeBuilder {
  private parent: TransactionGraphBuilder;
  private unstakeAmount: number = 0;
  private stakeAccount?: string;
  private priorityLevel: PriorityLevel = PriorityLevel.Medium;

  constructor(parent: TransactionGraphBuilder) {
    this.parent = parent;
  }

  amount(value: number): this {
    this.unstakeAmount = value;
    return this;
  }

  fromStakeAccount(account: string): this {
    this.stakeAccount = account;
    return this;
  }

  priority(level: PriorityLevel): this {
    this.priorityLevel = level;
    return this;
  }

  then(): TransactionGraphBuilder {
    this.finalize();
    return this.parent.then();
  }

  build(): TransactionGraph {
    this.finalize();
    return this.parent.build();
  }

  private finalize(): void {
    const id = this.parent._nextId("unstake");
    const programId = new PublicKey("Stake11111111111111111111111111111111111111");
    const solMint = new PublicKey(KNOWN_MINTS["SOL"]);

    const accesses: AccountAccess[] = [
      { pubkey: solMint, isWritable: true, isSigner: false },
    ];

    if (this.stakeAccount) {
      accesses.push({
        pubkey: new PublicKey(this.stakeAccount),
        isWritable: true,
        isSigner: false,
      });
    }

    const node = new GraphNode({
      id,
      programId,
      accountAccesses: accesses,
      estimatedCu: 150_000,
      priority: this.priorityLevel,
      label: `Unstake ${this.unstakeAmount} SOL`,
      metadata: {
        type: "unstake",
        amount: this.unstakeAmount,
        stakeAccount: this.stakeAccount,
      },
    });

    this.parent._registerNode(node);
  }
}

/**
 * Builder for transfer transaction nodes.
 */
class TransferNodeBuilder {
  private parent: TransactionGraphBuilder;
  private mint: string = KNOWN_MINTS["SOL"];
  private transferAmount: number = 0;
  private recipient: string = "";
  private priorityLevel: PriorityLevel = PriorityLevel.Medium;

  constructor(parent: TransactionGraphBuilder) {
    this.parent = parent;
  }

  token(mint: string): this {
    this.mint = KNOWN_MINTS[mint.toUpperCase()] ?? mint;
    return this;
  }

  amount(value: number): this {
    this.transferAmount = value;
    return this;
  }

  to(dest: string): this {
    this.recipient = dest;
    return this;
  }

  priority(level: PriorityLevel): this {
    this.priorityLevel = level;
    return this;
  }

  then(): TransactionGraphBuilder {
    this.finalize();
    return this.parent.then();
  }

  build(): TransactionGraph {
    this.finalize();
    return this.parent.build();
  }

  private finalize(): void {
    const id = this.parent._nextId("transfer");
    const programId = new PublicKey(
      "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
    );
    const mintKey = new PublicKey(this.mint);
    const recipientKey = new PublicKey(this.recipient);

    const accesses: AccountAccess[] = [
      { pubkey: mintKey, isWritable: true, isSigner: false },
      { pubkey: recipientKey, isWritable: true, isSigner: false },
    ];

    const node = new GraphNode({
      id,
      programId,
      accountAccesses: accesses,
      estimatedCu: 50_000,
      priority: this.priorityLevel,
      label: `Transfer ${this.transferAmount}`,
      metadata: {
        type: "transfer",
        mint: this.mint,
        amount: this.transferAmount,
        recipient: this.recipient,
      },
    });

    this.parent._registerNode(node);
  }
}

/**
 * Builder for provide-liquidity transaction nodes.
 */
class LiquidityNodeBuilder {
  private parent: TransactionGraphBuilder;
  private tokenAMint: string = "";
  private tokenBMint: string = "";
  private amountA: number = 0;
  private amountB: number = 0;
  private pool?: string;
  private priorityLevel: PriorityLevel = PriorityLevel.Medium;

  constructor(parent: TransactionGraphBuilder) {
    this.parent = parent;
  }

  tokenA(mint: string): this {
    this.tokenAMint = KNOWN_MINTS[mint.toUpperCase()] ?? mint;
    return this;
  }

  tokenB(mint: string): this {
    this.tokenBMint = KNOWN_MINTS[mint.toUpperCase()] ?? mint;
    return this;
  }

  amounts(a: number, b: number): this {
    this.amountA = a;
    this.amountB = b;
    return this;
  }

  poolAddress(addr: string): this {
    this.pool = addr;
    return this;
  }

  priority(level: PriorityLevel): this {
    this.priorityLevel = level;
    return this;
  }

  then(): TransactionGraphBuilder {
    this.finalize();
    return this.parent.then();
  }

  build(): TransactionGraph {
    this.finalize();
    return this.parent.build();
  }

  private finalize(): void {
    const id = this.parent._nextId("liquidity");
    const programId = new PublicKey(
      "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
    );
    const mintA = new PublicKey(this.tokenAMint);
    const mintB = new PublicKey(this.tokenBMint);

    const accesses: AccountAccess[] = [
      { pubkey: mintA, isWritable: true, isSigner: false },
      { pubkey: mintB, isWritable: true, isSigner: false },
    ];

    const node = new GraphNode({
      id,
      programId,
      accountAccesses: accesses,
      estimatedCu: 400_000,
      priority: this.priorityLevel,
      label: `AddLiquidity ${this.amountA}/${this.amountB}`,
      metadata: {
        type: "provide_liquidity",
        tokenAMint: this.tokenAMint,
        tokenBMint: this.tokenBMint,
        amountA: this.amountA,
        amountB: this.amountB,
        pool: this.pool,
      },
    });

    this.parent._registerNode(node);
  }
}

/**
 * Builder for custom instruction nodes.
 */
class CustomNodeBuilder {
  private parent: TransactionGraphBuilder;
  private programId: PublicKey;
  private ixs: TransactionInstruction[] = [];
  private cu: number = 200_000;
  private priorityLevel: PriorityLevel = PriorityLevel.Medium;
  private nodeLabel?: string;

  constructor(parent: TransactionGraphBuilder, programId: PublicKey) {
    this.parent = parent;
    this.programId = programId;
  }

  instruction(ix: TransactionInstruction): this {
    this.ixs.push(ix);
    return this;
  }

  estimatedCu(cu: number): this {
    this.cu = cu;
    return this;
  }

  priority(level: PriorityLevel): this {
    this.priorityLevel = level;
    return this;
  }

  label(l: string): this {
    this.nodeLabel = l;
    return this;
  }

  then(): TransactionGraphBuilder {
    this.finalize();
    return this.parent.then();
  }

  build(): TransactionGraph {
    this.finalize();
    return this.parent.build();
  }

  private finalize(): void {
    const id = this.parent._nextId("custom");
    const node = new GraphNode({
      id,
      programId: this.programId,
      instructions: this.ixs,
      estimatedCu: this.cu,
      priority: this.priorityLevel,
      label: this.nodeLabel,
    });

    // Auto-extract account accesses from instructions
    for (const ix of this.ixs) {
      node.addInstruction(ix);
    }
    // Clear duplicates from the double-add (constructor + addInstruction)
    node.instructions = this.ixs;

    this.parent._registerNode(node);
  }
}

/**
 * Builder for creating parallel groups of nodes.
 */
export class ParallelGroupBuilder {
  private parent: TransactionGraphBuilder;
  private nodeIds: string[] = [];

  constructor(parent: TransactionGraphBuilder) {
    this.parent = parent;
  }

  swap(): SwapNodeBuilder {
    return new SwapNodeBuilder(this.parent);
  }

  stake(): StakeNodeBuilder {
    return new StakeNodeBuilder(this.parent);
  }

  transfer(): TransferNodeBuilder {
    return new TransferNodeBuilder(this.parent);
  }

  custom(programId: PublicKey): CustomNodeBuilder {
    return new CustomNodeBuilder(this.parent, programId);
  }

  finalize(): void {
    // Nodes added by sub-builders are already registered in the graph.
    // Parallel group nodes have no edges between them by default.
  }
}
