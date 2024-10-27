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
use solver::{
    BranchAndBoundSolver, GreedySolver, Solver, SolverConfig, SolverResult, SwapRequest,
};

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

    /// Get all registered pools.
    fn all_pools(&self) -> Vec<PoolInfo> {
        // We need to collect all pools from the registry.
        // Since PoolRegistry uses DashMap internally, we can iterate it.
        let mut pools = Vec::new();
        // Access the pools through the token_index to enumerate all pools.
        let mut seen = std::collections::HashSet::new();
        // Use a helper: for each token, get its pools.
        // This is a workaround since we don't expose DashMap iteration directly.
        // Instead, we just get pools for each known token and deduplicate.
        let graph = self.registry.build_graph();
        for token in &graph.tokens {
            for pool in self.registry.pools_for_token(token) {
                if seen.insert(pool.address) {
                    pools.push(pool);
                }
            }
        }
        pools
    }

    /// Solve swap requests using the specified strategy.
    pub fn solve(
        &self,
        requests: &[SwapRequest],
        strategy: SolverStrategy,
    ) -> Result<SolverResult> {
        let engine = self.get_route_engine();

        match strategy {
            SolverStrategy::Greedy => {
                let solver = GreedySolver::new();
                solver.solve(requests, &engine, &self.solver_config)
            }
            SolverStrategy::BranchAndBound => {
                let solver = BranchAndBoundSolver::new();
                solver.solve(requests, &engine, &self.solver_config)
            }
        }
    }

    /// Solve and then optimize the result.
    pub fn solve_optimized(
        &self,
        requests: &[SwapRequest],
        strategy: SolverStrategy,
    ) -> Result<OptimizationResult> {
        let result = self.solve(requests, strategy)?;
        self.optimize(&result)
    }

    /// Optimize an existing solver result.
    pub fn optimize(&self, result: &SolverResult) -> Result<OptimizationResult> {
        let engine = self.get_route_engine();
        let optimizer = ExecutionOptimizer::new().with_config(self.optimizer_config.clone());
        optimizer.optimize(result, &engine)
    }

    /// Find the best route between two tokens for a given amount.
    pub fn find_best_route(
        &self,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        amount: u64,
    ) -> Result<router::Route> {
        let engine = self.get_route_engine();
        engine.find_best_route(input_mint, output_mint, amount)
    }

    /// Find multiple routes between two tokens.
    pub fn find_routes(
        &self,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        amount: u64,
    ) -> Result<Vec<router::Route>> {
        let engine = self.get_route_engine();
        engine.find_routes(input_mint, output_mint, amount)
    }

    /// Get the number of registered pools.
    pub fn pool_count(&self) -> usize {
        self.registry.pool_count()
    }

    /// Get the number of known tokens.
    pub fn token_count(&self) -> usize {
        self.registry.token_count()
    }

    /// Update reserves for a pool after fetching new on-chain data.
    pub fn update_reserves(
        &mut self,
        pool_address: &Pubkey,
        reserve_a: u64,
        reserve_b: u64,
    ) -> Result<()> {
        self.registry
            .update_reserves(pool_address, reserve_a, reserve_b)?;
        self.invalidate_route_engine();
        Ok(())
    }
}

impl Default for SolverEngine {
    fn default() -> Self {
        Self::new()
    }
}

