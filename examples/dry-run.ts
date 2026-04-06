// Free, read-only screening: ask the vault whether a send would be allowed
// without spending gas. Useful for UI that shows a result before the user signs.
// Run with: tsx examples/dry-run.ts
import { createPublicClient, http, parseEther } from "viem";
import { base } from "viem/chains";
import { WaneVaultClient } from "@wane/vault-sdk";

const OWNER = (process.env.OWNER ?? "") as `0x${string}`;
const RECIPIENT = (process.env.RECIPIENT ?? "") as `0x${string}`;

async function main() {
  // no walletClient needed for read-only calls
  const publicClient = createPublicClient({ chain: base, transport: http() });
  const wane = new WaneVaultClient({ publicClient });

  const vault = await wane.vaultOf(OWNER);
  if (vault === "0x0000000000000000000000000000000000000000") {
    throw new Error("no vault for this owner");
  }

  const value = parseEther("0.1");
  const check = await wane.wouldAllow(vault, RECIPIENT, value);

  console.log(`vault     ${vault}`);
  console.log(`recipient ${RECIPIENT}`);
  console.log(`allowed   ${check.allowed}`);
  console.log(`reason    ${check.reason} (${check.label})`);
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
