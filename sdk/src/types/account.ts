import { PublicKey } from "@solana/web3.js";

/**
 * Describes how a transaction accesses a specific account.
 */
export interface AccountAccess {
  pubkey: PublicKey;
  isWritable: boolean;
  isSigner: boolean;
}

/**
 * Account lock state used during parallel scheduling.
 */
export enum AccountLockState {
  Unlocked = "unlocked",
  ReadLocked = "read_locked",
  WriteLocked = "write_locked",
}

/**
 * Tracks the lock state of an account across execution lanes.
 */
export interface AccountLockEntry {
  pubkey: PublicKey;
  state: AccountLockState;
  holdingLaneIndex: number;
  readCount: number;
}

/**
 * Manages account lock tracking for parallel execution scheduling.
 */
export class AccountLockManager {
  private locks: Map<string, AccountLockEntry> = new Map();

  /**
   * Attempt to acquire a read lock on an account.
   * Succeeds if the account is unlocked or already read-locked.
   */
  acquireRead(pubkey: PublicKey, laneIndex: number): boolean {
    const key = pubkey.toBase58();
    const existing = this.locks.get(key);
    if (!existing) {
      this.locks.set(key, {
        pubkey,
        state: AccountLockState.ReadLocked,
        holdingLaneIndex: laneIndex,
        readCount: 1,
      });
      return true;
    }
    if (existing.state === AccountLockState.ReadLocked) {
      existing.readCount++;
      return true;
    }
    return false;
  }

  /**
   * Attempt to acquire a write lock on an account.
   * Only succeeds if the account is currently unlocked.
   */
  acquireWrite(pubkey: PublicKey, laneIndex: number): boolean {
    const key = pubkey.toBase58();
    const existing = this.locks.get(key);
    if (!existing) {
      this.locks.set(key, {
        pubkey,
        state: AccountLockState.WriteLocked,
        holdingLaneIndex: laneIndex,
        readCount: 0,
      });
      return true;
    }
    return false;
  }

  /**
   * Release a lock on an account.
   */
  release(pubkey: PublicKey): void {
    const key = pubkey.toBase58();
    const existing = this.locks.get(key);
    if (!existing) return;

    if (existing.state === AccountLockState.ReadLocked) {
      existing.readCount--;
      if (existing.readCount <= 0) {
        this.locks.delete(key);
      }
    } else {
      this.locks.delete(key);
    }
  }

  /**
   * Release all locks held by a specific lane.
   */
  releaseAllForLane(laneIndex: number): void {
    for (const [key, entry] of this.locks.entries()) {
      if (entry.holdingLaneIndex === laneIndex) {
        this.locks.delete(key);
      }
    }
  }

  /**
   * Check if an account has any conflicting access with the given requirement.
   */
  hasConflict(pubkey: PublicKey, needsWrite: boolean): boolean {
    const key = pubkey.toBase58();
    const existing = this.locks.get(key);
    if (!existing) return false;
    if (needsWrite) return true;
    return existing.state === AccountLockState.WriteLocked;
  }

  /**
   * Check if a set of account accesses can all be acquired without conflict.
   */
  canAcquireAll(accesses: AccountAccess[]): boolean {
    for (const access of accesses) {
      if (this.hasConflict(access.pubkey, access.isWritable)) {
        return false;
      }
    }
    return true;
  }

  /**
   * Reset all locks.
   */
  clear(): void {
    this.locks.clear();
  }

  /**
   * Get number of active locks.
   */
  get size(): number {
    return this.locks.size;
  }

  /**
   * Get all currently locked account keys.
   */
  getLockedAccounts(): PublicKey[] {
    return Array.from(this.locks.values()).map((e) => e.pubkey);
  }
}
