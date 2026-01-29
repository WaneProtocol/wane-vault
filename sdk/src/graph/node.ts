import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import {
  TransactionNode,
  AccountAccess,
  PriorityLevel,
  DependencyType,
} from "../types";

/**
 * Wrapper class around TransactionNode providing helper methods
 * for construction, inspection, and conflict detection.
 */
export class GraphNode implements TransactionNode {
  id: string;
  programId: PublicKey;
  instructions: TransactionInstruction[];
  accountAccesses: AccountAccess[];
  estimatedCu: number;
  priority: PriorityLevel;
  label?: string;
  metadata?: Record<string, unknown>;

  constructor(params: {
    id: string;
    programId: PublicKey;
    instructions?: TransactionInstruction[];
    accountAccesses?: AccountAccess[];
    estimatedCu?: number;
    priority?: PriorityLevel;
    label?: string;
    metadata?: Record<string, unknown>;
  }) {
    this.id = params.id;
    this.programId = params.programId;
    this.instructions = params.instructions ?? [];
    this.accountAccesses = params.accountAccesses ?? [];
    this.estimatedCu = params.estimatedCu ?? 200_000;
    this.priority = params.priority ?? PriorityLevel.Medium;
    this.label = params.label;
    this.metadata = params.metadata;
  }

  /**
   * Add an instruction to this node.
   */
  addInstruction(ix: TransactionInstruction): this {
    this.instructions.push(ix);
    // Auto-populate account accesses from instruction keys
    for (const key of ix.keys) {
      const exists = this.accountAccesses.some(
        (a) => a.pubkey.equals(key.pubkey)
      );
      if (!exists) {
        this.accountAccesses.push({
          pubkey: key.pubkey,
          isWritable: key.isWritable,
          isSigner: key.isSigner,
        });
      } else {
        // Upgrade to writable if needed
        const existing = this.accountAccesses.find((a) =>
          a.pubkey.equals(key.pubkey)
        )!;
        if (key.isWritable && !existing.isWritable) {
          existing.isWritable = true;
        }
        if (key.isSigner && !existing.isSigner) {
          existing.isSigner = true;
        }
      }
    }
    return this;
  }

  /**
   * Get all accounts that this node writes to.
   */
  getWritableAccounts(): PublicKey[] {
    return this.accountAccesses
      .filter((a) => a.isWritable)
      .map((a) => a.pubkey);
  }

  /**
   * Get all accounts that this node reads (but does not write).
   */
  getReadOnlyAccounts(): PublicKey[] {
    return this.accountAccesses
      .filter((a) => !a.isWritable)
      .map((a) => a.pubkey);
  }

  /**
   * Get all signer accounts.
   */
  getSignerAccounts(): PublicKey[] {
    return this.accountAccesses
      .filter((a) => a.isSigner)
      .map((a) => a.pubkey);
  }

  /**
   * Determine the dependency type between this node and another.
   * Returns null if there is no dependency.
   */
  getDependencyWith(other: GraphNode): DependencyType | null {
    const thisWritable = new Set(
      this.getWritableAccounts().map((p) => p.toBase58())
    );
    const thisReadOnly = new Set(
      this.getReadOnlyAccounts().map((p) => p.toBase58())
    );
    const otherWritable = new Set(
      other.getWritableAccounts().map((p) => p.toBase58())
    );
    const otherReadOnly = new Set(
      other.getReadOnlyAccounts().map((p) => p.toBase58())
    );

    // Write-after-write: both write to same account
    for (const acc of thisWritable) {
      if (otherWritable.has(acc)) return DependencyType.WriteAfterWrite;
    }

    // Read-after-write: other reads what this writes
    for (const acc of thisWritable) {
      if (otherReadOnly.has(acc)) return DependencyType.ReadAfterWrite;
    }

    // Write-after-read: other writes what this reads
    for (const acc of thisReadOnly) {
      if (otherWritable.has(acc)) return DependencyType.WriteAfterRead;
    }

    return null;
  }

  /**
   * Check if this node conflicts with another (shares any writable account).
   */
  conflictsWith(other: GraphNode): boolean {
    return this.getDependencyWith(other) !== null;
  }

  /**
   * Get the total number of unique accounts accessed.
   */
  getAccountCount(): number {
    return this.accountAccesses.length;
  }

  /**
   * Create a shallow clone of this node with a new ID.
   */
  clone(newId?: string): GraphNode {
    return new GraphNode({
      id: newId ?? `${this.id}_clone`,
      programId: this.programId,
      instructions: [...this.instructions],
      accountAccesses: this.accountAccesses.map((a) => ({ ...a })),
      estimatedCu: this.estimatedCu,
      priority: this.priority,
      label: this.label,
      metadata: this.metadata ? { ...this.metadata } : undefined,
    });
  }

  /**
   * Convert to a plain TransactionNode object.
   */
  toPlain(): TransactionNode {
    return {
      id: this.id,
      programId: this.programId,
      instructions: this.instructions,
      accountAccesses: this.accountAccesses,
      estimatedCu: this.estimatedCu,
      priority: this.priority,
      label: this.label,
      metadata: this.metadata,
    };
  }

  /**
   * Create a GraphNode from a plain TransactionNode.
   */
  static fromPlain(node: TransactionNode): GraphNode {
    return new GraphNode({
      id: node.id,
      programId: node.programId,
      instructions: node.instructions,
      accountAccesses: node.accountAccesses,
      estimatedCu: node.estimatedCu,
      priority: node.priority,
      label: node.label,
      metadata: node.metadata,
    });
  }
}
