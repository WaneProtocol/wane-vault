![IVZA Banner](assets/banner.png)

[![Website](https://img.shields.io/badge/website-ivza.dev-020204?style=flat-square&labelColor=020204&color=00ddb5)](https://ivza.dev)
[![Docs](https://img.shields.io/badge/docs-ivza.dev%2Fdocs-020204?style=flat-square&labelColor=020204&color=00ddb5)](https://ivza.dev/docs)
[![Blog](https://img.shields.io/badge/blog-ivza.dev%2Fblog-020204?style=flat-square&labelColor=020204&color=00b4d8)](https://ivza.dev/blog)
[![X](https://img.shields.io/badge/X-@ivzadotdev-020204?style=flat-square&labelColor=020204&color=00b4d8)](https://x.com/ivzadotdev)
[![License](https://img.shields.io/badge/license-MIT-020204?style=flat-square&labelColor=020204&color=00ddb5)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-020204?style=flat-square&labelColor=020204&color=00ddb5)](https://www.rust-lang.org)
[![Solana](https://img.shields.io/badge/solana-1.18-020204?style=flat-square&labelColor=020204&color=00b4d8)](https://solana.com)

# IVZA

Parallel execution engine for Solana. Decomposes transaction intents into dependency graphs and schedules them across parallel execution lanes for maximum throughput on Sealevel.

## Architecture

```mermaid
flowchart LR
    A[Intent] --> B[Decompose]
    B --> C[Analyze]
    C --> D[Schedule]
    D --> E[Execute]
    E --> F[Solana]

    style A fill:#0d1117,stroke:#00ddb5,color:#e4eaef
    style B fill:#0d1117,stroke:#00ddb5,color:#e4eaef
    style C fill:#0d1117,stroke:#00ddb5,color:#e4eaef
    style D fill:#0d1117,stroke:#00ddb5,color:#e4eaef
    style E fill:#0d1117,stroke:#00ddb5,color:#e4eaef
    style F fill:#0d1117,stroke:#00b4d8,color:#e4eaef
```

| Stage | Module | Description |
|-------|--------|-------------|
| Decompose | `GraphDecomposer` | Break intents into atomic instruction nodes with account access patterns |
| Analyze | `DependencyAnalyzer` `CriticalPathAnalyzer` | Detect read/write conflicts, build dependency edges, compute critical path |
| Schedule | `ExecutionPlanner` `PriorityScheduler` | Assign priorities, pack independent nodes into parallel lanes |
| Execute | `ParallelExecutor` `BundleBuilder` | Map lanes to Jito bundles, submit concurrently on Sealevel |

## Crates

| Crate | Description |
|-------|-------------|
| `ivza-core` | Graph decomposition, dependency analysis, critical path, parallel scheduler. Built on petgraph. |
| `ivza-solver` | Dijkstra multi-hop routing, AMM/CLMM math, greedy and branch-and-bound optimization. |
| `ivza-engine` | Anchor on-chain program. Graph submission, lane execution, settlement. |
| `ivza-cli` | Offline analysis, graph submission, status tracking. |

## SDK

TypeScript client at `sdk/`. Graph builder, intent DSL parser, parallel executor, Jito bundle submission.

## Quick Start

```bash
git clone https://github.com/ivzadotdev/ivza.git
cd ivza

# Build Rust workspace
cargo build --release

# Build TypeScript SDK
cd sdk && npm install && npm run build && cd ..

# Run tests
cargo test
```

## Usage

### Rust

```rust
use ivza_core::IvzaEngine;

let engine = IvzaEngine::default();

// full pipeline: decompose -> analyze -> schedule
let plan = engine.process(&instructions)?;

for lane in &plan.lanes {
    println!(
        "lane {}: {} nodes, {} CU",
        lane.index,
        lane.nodes.len(),
        lane.estimated_cu
    );
}
```

### TypeScript

```typescript
import { IvzaClient } from "./sdk/src";

const client = new IvzaClient(connection, wallet);

const plan = await client.processIntent(`
  swap 500 USDC to SOL
  stake 50% of output SOL
`);

const results = await client.executeLocally(plan);
```

### Intent DSL

```
swap 100 USDC to SOL
stake 50% of output SOL
transfer 10 SOL to 7xKp...3nF
```

Deterministic parser. No LLM. Tokenizes on whitespace, matches verb, resolves mints and ATAs, builds transaction graph.

### CLI

```bash
# Analyze an intent offline
ivza analyze --intent swap.json

# Submit a graph on-chain
ivza submit --intent swap.json --keypair ~/.config/solana/id.json

# Check graph status
ivza status --graph <graph-id>
```

## Project Structure

```
ivza/
  crates/
    ivza-core/        Core engine (graph, analyzer, scheduler, intent)
    ivza-solver/      Route solver (pool, router, optimizer)
    ivza-cli/         CLI tool
  programs/
    ivza-engine/      Anchor on-chain program
  sdk/                TypeScript SDK
  tests/              Integration tests
  docs/               Documentation
  scripts/            Build, deploy, test scripts
  .github/            CI/CD workflows, issue templates
```

## Key Algorithms

- **Dependency Detection** -- O(n^2) scan with HashSet::is_disjoint on account access sets. Classifies write-write, read-write, and account conflicts.
- **Critical Path Method** -- Forward pass (earliest start) + backward pass (latest start). Zero-slack nodes form the critical path and get highest scheduling priority.
- **Lane Packing** -- Topological sort into levels, then greedy bin-packing per level. Nodes only share a lane if their account sets don't conflict.
- **Solver** -- Dijkstra on pool graph for multi-hop routing. Constant-product AMM and CLMM tick-range math. Branch-and-bound for globally optimal route assignment across shared liquidity pools.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. See [LICENSE](LICENSE).
