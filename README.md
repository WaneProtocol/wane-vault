![IVZA Banner](assets/banner.png)

![License](https://img.shields.io/badge/license-MIT-020204?style=flat-square&labelColor=020204&color=00ddb5)
![Rust](https://img.shields.io/badge/rust-1.75%2B-020204?style=flat-square&labelColor=020204&color=00ddb5)
![Solana](https://img.shields.io/badge/solana-1.18-020204?style=flat-square&labelColor=020204&color=00b4d8)
![TypeScript](https://img.shields.io/badge/typescript-5.x-020204?style=flat-square&labelColor=020204&color=00ddb5)
![Open Source](https://img.shields.io/badge/open%20source-MIT-020204?style=flat-square&labelColor=020204&color=00b4d8)

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