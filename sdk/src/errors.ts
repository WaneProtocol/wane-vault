import { decodeErrorResult, type Hex } from "viem";
import { vaultAbi } from "./abi.js";
import { reasonLabel } from "./constants.js";

/// A decoded `Blocked(target, reason)` revert from the vault. Thrown so callers
/// can branch on which address tripped and why, rather than parsing raw revert
/// data themselves.
export class BlockedError extends Error {
  readonly target: `0x${string}`;
  readonly reason: number;
  readonly label: string;

  constructor(target: `0x${string}`, reason: number) {
    super(`vault blocked ${target}: ${reasonLabel(reason)} (reason ${reason})`);
    this.name = "BlockedError";
    this.target = target;
    this.reason = reason;
    this.label = reasonLabel(reason);
  }
}

/// Thrown when a non-owner tries to drive the vault.
export class NotOwnerError extends Error {
  constructor() {
    super("caller is not the vault owner");
    this.name = "NotOwnerError";
  }
}

/// Try to decode raw revert data into a typed vault error. Returns the decoded
/// error, or undefined if the data is not a known vault error. Use this to turn
/// a viem ContractFunctionRevertedError's `data` into a BlockedError.
export function decodeVaultError(
  data: Hex,
): BlockedError | NotOwnerError | undefined {
  let decoded: ReturnType<typeof decodeErrorResult>;
  try {
    decoded = decodeErrorResult({ abi: vaultAbi, data });
  } catch {
    return undefined;
  }

  if (decoded.errorName === "Blocked") {
    const [target, reason] = decoded.args as readonly [`0x${string}`, number];
    return new BlockedError(target, Number(reason));
  }
  if (decoded.errorName === "NotOwner") {
    return new NotOwnerError();
  }
  return undefined;
}
