// Minimal ABIs for the on-chain Wane vault surfaces the SDK touches.

export const factoryAbi = [
  {
    type: "function",
    name: "createVault",
    stateMutability: "nonpayable",
    inputs: [],
    outputs: [{ name: "vault", type: "address" }],
  },
  {
    type: "function",
    name: "createVaultFor",
    stateMutability: "nonpayable",
    inputs: [{ name: "owner", type: "address" }],
    outputs: [{ name: "vault", type: "address" }],
  },
  {
    type: "function",
    name: "predict",
    stateMutability: "view",
    inputs: [{ name: "owner", type: "address" }],
    outputs: [{ name: "", type: "address" }],
  },
  {
    type: "function",
    name: "vaultOf",
    stateMutability: "view",
    inputs: [{ name: "", type: "address" }],
    outputs: [{ name: "", type: "address" }],
  },
  {
    type: "function",
    name: "policy",
    stateMutability: "view",
    inputs: [],
    outputs: [{ name: "", type: "address" }],
  },
  {
    type: "event",
    name: "VaultCreated",
    inputs: [
      { name: "owner", type: "address", indexed: true },
      { name: "vault", type: "address", indexed: false },
    ],
    anonymous: false,
  },
] as const;
