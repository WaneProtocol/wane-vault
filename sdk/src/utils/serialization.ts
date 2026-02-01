import { PublicKey } from "@solana/web3.js";
import {
  Intent,
  IntentType,
  IntentParams,
  KNOWN_MINTS,
} from "../types";
import { TransactionGraph } from "../graph";
import { SerializedGraph, SerializedNode, SerializedEdge } from "../graph/types";

/**
 * Serialized form of an Intent for wire transfer / storage.
 */
export interface SerializedIntent {
  id: string;
  type: string;
  params: Record<string, unknown>;
  priority: number;
  createdAt: number;
}

/**
 * Serialize a TransactionGraph to a JSON-compatible object.
 */
export function serializeGraph(graph: TransactionGraph): SerializedGraph {
  return graph.serialize();
}

/**
 * Deserialize a TransactionGraph from a plain object.
 */
export function deserializeGraph(data: SerializedGraph): TransactionGraph {
  return TransactionGraph.deserialize(data);
}

/**
 * Serialize a TransactionGraph to a JSON string.
 */
export function serializeGraphToString(graph: TransactionGraph): string {
  return JSON.stringify(serializeGraph(graph));
}

/**
 * Deserialize a TransactionGraph from a JSON string.
 */
export function deserializeGraphFromString(json: string): TransactionGraph {
  const data = JSON.parse(json) as SerializedGraph;
  return deserializeGraph(data);
}

/**
 * Serialize an Intent to a plain object.
 */
export function serializeIntent(intent: Intent): SerializedIntent {
  return {
    id: intent.id,
    type: intent.type,
    params: intent.params as unknown as Record<string, unknown>,
    priority: intent.priority,
    createdAt: intent.createdAt,
  };
}

/**
 * Deserialize an Intent from a plain object.
 */
export function deserializeIntent(data: SerializedIntent): Intent {
  return {
    id: data.id,
    type: data.type as IntentType,
    params: data.params as unknown as IntentParams,
    priority: data.priority,
    createdAt: data.createdAt,
  };
}

/**
 * Serialize an array of intents to a JSON string.
 */
export function serializeIntents(intents: Intent[]): string {
  return JSON.stringify(intents.map(serializeIntent));
}

/**
 * Deserialize an array of intents from a JSON string.
 */
export function deserializeIntents(json: string): Intent[] {
  const data = JSON.parse(json) as SerializedIntent[];
  return data.map(deserializeIntent);
}

/**
 * Encode bytes to base64.
 */
export function encodeBase64(data: Uint8Array): string {
  return Buffer.from(data).toString("base64");
}

/**
 * Decode base64 to bytes.
 */
export function decodeBase64(str: string): Uint8Array {
  return new Uint8Array(Buffer.from(str, "base64"));
}

/**
 * Encode bytes to base58.
 */
export function encodeBase58(data: Uint8Array): string {
  const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let num = BigInt(0);
  for (const byte of data) {
    num = num * BigInt(256) + BigInt(byte);
  }

  let result = "";
  while (num > BigInt(0)) {
    const remainder = Number(num % BigInt(58));
    num = num / BigInt(58);
    result = ALPHABET[remainder] + result;
  }

  // Preserve leading zeros
  for (const byte of data) {
    if (byte === 0) {
      result = "1" + result;
    } else {
      break;
    }
  }

  return result || "1";
}

/**
 * Decode base58 to bytes.
 */
export function decodeBase58(str: string): Uint8Array {
  const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let num = BigInt(0);

  for (const char of str) {
    const index = ALPHABET.indexOf(char);
    if (index === -1) {
      throw new Error(`Invalid base58 character: ${char}`);
    }
    num = num * BigInt(58) + BigInt(index);
  }

  // Convert bigint to bytes
  const bytes: number[] = [];
  while (num > BigInt(0)) {
    bytes.unshift(Number(num & BigInt(0xff)));
    num = num >> BigInt(8);
  }

  // Preserve leading zeros (represented as '1' in base58)
  for (const char of str) {
    if (char === "1") {
      bytes.unshift(0);
    } else {
      break;
    }
  }

  return new Uint8Array(bytes);
}

/**
 * Derive a Program Derived Address (PDA).
 */
export function derivePDA(
  seeds: Array<Buffer | Uint8Array>,
  programId: PublicKey
): { address: PublicKey; bump: number } {
  const [address, bump] = PublicKey.findProgramAddressSync(seeds, programId);
  return { address, bump };
}

/**
 * Derive an Associated Token Account (ATA) address.
 */
export function deriveATA(
  owner: PublicKey,
  mint: PublicKey
): PublicKey {
  const TOKEN_PROGRAM_ID = new PublicKey(
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
  );
  const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
  );

  const [ata] = PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID
  );

  return ata;
}

/**
 * Resolve a token symbol to its mint address PublicKey.
 * Returns the input as-is if it's already a valid address.
 */
export function resolveTokenMint(symbolOrAddress: string): PublicKey {
  const upper = symbolOrAddress.toUpperCase();
  const address = KNOWN_MINTS[upper] ?? symbolOrAddress;
  return new PublicKey(address);
}

/**
 * Reverse-lookup: get the token symbol for a known mint address.
 * Returns null if not a known mint.
 */
export function getTokenSymbol(mintAddress: string): string | null {
  for (const [symbol, address] of Object.entries(KNOWN_MINTS)) {
    if (address === mintAddress) {
      return symbol;
    }
  }
  return null;
}

/**
 * Hash a set of bytes using a simple non-cryptographic hash.
 * Useful for graph fingerprinting.
 */
export function hashBytes(data: Uint8Array): string {
  let h1 = 0xdeadbeef;
  let h2 = 0x41c6ce57;

  for (let i = 0; i < data.length; i++) {
    const ch = data[i];
    h1 = Math.imul(h1 ^ ch, 2654435761);
    h2 = Math.imul(h2 ^ ch, 1597334677);
  }

  h1 = Math.imul(h1 ^ (h1 >>> 16), 2246822507);
  h1 ^= Math.imul(h2 ^ (h2 >>> 13), 3266489909);
  h2 = Math.imul(h2 ^ (h2 >>> 16), 2246822507);
  h2 ^= Math.imul(h1 ^ (h1 >>> 13), 3266489909);

  const combined = 4294967296 * (2097151 & h2) + (h1 >>> 0);
  return combined.toString(36);
}

/**
 * Create a fingerprint for a graph based on its structure.
 */
export function fingerprintGraph(graph: TransactionGraph): string {
  const serialized = serializeGraphToString(graph);
  const bytes = new TextEncoder().encode(serialized);
  return hashBytes(bytes);
}
