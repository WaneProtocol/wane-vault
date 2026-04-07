// Send an ERC-20 from the vault. The real recipient is decoded from the transfer
// calldata on-chain and screened, so a transfer to a flagged address reverts
// even though the call target is the token contract.
// Run with: tsx examples/screened-token-send.ts
import { createPublicClient, createWalletClient, http } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { base } from "viem/chains";
import { WaneVaultClient } from "@wane/vault-sdk";

const PK = process.env.PRIVATE_KEY as `0x${string}`;
const TOKEN = (process.env.TOKEN ?? "") as `0x${string}`;
const RECIPIENT = (process.env.RECIPIENT ?? "") as `0x${string}`;

async function main() {
  const account = privateKeyToAccount(PK);
  const publicClient = createPublicClient({ chain: base, transport: http() });
  const walletClient = createWalletClient({ account, chain: base, transport: http() });
  const wane = new WaneVaultClient({ publicClient, walletClient });

  const vault = await wane.vaultOf(account.address);
  if (vault === "0x0000000000000000000000000000000000000000") {
    throw new Error("no vault for this owner, run create-and-send.ts first");
  }

  const amount = 100_000000n; // 100 units of a 6-decimal token

  // free dry-run: the decoded recipient is screened, not just the token target
  const check = await wane.wouldAllowToken(vault, TOKEN, RECIPIENT, amount);
  console.log("screen:", check);
  if (!check.allowed) {
    console.log("recipient flagged, not sending");
    return;
  }

  const tx = await wane.sendToken(vault, TOKEN, RECIPIENT, amount);
  console.log("sent token, tx", tx);
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
