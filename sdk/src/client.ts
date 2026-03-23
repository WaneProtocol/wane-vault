import {
  type Address,
  type Hash,
  type Hex,
  type PublicClient,
  type WalletClient,
  encodeFunctionData,
  getAddress,
} from "viem";
import { factoryAbi, vaultAbi, erc20Abi } from "./abi.js";
import { ADDRESSES, reasonLabel } from "./constants.js";

export interface WaneVaultClientConfig {
  publicClient: PublicClient;
  /// Required only for state-changing calls (createVault, execute, withdraw).
  walletClient?: WalletClient;
  /// Override the factory address (defaults to the live Base mainnet factory).
  factory?: Address;
}

export interface ScreenResult {
  allowed: boolean;
  reason: number;
  label: string;
}

/// Thin viem client around the Wane vault factory and a per-owner vault. Every
/// outbound send routes through the vault's execute(), which screens against the
/// owner's policy before any value moves. Withdrawals return funds to the owner
/// and are intentionally unscreened.
export class WaneVaultClient {
  readonly publicClient: PublicClient;
  readonly walletClient?: WalletClient;
  readonly factory: Address;
