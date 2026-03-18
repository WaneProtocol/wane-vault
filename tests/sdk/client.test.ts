/**
 * Comprehensive tests for the IVZA SDK IntentParser, IntentValidator,
 * AccountLockManager, and serialization utilities.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { PublicKey } from '@solana/web3.js';
import { IntentParser, IntentValidator } from '../../sdk/src/intent/parser';
import {
  IntentType,
  SwapParams,
  MultiHopSwapParams,
  StakeParams,
  TransferParams,
  ProvideLiquidityParams,
  UnstakeParams,
  KNOWN_MINTS,
  AccountLockManager,
  AccountLockState,
  DependencyType,
  PriorityLevel,
} from '../../sdk/src/types';
import { TransactionGraph, GraphNode } from '../../sdk/src/graph';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makePubkey(seed: number): PublicKey {
  const bytes = new Uint8Array(32).fill(seed);
  return new PublicKey(bytes);
}

// ---------------------------------------------------------------------------
// IntentParser: DSL parsing
// ---------------------------------------------------------------------------

describe('IntentParser - DSL', () => {
  let parser: IntentParser;

  beforeEach(() => {
    parser = new IntentParser();
  });

  it('parses "swap 100 USDC to SOL"', () => {
    const intent = parser.parse('swap 100 USDC to SOL');
    expect(intent.type).toBe(IntentType.Swap);
    const params = intent.params as SwapParams;
    expect(params.amount).toBe(100);
    expect(params.inputMint).toBe(KNOWN_MINTS['USDC']);
    expect(params.outputMint).toBe(KNOWN_MINTS['SOL']);
  });

  it('parses swap with slippage', () => {
    const intent = parser.parse('swap 50 SOL to USDC slippage 100');
    const params = intent.params as SwapParams;
    expect(params.slippageBps).toBe(100);
  });

  it('parses multi-hop swap "swap 100 USDC to RAY to SOL"', () => {
    const intent = parser.parse('swap 100 USDC to RAY to SOL');
    expect(intent.type).toBe(IntentType.MultiHopSwap);
    const params = intent.params as MultiHopSwapParams;
    expect(params.hops.length).toBe(2);
    expect(params.hops[0].inputMint).toBe(KNOWN_MINTS['USDC']);
    expect(params.hops[0].outputMint).toBe(KNOWN_MINTS['RAY']);
    expect(params.hops[1].inputMint).toBe(KNOWN_MINTS['RAY']);
    expect(params.hops[1].outputMint).toBe(KNOWN_MINTS['SOL']);
  });

  it('parses "stake 10 SOL"', () => {
    const intent = parser.parse('stake 10 SOL');
    expect(intent.type).toBe(IntentType.Stake);
    const params = intent.params as StakeParams;
    expect(params.amount).toBe(10);
  });

  it('parses "unstake 5 SOL"', () => {
    const intent = parser.parse('unstake 5 SOL');
    expect(intent.type).toBe(IntentType.Unstake);
    const params = intent.params as UnstakeParams;
    expect(params.amount).toBe(5);
  });

  it('parses "transfer 50 USDC to <pubkey>"', () => {
    const recipient = '11111111111111111111111111111111';
    const intent = parser.parse(`transfer 50 USDC to ${recipient}`);
    expect(intent.type).toBe(IntentType.Transfer);
    const params = intent.params as TransferParams;
    expect(params.amount).toBe(50);
    expect(params.mint).toBe(KNOWN_MINTS['USDC']);
  });

  it('accepts verb aliases: exchange, trade, convert', () => {
    for (const verb of ['exchange', 'trade', 'convert']) {
      const intent = parser.parse(`${verb} 100 USDC to SOL`);
      expect(intent.type).toBe(IntentType.Swap);
    }
  });

  it('accepts stake alias: delegate', () => {
    const intent = parser.parse('delegate 5 SOL');
    expect(intent.type).toBe(IntentType.Stake);
  });

  it('accepts transfer alias: send', () => {
    const intent = parser.parse('send 10 SOL to 11111111111111111111111111111111');
    expect(intent.type).toBe(IntentType.Transfer);
  });

  it('throws on empty string', () => {
    expect(() => parser.parse('')).toThrow();
  });

  it('throws on unknown verb', () => {
    expect(() => parser.parse('foobar 100 USDC')).toThrow();
  });

  it('throws on invalid amount', () => {
    expect(() => parser.parse('swap abc USDC to SOL')).toThrow();
  });

  it('generates unique intent IDs', () => {
    const a = parser.parse('swap 100 USDC to SOL');
    const b = parser.parse('swap 200 USDC to SOL');
    expect(a.id).not.toBe(b.id);
  });

  it('sets createdAt timestamp', () => {
    const before = Date.now();
    const intent = parser.parse('swap 100 USDC to SOL');
    const after = Date.now();
    expect(intent.createdAt).toBeGreaterThanOrEqual(before);
    expect(intent.createdAt).toBeLessThanOrEqual(after);
  });
});

// ---------------------------------------------------------------------------
// IntentParser: JSON parsing
// ---------------------------------------------------------------------------

describe('IntentParser - JSON', () => {
  let parser: IntentParser;

  beforeEach(() => {
    parser = new IntentParser();
  });

  it('parses JSON swap intent', () => {
    const intent = parser.parse({
      type: 'swap',
      params: {
        inputMint: 'USDC',
        outputMint: 'SOL',
        amount: 100,
        slippageBps: 50,
      },
    });

    expect(intent.type).toBe(IntentType.Swap);
    const params = intent.params as SwapParams;
    expect(params.inputMint).toBe(KNOWN_MINTS['USDC']);
    expect(params.amount).toBe(100);
  });

  it('parses JSON stake intent', () => {
    const intent = parser.parse({
      type: 'stake',
      params: { amount: 10 },
    });
    expect(intent.type).toBe(IntentType.Stake);
  });

  it('parses JSON transfer intent', () => {
    const intent = parser.parse({
      type: 'transfer',
      params: {
        mint: 'SOL',
        amount: 5,
        recipient: '11111111111111111111111111111111',
      },
    });
    expect(intent.type).toBe(IntentType.Transfer);
  });

  it('parses JSON multi-hop intent', () => {
    const intent = parser.parse({
      type: 'multi_hop_swap',
      params: {
        hops: [
          { inputMint: 'USDC', outputMint: 'RAY' },
          { inputMint: 'RAY', outputMint: 'SOL' },
        ],
        amount: 100,
        slippageBps: 50,
      },
    });
    expect(intent.type).toBe(IntentType.MultiHopSwap);
    const params = intent.params as MultiHopSwapParams;
    expect(params.hops.length).toBe(2);
  });

  it('parses JSON provide_liquidity intent', () => {
    const intent = parser.parse({
      type: 'provide_liquidity',
      params: {
        tokenAMint: 'USDC',
        tokenBMint: 'SOL',
        amountA: 100,
        amountB: 0.5,
      },
    });
    expect(intent.type).toBe(IntentType.ProvideLiquidity);
  });

  it('accepts type alias: "send" maps to Transfer', () => {
    const intent = parser.parse({
      type: 'send',
      params: { mint: 'SOL', amount: 1, recipient: '1111111111111111111111111111111' },
    });
    expect(intent.type).toBe(IntentType.Transfer);
  });

  it('throws on unknown type', () => {
    expect(() =>
      parser.parse({ type: 'unknown_intent', params: {} })
    ).toThrow();
  });

  it('respects priority field', () => {
    const intent = parser.parse({
      type: 'swap',
      params: { inputMint: 'USDC', outputMint: 'SOL', amount: 100 },
      priority: 5,
    });
    expect(intent.priority).toBe(5);
  });

  it('uses "from" and "to" aliases for swap params', () => {
    const intent = parser.parse({
      type: 'swap',
      params: { from: 'USDC', to: 'SOL', amount: 100 },
    });
    const params = intent.params as SwapParams;
    expect(params.inputMint).toBe(KNOWN_MINTS['USDC']);
    expect(params.outputMint).toBe(KNOWN_MINTS['SOL']);
  });
});

// ---------------------------------------------------------------------------
// IntentValidator
// ---------------------------------------------------------------------------

describe('IntentValidator', () => {
  let parser: IntentParser;
  let validator: IntentValidator;

  beforeEach(() => {
    parser = new IntentParser();
    validator = new IntentValidator();
  });

  it('validates a correct swap intent', () => {
    const intent = parser.parse('swap 100 USDC to SOL');
    const result = validator.validate(intent);
    expect(result.valid).toBe(true);
    expect(result.errors.length).toBe(0);
  });

  it('rejects swap with zero amount', () => {
    const intent = parser.parse({
      type: 'swap',
      params: { inputMint: 'USDC', outputMint: 'SOL', amount: 0, slippageBps: 50 },
    });
    const result = validator.validate(intent);
    expect(result.valid).toBe(false);
    expect(result.errors.some((e) => e.field === 'amount')).toBe(true);
  });

  it('rejects swap with same input and output mint', () => {
    const intent = parser.parse({
      type: 'swap',
      params: {
        inputMint: KNOWN_MINTS['USDC'],
        outputMint: KNOWN_MINTS['USDC'],
        amount: 100,
        slippageBps: 50,
      },
    });
    const result = validator.validate(intent);
    expect(result.valid).toBe(false);
  });

  it('warns on high slippage', () => {
    const intent = parser.parse({
      type: 'swap',
      params: {
        inputMint: 'USDC',
        outputMint: 'SOL',
        amount: 100,
        slippageBps: 1000,
      },
    });
    const result = validator.validate(intent);
    expect(result.warnings.length).toBeGreaterThan(0);
  });

  it('validates multi-hop continuity', () => {
    const intent = parser.parse({
      type: 'multi_hop_swap',
      params: {
        hops: [
          { inputMint: 'USDC', outputMint: 'RAY' },
          { inputMint: 'SOL', outputMint: 'BONK' }, // broken chain
        ],
        amount: 100,
        slippageBps: 50,
      },
    });
    const result = validator.validate(intent);
    expect(result.valid).toBe(false);
    expect(result.errors.some((e) => e.message.includes('chain broken'))).toBe(true);
  });

  it('validates stake minimum', () => {
    const intent = parser.parse({
      type: 'stake',
      params: { amount: 0.001 },
    });
    const result = validator.validate(intent);
    expect(result.valid).toBe(false);
  });

  it('validates transfer requires recipient', () => {
    const intent = parser.parse({
      type: 'transfer',
      params: { mint: 'SOL', amount: 1, recipient: '' },
    });
    const result = validator.validate(intent);
    expect(result.valid).toBe(false);
    expect(result.errors.some((e) => e.field === 'recipient')).toBe(true);
  });

  it('validates provide-liquidity amounts', () => {
    const intent = parser.parse({
      type: 'provide_liquidity',
      params: {
        tokenAMint: 'USDC',
        tokenBMint: 'SOL',
        amountA: 0,
        amountB: 0,
      },
    });
    const result = validator.validate(intent);
    expect(result.valid).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// AccountLockManager
// ---------------------------------------------------------------------------

describe('AccountLockManager', () => {
  let manager: AccountLockManager;
  let account1: PublicKey;
  let account2: PublicKey;

  beforeEach(() => {
    manager = new AccountLockManager();
    account1 = makePubkey(1);
    account2 = makePubkey(2);
  });

  it('starts empty', () => {
    expect(manager.size).toBe(0);
  });

  it('acquires read lock on unlocked account', () => {
    expect(manager.acquireRead(account1, 0)).toBe(true);
    expect(manager.size).toBe(1);
  });

  it('acquires write lock on unlocked account', () => {
    expect(manager.acquireWrite(account1, 0)).toBe(true);
    expect(manager.size).toBe(1);
  });

  it('allows multiple read locks', () => {
    expect(manager.acquireRead(account1, 0)).toBe(true);
    expect(manager.acquireRead(account1, 1)).toBe(true);
    expect(manager.size).toBe(1); // Same account, one entry
  });

  it('rejects write lock when read-locked', () => {
    manager.acquireRead(account1, 0);
    expect(manager.acquireWrite(account1, 1)).toBe(false);
  });

  it('rejects read lock when write-locked', () => {
    manager.acquireWrite(account1, 0);
    expect(manager.acquireRead(account1, 1)).toBe(false);
  });

  it('rejects write lock when write-locked', () => {
    manager.acquireWrite(account1, 0);
    expect(manager.acquireWrite(account1, 1)).toBe(false);
  });

  it('release allows re-acquisition', () => {
    manager.acquireWrite(account1, 0);
    manager.release(account1);
    expect(manager.acquireRead(account1, 1)).toBe(true);
  });

  it('release read decrements count', () => {
    manager.acquireRead(account1, 0);
    manager.acquireRead(account1, 1);
    manager.release(account1);
    // Still one read lock
    expect(manager.size).toBe(1);
    manager.release(account1);
    expect(manager.size).toBe(0);
  });

  it('releaseAllForLane clears correct lane', () => {
    manager.acquireWrite(account1, 0);
    manager.acquireWrite(account2, 1);
    manager.releaseAllForLane(0);
    expect(manager.size).toBe(1); // Only account2 remains
  });

  it('hasConflict detects write conflict', () => {
    manager.acquireWrite(account1, 0);
    expect(manager.hasConflict(account1, false)).toBe(true); // read vs write
    expect(manager.hasConflict(account1, true)).toBe(true); // write vs write
  });

  it('hasConflict allows read on read-locked', () => {
    manager.acquireRead(account1, 0);
    expect(manager.hasConflict(account1, false)).toBe(false); // read vs read = ok
    expect(manager.hasConflict(account1, true)).toBe(true); // write vs read = conflict
  });

  it('canAcquireAll checks multiple accounts', () => {
    manager.acquireWrite(account1, 0);
    const accesses = [
      { pubkey: account1, isWritable: false, isSigner: false },
      { pubkey: account2, isWritable: true, isSigner: false },
    ];
    expect(manager.canAcquireAll(accesses)).toBe(false); // account1 has write lock
  });

  it('canAcquireAll succeeds when no conflicts', () => {
    const accesses = [
      { pubkey: account1, isWritable: true, isSigner: false },
      { pubkey: account2, isWritable: false, isSigner: false },
    ];
    expect(manager.canAcquireAll(accesses)).toBe(true);
  });

  it('clear removes all locks', () => {
    manager.acquireWrite(account1, 0);
    manager.acquireRead(account2, 1);
    manager.clear();
    expect(manager.size).toBe(0);
  });

  it('getLockedAccounts returns all locked pubkeys', () => {
    manager.acquireWrite(account1, 0);
    manager.acquireRead(account2, 1);
    const locked = manager.getLockedAccounts();
    expect(locked.length).toBe(2);
  });
});

// ---------------------------------------------------------------------------
// Graph serialization round-trip (integration)
// ---------------------------------------------------------------------------

describe('Graph Serialization Integration', () => {
  it('serialized graph preserves structure through JSON', () => {
    const graph = new TransactionGraph('test');
    const a = new GraphNode({
      id: 'a',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: makePubkey(1), isWritable: true, isSigner: false }],
      estimatedCu: 100_000,
      priority: PriorityLevel.High,
    });
    const b = new GraphNode({
      id: 'b',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: makePubkey(2), isWritable: false, isSigner: false }],
      estimatedCu: 200_000,
      priority: PriorityLevel.Low,
    });
    graph.addNode(a);
    graph.addNode(b);
    graph.addEdge({ from: 'a', to: 'b', dependencyType: DependencyType.ReadAfterWrite });

    // Serialize -> JSON string -> parse -> deserialize
    const serialized = graph.serialize();
    const jsonStr = JSON.stringify(serialized);
    const parsed = JSON.parse(jsonStr);
    const restored = TransactionGraph.deserialize(parsed);

    expect(restored.nodeCount).toBe(2);
    expect(restored.edgeCount).toBe(1);

    // Topological sort should still work
    const sorted = restored.topologicalSort();
    expect(sorted[0].id).toBe('a');
    expect(sorted[1].id).toBe('b');
  });
});

// ---------------------------------------------------------------------------
// AccountSet conflict detection (via GraphNode)
// ---------------------------------------------------------------------------

describe('AccountSet Conflict Detection via GraphNode', () => {
  it('no conflict between nodes with disjoint accounts', () => {
    const nodeA = new GraphNode({
      id: 'a',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: makePubkey(1), isWritable: true, isSigner: false }],
    });
    const nodeB = new GraphNode({
      id: 'b',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: makePubkey(2), isWritable: true, isSigner: false }],
    });
    expect(nodeA.conflictsWith(nodeB)).toBe(false);
  });

  it('write-write conflict on shared account', () => {
    const shared = makePubkey(1);
    const nodeA = new GraphNode({
      id: 'a',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: shared, isWritable: true, isSigner: false }],
    });
    const nodeB = new GraphNode({
      id: 'b',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: shared, isWritable: true, isSigner: false }],
    });
    expect(nodeA.conflictsWith(nodeB)).toBe(true);
    expect(nodeA.getDependencyWith(nodeB)).toBe(DependencyType.WriteAfterWrite);
  });

  it('read-write conflict on shared account', () => {
    const shared = makePubkey(1);
    const nodeA = new GraphNode({
      id: 'a',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: shared, isWritable: true, isSigner: false }],
    });
    const nodeB = new GraphNode({
      id: 'b',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: shared, isWritable: false, isSigner: false }],
    });
    expect(nodeA.conflictsWith(nodeB)).toBe(true);
  });

  it('read-read is not a conflict', () => {
    const shared = makePubkey(1);
    const nodeA = new GraphNode({
      id: 'a',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: shared, isWritable: false, isSigner: false }],
    });
    const nodeB = new GraphNode({
      id: 'b',
      programId: makePubkey(0),
      accountAccesses: [{ pubkey: shared, isWritable: false, isSigner: false }],
    });
    expect(nodeA.conflictsWith(nodeB)).toBe(false);
  });

  it('multi-account conflict detection', () => {
    const acc1 = makePubkey(1);
    const acc2 = makePubkey(2);
    const acc3 = makePubkey(3);

    const nodeA = new GraphNode({
      id: 'a',
      programId: makePubkey(0),
      accountAccesses: [
        { pubkey: acc1, isWritable: true, isSigner: false },
        { pubkey: acc2, isWritable: false, isSigner: false },
      ],
    });
    const nodeB = new GraphNode({
      id: 'b',
      programId: makePubkey(0),
      accountAccesses: [
        { pubkey: acc2, isWritable: true, isSigner: false },
        { pubkey: acc3, isWritable: true, isSigner: false },
      ],
    });
    // nodeA reads acc2, nodeB writes acc2 -> write-after-read
    expect(nodeA.conflictsWith(nodeB)).toBe(true);
  });
});
