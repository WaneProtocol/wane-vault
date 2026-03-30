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

  constructor(config: WaneVaultClientConfig) {
    this.publicClient = config.publicClient;
    this.walletClient = config.walletClient;
    this.factory = getAddress(config.factory ?? ADDRESSES.vaultFactory);
  }

  private requireWallet(): WalletClient {
    if (!this.walletClient) {
      throw new Error("walletClient is required for state-changing calls");
    }
    return this.walletClient;
  }

  private requireAccount(wallet: WalletClient): Address {
    const account = wallet.account?.address;
    if (!account) throw new Error("walletClient has no account");
    return account;
  }

  /* factory: predict + create */

  /// The deterministic vault address for `owner`, whether or not it exists yet.
  /// Mirrors the on-chain CREATE2 derivation, so a client can fund it ahead of
  /// deployment.
  async predictVault(owner: Address): Promise<Address> {
    return this.publicClient.readContract({
      address: this.factory,
      abi: factoryAbi,
      functionName: "predict",
      args: [getAddress(owner)],
    });
  }

  /// The already-created vault for `owner`, or the zero address if none.
  async vaultOf(owner: Address): Promise<Address> {
    return this.publicClient.readContract({
      address: this.factory,
      abi: factoryAbi,
      functionName: "vaultOf",
      args: [getAddress(owner)],
    });
  }

  /// Create the connected account's vault. Returns the tx hash; the vault
  /// address equals predictVault(owner) and is also emitted as VaultCreated.
  async createVault(): Promise<Hash> {
    const wallet = this.requireWallet();
    const account = this.requireAccount(wallet);
    const { request } = await this.publicClient.simulateContract({
      address: this.factory,
      abi: factoryAbi,
      functionName: "createVault",
      account,
    });
    return wallet.writeContract(request);
  }

  /// Create a vault for `owner` (owner still solely controls it).
  async createVaultFor(owner: Address): Promise<Hash> {
    const wallet = this.requireWallet();
    const account = this.requireAccount(wallet);
    const { request } = await this.publicClient.simulateContract({
      address: this.factory,
      abi: factoryAbi,
      functionName: "createVaultFor",
      args: [getAddress(owner)],
      account,
    });
    return wallet.writeContract(request);
  }

  /* screened outbound actions */

  /// Screen + run one raw action from the vault. The vault reverts with
  /// Blocked(target, reason) before any value moves if the policy flags it.
  async execute(
    vault: Address,
    target: Address,
    value: bigint,
    data: Hex = "0x",
  ): Promise<Hash> {
    const wallet = this.requireWallet();
    const account = this.requireAccount(wallet);
    const { request } = await this.publicClient.simulateContract({
      address: getAddress(vault),
      abi: vaultAbi,
      functionName: "execute",
      args: [getAddress(target), value, data],
      account,
    });
    return wallet.writeContract(request);
  }

  /// Send native ETH from the vault to `to`, screened.
  async send(vault: Address, to: Address, value: bigint): Promise<Hash> {
    return this.execute(vault, to, value, "0x");
  }

  /// Send an ERC-20 from the vault to `to`, screened. The real recipient is
  /// decoded on-chain from the transfer calldata, so a flagged recipient is
  /// blocked even though the call target is the token contract.
  async sendToken(
    vault: Address,
    token: Address,
    to: Address,
    amount: bigint,
  ): Promise<Hash> {
    const data = encodeFunctionData({
      abi: erc20Abi,
      functionName: "transfer",
      args: [getAddress(to), amount],
    });
    return this.execute(vault, getAddress(token), 0n, data);
  }

  /// Screen + run a batch. Any flagged action reverts the whole batch.
  async executeBatch(
    vault: Address,
    targets: Address[],
    values: bigint[],
    datas: Hex[],
  ): Promise<Hash> {
    const wallet = this.requireWallet();
    const account = this.requireAccount(wallet);
    const { request } = await this.publicClient.simulateContract({
      address: getAddress(vault),
      abi: vaultAbi,
      functionName: "executeBatch",
      args: [targets.map(getAddress), values, datas],
      account,
    });
    return wallet.writeContract(request);
  }

  /* owner withdraw (returns funds to the owner, unscreened) */

  async withdrawETH(vault: Address, amount: bigint): Promise<Hash> {
    const wallet = this.requireWallet();
    const account = this.requireAccount(wallet);
    const { request } = await this.publicClient.simulateContract({
      address: getAddress(vault),
      abi: vaultAbi,
      functionName: "withdrawETH",
      args: [amount],
      account,
    });
    return wallet.writeContract(request);
  }

  async withdrawToken(
    vault: Address,
    token: Address,
    amount: bigint,
  ): Promise<Hash> {
    const wallet = this.requireWallet();
    const account = this.requireAccount(wallet);
    const { request } = await this.publicClient.simulateContract({
      address: getAddress(vault),
      abi: vaultAbi,
      functionName: "withdrawToken",
      args: [getAddress(token), amount],
      account,
    });
    return wallet.writeContract(request);
  }

  /* views */

  /// Dry-run the screen without executing. Free. Returns the decision plus a
  /// human-readable label for the reason code.
  async wouldAllow(
    vault: Address,
    target: Address,
    value: bigint,
    data: Hex = "0x",
  ): Promise<ScreenResult> {
    const [allowed, reason] = await this.publicClient.readContract({
      address: getAddress(vault),
      abi: vaultAbi,
      functionName: "wouldAllow",
      args: [getAddress(target), value, data],
    });
    return { allowed, reason, label: reasonLabel(reason) };
  }
