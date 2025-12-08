//! iVZA Solver: Route finding, optimization, and solving for the iVZA parallel execution engine.
//!
//! This crate provides the solver layer that sits on top of `ivza-core`'s execution planning.
//! It handles:
//!
//! - **pool**: Liquidity pool registry, AMM math (constant-product and CLMM), and pool graph.
//! - **router**: Multi-hop route finding using Dijkstra's algorithm on the pool graph.
//! - **solver**: Greedy and branch-and-bound solvers for optimal route assignment.
//! - **optimizer**: Swap splitting, transaction merging, and lane reordering.
//!
//! # Usage
//!
//! ```ignore
//! use ivza_solver::{SolverEngine, SolverStrategy};
//! use ivza_solver::pool::PoolInfo;
//! use ivza_solver::solver::SwapRequest;
//!
//! let mut engine = SolverEngine::new();
//! engine.register_pool(pool_info);
//! let result = engine.solve(&[swap_request], SolverStrategy::Greedy)?;
//! let optimized = engine.optimize(&result)?;
//! ```

pub mod optimizer;
pub mod pool;
pub mod router;
pub mod solver;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tracing::info;

use optimizer::{ExecutionOptimizer, OptimizationResult, OptimizerConfig};
use pool::{PoolFetcher, PoolInfo, PoolRegistry};
use router::{RouteConfig, RouteEngine};
use solver::{BranchAndBoundSolver, GreedySolver, Solver, SolverConfig, SolverResult, SwapRequest};

/// Which solver strategy to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolverStrategy {
    /// Fast greedy solver: independently picks the best route per swap.
    Greedy,
    /// Optimal branch-and-bound solver: considers shared liquidity.
    BranchAndBound,
}

impl Default for SolverStrategy {
    fn default() -> Self {
        SolverStrategy::Greedy
    }
}

/// The top-level engine that ties together pool registry, router, solver, and optimizer.
pub struct SolverEngine {
    /// Pool registry (thread-safe, can be shared).
    pub registry: PoolRegistry,
    /// Route engine (built on top of the registry).
    route_engine: Option<RouteEngine>,
    /// Solver configuration.
    pub solver_config: SolverConfig,
    /// Optimizer configuration.
    pub optimizer_config: OptimizerConfig,
    /// Route finding configuration.
    pub route_config: RouteConfig,
    /// Pool fetcher for simulated on-chain data.
    pub fetcher: PoolFetcher,
}

impl SolverEngine {
    /// Create a new solver engine with default configuration.
    pub fn new() -> Self {
        let registry = PoolRegistry::new();
        Self {
            registry,
            route_engine: None,
            solver_config: SolverConfig::default(),
            optimizer_config: OptimizerConfig::default(),
            route_config: RouteConfig::default(),
            fetcher: PoolFetcher::new(),
        }
    }

    /// Create with custom configurations.
    pub fn with_configs(
        solver_config: SolverConfig,
        optimizer_config: OptimizerConfig,
        route_config: RouteConfig,
    ) -> Self {
        let registry = PoolRegistry::new();
        Self {
            registry,
            route_engine: None,
            solver_config,
            optimizer_config,
            route_config,
            fetcher: PoolFetcher::new(),
        }
    }

    /// Register a single pool.
    pub fn register_pool(&mut self, pool: PoolInfo) {
        self.registry.register(pool);
        self.invalidate_route_engine();
    }

    /// Register multiple pools at once.
    pub fn register_pools(&mut self, pools: Vec<PoolInfo>) {
        for pool in pools {
            self.registry.register(pool);
        }
        self.invalidate_route_engine();
    }

    /// Fetch and register pools for the given token mints.
    pub fn fetch_and_register(&mut self, token_mints: &[Pubkey]) {
        let pools = self.fetcher.fetch_pools(token_mints);
        let count = pools.len();
        self.register_pools(pools);
        info!(
            "Fetched and registered {} pools for {} tokens",
            count,
            token_mints.len()
        );
    }

    /// Invalidate the cached route engine (call after pool changes).
    fn invalidate_route_engine(&mut self) {
        self.route_engine = None;
    }

    /// Get or build the route engine. We rebuild when the pool set changes.
    fn get_route_engine(&self) -> RouteEngine {
        // Build a new RouteEngine from the current registry state.
        // We create a new PoolRegistry and copy pools into it for the RouteEngine.
        // This is necessary because RouteEngine takes ownership of the registry.
        let new_registry = PoolRegistry::new();
        // Copy all pools from our registry to the new one.
        // We iterate through the DashMap and re-register each pool.
        for pool in self.all_pools() {
            new_registry.register(pool);
        }
        RouteEngine::new(new_registry).with_config(self.route_config.clone())
    }