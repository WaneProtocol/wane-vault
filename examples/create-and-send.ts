// Create a vault, then send ETH through the screen. The send reverts with
// Blocked(target, reason) if the policy flags the recipient, before any value
// moves. Run with: tsx examples/create-and-send.ts
import { createPublicClient, createWalletClient, http, parseEther } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { base } from "viem/chains";
import { WaneVaultClient } from "@wane/vault-sdk";

const PK = process.env.PRIVATE_KEY as `0x${string}`;
const RECIPIENT = (process.env.RECIPIENT ?? "") as `0x${string}`;

async function main() {
  const account = privateKeyToAccount(PK);
  const publicClient = createPublicClient({ chain: base, transport: http() });
  const walletClient = createWalletClient({ account, chain: base, transport: http() });
  const wane = new WaneVaultClient({ publicClient, walletClient });

  // 1. compute the vault address, create it if it does not exist yet
  const vault = await wane.predictVault(account.address);
  const existing = await wane.vaultOf(account.address);
  if (existing.toLowerCase() !== vault.toLowerCase()) {
    const createTx = await wane.createVault();
    console.log("created vault", vault, "tx", createTx);
  } else {
    console.log("vault already exists", vault);
  }

  // 2. dry-run the screen for free, then send if allowed
  const value = parseEther("0.01");
  const check = await wane.wouldAllow(vault, RECIPIENT, value);
  console.log("screen:", check);
  if (!check.allowed) {
    console.log("recipient flagged, not sending");
    return;
  }

  const sendTx = await wane.send(vault, RECIPIENT, value);
  console.log("sent", value.toString(), "wei, tx", sendTx);
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
