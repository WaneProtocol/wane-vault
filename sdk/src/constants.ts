import type { Address } from "viem";

/// Live Base mainnet (chain 8453) deployment. The factory mints per-owner vaults
/// that reuse the already-deployed policy + antibody registry below.
export const BASE_MAINNET_CHAIN_ID = 8453 as const;

export const ADDRESSES = {
  vaultFactory: "0x6640dd13F172c356f671d35ef76695792908e2a9",
  policy: "0x26deE4503C7f67356837ED41cE285026EF256667",
  registry: "0x027F371fB139A57EcD2A2E175d30157eEA1C56de",
} as const satisfies Record<string, Address>;

/// WanePolicy reason codes returned by evaluate / wouldAllow. A blocked action
/// reverts with Blocked(target, reason); a dry-run returns (false, reason).
export const REASON = {
  OK: 0,
  BLOCKLIST: 1,
  ANTIBODY: 2,
  PER_TX_CAP: 3,
  DAILY_CAP: 4,
  PAUSED: 5,
  GLOBAL_DENY: 6,
  EXPIRED: 7,
  SELECTOR: 8,
  TOKEN: 9,
} as const;

export type ReasonCode = (typeof REASON)[keyof typeof REASON];

const REASON_LABEL: Record<number, string> = {
  0: "ok",
  1: "owner blocklist",
  2: "antibody match",
  3: "per-tx cap exceeded",
  4: "daily cap exceeded",
  5: "paused (agent or global kill switch)",
  6: "global recipient denylist",
  7: "policy expired",
  8: "selector not allowed",
  9: "token not allowed",
};

/// Human-readable label for a reason code.
export function reasonLabel(reason: number): string {
  return REASON_LABEL[reason] ?? `unknown (${reason})`;
}
