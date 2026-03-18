# Getting Started with IVZA

This guide walks through installing IVZA, building the project, creating your first transaction graph, and running the CLI.

---

## Prerequisites

Install the following before proceeding:

### Rust

Install Rust via rustup. IVZA requires Rust 1.75 or later.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy
```

Verify:

```bash
rustc --version
# rustc 1.75.0 or later
```

### Node.js

Install Node.js 18 or later. Use nvm or download from the official site.

```bash
node --version
# v18.0.0 or later

npm --version
# 9.0.0 or later
```

### Solana CLI

Install the Solana CLI tools.

```bash
sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
```

Verify:

```bash
solana --version
# solana-cli 1.17.x
```

### Anchor

Install Anchor for on-chain program development.

```bash
cargo install --git https://github.com/coral-xyz/anchor avm --force
avm install 0.29.0
avm use 0.29.0
```

---

## Installation

### Clone the Repository

```bash
git clone <repository-url> ivza
cd ivza/product
```

### Build Everything

Use the provided build script:

```bash
chmod +x scripts/build.sh
./scripts/build.sh
```

Or build manually:

```bash
# Rust workspace
cargo build --release

# TypeScript SDK
cd sdk/
npm install
npm run build
cd ..
```

### Verify the Build

```bash
cargo test
cargo run --bin ivza-cli -- --help
```

---

## Your First Transaction Graph

### Using Rust

Create a simple graph with two independent swap nodes:

```rust
use ivza_core::graph::{TransactionGraph, TxNode, Edge};
use ivza_core::analyzer::DependencyAnalyzer;
use ivza_core::scheduler::ParallelScheduler;
use solana_sdk::pubkey::Pubkey;

fn main() -> anyhow::Result<()> {
    let mut graph = TransactionGraph::new();

    // Two swaps with no shared accounts -- fully independent
    let node_a = graph.add_node(TxNode::new(
        swap_instruction(usdc_mint, sol_mint, 100_000_000),
        vec![usdc_mint],
        vec![user_usdc_account],
    ));

    let node_b = graph.add_node(TxNode::new(
        swap_instruction(usdt_mint, bonk_mint, 50_000_000),
        vec![usdt_mint],
        vec![user_usdt_account],
    ));

    // Analyze -- should detect no conflicts
    let analyzer = DependencyAnalyzer::new();
    let analysis = analyzer.analyze(&graph)?;

    assert!(analysis.are_independent(node_a, node_b));

    // Schedule -- both nodes land in level 0
    let scheduler = ParallelScheduler::new(8);
    let schedule = scheduler.schedule(&graph, &analysis)?;

    assert_eq!(schedule.level_count(), 1);
    assert_eq!(schedule.level(0).len(), 2);

    println!("Graph has {} levels with {} total nodes",
        schedule.level_count(),
        graph.node_count(),
    );

    Ok(())
}
```

### Using TypeScript

```typescript
import { IvzaClient, Intent } from "@ivza/sdk";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";

async function main() {
  const connection = new Connection("http://127.0.0.1:8899");
  const wallet = Keypair.generate();

  const client = new IvzaClient({
    connection,
    wallet,
    maxParallelism: 4,
  });

  const intent: Intent = {
    type: "batch_transfer",
    transfers: [
      { destination: new PublicKey("..."), amount: 1_000_000n },
      { destination: new PublicKey("..."), amount: 2_000_000n },
      { destination: new PublicKey("..."), amount: 500_000n },
    ],
    mint: new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
  };

  // Decompose, analyze, schedule, and execute in one call
  const results = await client.execute(intent);

  for (const result of results) {
    console.log(`Node ${result.nodeId}: ${result.signature}`);
  }
}

main().catch(console.error);
```

---

## Using the CLI

The IVZA CLI provides a command-line interface for common operations.

### Help

```bash
cargo run --bin ivza-cli -- --help
```

### Execute an Intent from JSON

```bash
cargo run --bin ivza-cli -- execute \
  --intent intent.json \
  --rpc https://api.devnet.solana.com \
  --keypair ~/.config/solana/id.json \
  --max-parallelism 8
```

### Analyze a Graph

Dry-run mode decomposes and analyzes an intent without executing:

```bash
cargo run --bin ivza-cli -- analyze \
  --intent intent.json \
  --output schedule.json
```

This outputs the execution schedule as JSON, showing levels, lanes, and dependency edges.

### Example Intent JSON

```json
{
  "type": "composite",
  "steps": [
    {
      "type": "swap",
      "inputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      "outputMint": "So11111111111111111111111111111111111111112",
      "amount": 100000000,
      "slippageBps": 50
    },
    {
      "type": "swap",
      "inputMint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      "outputMint": "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",
      "amount": 50000000,
      "slippageBps": 100
    }
  ],
  "atomic": false
}
```

---

## Next Steps

- Read the [Architecture](architecture.md) document for a deep dive into the pipeline stages.
- See the [API Reference](api-reference.md) for detailed type and function documentation.
- Check out the [Contributing Guide](../CONTRIBUTING.md) to set up a development environment.
