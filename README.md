![iVZA](../assets/logo.png)

![License](https://img.shields.io/badge/license-MIT-0a0a0a?style=flat-square&labelColor=0a0a0a&color=00ff41)
![Build Status](https://img.shields.io/badge/build-passing-0a0a0a?style=flat-square&labelColor=0a0a0a&color=00ff41)
![Rust](https://img.shields.io/badge/rust-1.75%2B-0a0a0a?style=flat-square&labelColor=0a0a0a&color=ff6600)
![Solana](https://img.shields.io/badge/solana-1.17-0a0a0a?style=flat-square&labelColor=0a0a0a&color=ff6600)
![TypeScript](https://img.shields.io/badge/typescript-5.x-0a0a0a?style=flat-square&labelColor=0a0a0a&color=00ff41)

---

# iVZA Parallel Execution Engine

**iVZA** is a parallel execution engine for Solana that decomposes, analyzes, and executes transaction graphs with maximum parallelism. It transforms sequential transaction intent into an optimized directed acyclic graph (DAG), identifies independent execution paths, and dispatches them across parallel lanes to minimize latency and maximize throughput.

The engine targets high-frequency DeFi operations, complex multi-step swaps, batch liquidations, and any workload where transaction ordering constraints allow concurrent execution.

---

## Table of Contents

- [Architecture](#architecture)
- [Core Concepts](#core-concepts)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Project Structure](#project-structure)
- [Configuration](#configuration)
- [Testing](#testing)
- [Performance](#performance)
- [Contributing](#contributing)
- [License](#license)

---

## Architecture

The iVZA pipeline processes transaction intents through five stages, each responsible for a discrete transformation of the workload.

```mermaid
flowchart LR
    A[Intent] --> B[Graph Decomposer]
    B --> C[Dependency Analyzer]
    C --> D[Parallel Scheduler]
    D --> E[Execution Engine]
    E --> F[Solana]

    style A fill:#0a0a0a,stroke:#ff6600,color:#ffffff
    style B fill:#0a0a0a,stroke:#00ff41,color:#ffffff
    style C fill:#0a0a0a,stroke:#00ff41,color:#ffffff
    style D fill:#0a0a0a,stroke:#00ff41,color:#ffffff
    style E fill:#0a0a0a,stroke:#00ff41,color:#ffffff
    style F fill:#0a0a0a,stroke:#ff6600,color:#ffffff
```

### Pipeline Stages

1. **Intent Parsing** -- Raw user intent is parsed into a normalized intermediate representation. Intents can express swaps, transfers, program invocations, or composite operations.

2. **Graph Decomposition** -- The intent is decomposed into a transaction graph (DAG). Each node represents an atomic on-chain instruction. Edges encode data dependencies and ordering constraints.

3. **Dependency Analysis** -- The analyzer walks the DAG and computes read/write sets for every node. Nodes that share no conflicting account access are marked as independent. The analyzer produces a dependency matrix used by the scheduler.

4. **Parallel Scheduling** -- Independent nodes are grouped into parallel lanes. The scheduler uses a topological sort with level assignment: all nodes at the same level execute concurrently. Lane width is bounded by configurable concurrency limits.

5. **Execution Engine** -- Each lane dispatches transactions to Solana via RPC or TPU. The engine handles confirmation, retry logic, and error propagation. Failed nodes trigger rollback or compensation paths as defined by the intent.

---

## Core Concepts

### Transaction Graphs

A transaction graph is a directed acyclic graph where each node is an atomic instruction and each edge is a dependency. The graph is the central data structure that flows through the entire pipeline.

```rust
pub struct TransactionGraph {
    pub id: GraphId,
    pub nodes: Vec<TxNode>,
    pub edges: Vec<Edge>,
    pub metadata: GraphMetadata,
}

pub struct TxNode {
    pub id: NodeId,
    pub instruction: Instruction,
    pub read_set: Vec<Pubkey>,
    pub write_set: Vec<Pubkey>,
    pub priority: u8,
}

pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
}

pub enum EdgeKind {
    DataDependency,
    OrderingConstraint,
    AccountLock,
}
```

### Dependency Analysis

The dependency analyzer computes a conflict matrix across all nodes. Two nodes conflict if their write sets intersect, or if one node's write set intersects the other's read set. Non-conflicting nodes are candidates for parallel execution.

The algorithm operates in O(n^2 * k) time where n is the number of nodes and k is the average account set size. For graphs with sparse dependencies this yields significant parallelism.

### Parallel Lanes

Parallel lanes are the execution units of the scheduler. Each lane contains a sequence of non-conflicting transactions that can be submitted to Solana concurrently. The number of active lanes is bounded by the configured `max_parallelism` parameter.

```
Lane 0: [Tx_A] -> [Tx_D] -> [Tx_G]
Lane 1: [Tx_B] -> [Tx_E]
Lane 2: [Tx_C] -> [Tx_F] -> [Tx_H] -> [Tx_I]
```

Lanes synchronize at barrier points when a node depends on outputs from multiple lanes.

### Intent System

Intents are the high-level input format. An intent describes what the user wants to accomplish without specifying how to decompose it into transactions.

```rust
pub enum Intent {
    Swap {
        input_mint: Pubkey,
        output_mint: Pubkey,
        amount: u64,
        slippage_bps: u16,
    },
    BatchTransfer {
        transfers: Vec<Transfer>,
    },
    CompositeOperation {
        steps: Vec<Intent>,
        atomic: bool,
    },
    ProgramInvocation {
        program_id: Pubkey,
        accounts: Vec<AccountMeta>,
        data: Vec<u8>,
    },
}
```

---

## Quick Start

### Prerequisites

- Rust 1.75 or later
- Node.js 18 or later
- Solana CLI 1.17 or later
- Anchor 0.29 or later (for on-chain program development)

### Installation

```bash
git clone <repository-url> ivza
cd ivza/product

# Build the Rust workspace
cargo build --release

# Build the TypeScript SDK
cd sdk/
npm install
npm run build
cd ..
```

### Verify Installation

```bash
# Run the test suite
cargo test

# Run the CLI
cargo run --bin ivza-cli -- --help
```

---

## Usage

### Rust

#### Creating and Executing a Transaction Graph

```rust
use ivza_core::{
    engine::ExecutionEngine,
    graph::{TransactionGraph, TxNode, Edge, EdgeKind},
    scheduler::ParallelScheduler,
    analyzer::DependencyAnalyzer,
    config::EngineConfig,
};
use solana_sdk::pubkey::Pubkey;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Configure the engine
    let config = EngineConfig {
        max_parallelism: 8,
        retry_attempts: 3,
        confirmation_commitment: "confirmed".to_string(),
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
    };

    // Build a transaction graph
    let mut graph = TransactionGraph::new();

    let node_a = graph.add_node(TxNode::new(
        swap_instruction(mint_a, mint_b, 1000),
        vec![mint_a],       // read set
        vec![token_acc_a],  // write set
    ));

    let node_b = graph.add_node(TxNode::new(
        swap_instruction(mint_c, mint_d, 2000),
        vec![mint_c],
        vec![token_acc_c],
    ));

    let node_c = graph.add_node(TxNode::new(
        transfer_instruction(token_acc_a, destination, 500),
        vec![token_acc_a],
        vec![destination],
    ));

    // node_c depends on node_a (reads token_acc_a after node_a writes it)
    graph.add_edge(Edge::new(node_a, node_c, EdgeKind::DataDependency));
    // node_b is independent -- no shared accounts

    // Analyze dependencies
    let analyzer = DependencyAnalyzer::new();
    let analysis = analyzer.analyze(&graph)?;

    // Schedule into parallel lanes
    let scheduler = ParallelScheduler::new(config.max_parallelism);
    let schedule = scheduler.schedule(&graph, &analysis)?;

    // Execute
    let engine = ExecutionEngine::new(config).await?;
    let results = engine.execute(schedule).await?;

    for result in &results {
        println!("tx {} -- signature: {}", result.node_id, result.signature);
    }

    Ok(())
}
```

#### Using the Intent System

```rust
use ivza_core::intent::{Intent, IntentProcessor};

let intent = Intent::CompositeOperation {
    steps: vec![
        Intent::Swap {
            input_mint: usdc_mint,
            output_mint: sol_mint,
            amount: 100_000_000,
            slippage_bps: 50,
        },
        Intent::Swap {
            input_mint: usdc_mint,
            output_mint: bonk_mint,
            amount: 50_000_000,
            slippage_bps: 100,
        },
    ],
    atomic: false, // allow partial execution
};

let processor = IntentProcessor::new(config);
let graph = processor.decompose(intent)?;
let results = processor.execute(graph).await?;
```

### TypeScript

#### SDK Quick Start

```typescript
import {
  IvzaClient,
  Intent,
  SwapIntent,
  BatchTransferIntent,
} from "@ivza/sdk";
import { Connection, PublicKey, Keypair } from "@solana/web3.js";

const connection = new Connection("https://api.mainnet-beta.solana.com");
const wallet = Keypair.fromSecretKey(/* your key */);

const client = new IvzaClient({
  connection,
  wallet,
  maxParallelism: 8,
});

// Execute parallel swaps
const intent: Intent = {
  type: "composite",
  steps: [
    {
      type: "swap",
      inputMint: new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
      outputMint: new PublicKey("So11111111111111111111111111111111111111112"),
      amount: 100_000_000n,
      slippageBps: 50,
    },
    {
      type: "swap",
      inputMint: new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"),
      outputMint: new PublicKey("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"),
      amount: 50_000_000n,
      slippageBps: 100,
    },
  ],
  atomic: false,
};

const results = await client.execute(intent);

for (const result of results) {
  console.log(`Node ${result.nodeId}: ${result.signature}`);
}
```

#### Batch Transfers

```typescript
const batchIntent: Intent = {
  type: "batch_transfer",
  transfers: [
    { destination: recipientA, amount: 1_000_000n },
    { destination: recipientB, amount: 2_000_000n },
    { destination: recipientC, amount: 500_000n },
    { destination: recipientD, amount: 3_000_000n },
  ],
  mint: usdcMint,
};

const results = await client.execute(batchIntent);
console.log(`Executed ${results.length} transfers in parallel`);
```

---

## Project Structure

```
product/
  programs/           Anchor on-chain programs
    ivza-engine/      Core execution engine program
  core/               Rust core library
    src/
      graph/          Transaction graph types and builders
      analyzer/       Dependency analysis algorithms
      scheduler/      Parallel lane scheduler
      engine/         Execution engine and RPC dispatch
      intent/         Intent parsing and decomposition
  cli/                Command-line interface
  sdk/                TypeScript SDK
    src/
      client.ts       IvzaClient main class
      intent.ts       Intent type definitions
      graph.ts        Graph types for TypeScript
      index.ts        Package entry point
  scripts/            Build, test, and deploy scripts
  docs/               Documentation
  .github/            CI configuration and templates
```

---

## Configuration

The engine accepts configuration via `EngineConfig` in Rust or the client constructor in TypeScript.

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `max_parallelism` | `usize` | `8` | Maximum number of concurrent execution lanes |
| `retry_attempts` | `u32` | `3` | Number of retry attempts per failed transaction |
| `confirmation_commitment` | `String` | `"confirmed"` | Solana commitment level for confirmations |
| `rpc_url` | `String` | -- | Solana RPC endpoint URL |
| `tpu_enabled` | `bool` | `false` | Use TPU for transaction submission |
| `compute_unit_price` | `u64` | `0` | Priority fee in micro-lamports per compute unit |
| `timeout_ms` | `u64` | `30000` | Timeout per transaction in milliseconds |

---

## Testing

```bash
# Run all Rust tests
cargo test

# Run with logging
RUST_LOG=debug cargo test -- --nocapture

# Run TypeScript SDK tests
cd sdk/
npm test

# Run the full test suite (Rust + TypeScript)
./scripts/test.sh
```

---

## Performance

iVZA achieves parallelism gains proportional to the independence ratio of the transaction graph. For workloads with high account locality (few shared accounts), the engine approaches linear speedup up to the `max_parallelism` bound.

| Workload | Sequential (ms) | iVZA Parallel (ms) | Speedup |
|----------|-----------------|---------------------|---------|
| 10 independent swaps | ~4500 | ~500 | 9x |
| 20 batch transfers | ~9000 | ~1200 | 7.5x |
| 5-step composite (3 independent) | ~2200 | ~1000 | 2.2x |
| Fully sequential chain | ~2200 | ~2200 | 1x |

Benchmarks measured on mainnet-beta with `max_parallelism: 10` and `confirmed` commitment.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and the pull request process.

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
