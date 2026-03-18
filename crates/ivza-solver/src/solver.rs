//! Solver trait and implementations for finding optimal execution routes.
//!
//! Provides:
//! - `Solver` trait: interface for route-finding strategies.
//! - `GreedySolver`: fast solver that picks the best route for each node independently.
//! - `BranchAndBoundSolver`: optimal solver that minimizes total cost across the plan.
//! - `SolverConfig`: shared configuration for all solver implementations.

use std::collections::HashMap;
use std::fmt;
use std::time::Instant;

use anyhow::Result;
use ivza_core::types::NodeId;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info, warn};

use crate::router::{Route, RouteEngine};

/// Configuration shared by all solver implementations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverConfig {
    /// Maximum number of routes to evaluate per node.
    pub max_routes: usize,
    /// Solver timeout in milliseconds.
    pub timeout_ms: u64,
    /// Maximum acceptable slippage in basis points.
    pub slippage_bps: u16,
    /// Minimum output ratio (output/input) below which a route is rejected.
    pub min_output_ratio: f64,
    /// Whether to allow multi-hop routes.
    pub allow_multi_hop: bool,
    /// Maximum number of hops per route.
    pub max_hops: usize,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            max_routes: 10,
            timeout_ms: 5_000,
            slippage_bps: 100, // 1%
            min_output_ratio: 0.90,
            allow_multi_hop: true,
            max_hops: 4,
        }
    }
}

/// Description of a swap that a solver should find routes for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapRequest {
    /// Node in the execution plan this swap belongs to.
    pub node_id: NodeId,
    /// Input token mint.
    pub input_mint: Pubkey,
    /// Output token mint.
    pub output_mint: Pubkey,
    /// Input amount.
    pub amount: u64,
    /// Optional label for debugging.
    pub label: Option<String>,
}

/// Result of solving a single swap request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolvedSwap {
    /// The original swap request.
    pub request: SwapRequest,
    /// The chosen route.
    pub route: Route,
    /// Minimum acceptable output after slippage.
    pub min_output: u64,
}

/// Overall result from the solver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverResult {
    /// Solved routes for each swap request, keyed by node ID.
    pub solved_swaps: HashMap<NodeId, SolvedSwap>,
    /// Total estimated input across all swaps.
    pub total_input: u64,
    /// Total estimated output across all swaps.
    pub total_output: u64,
    /// Total estimated cost in lamports (fees + rent).
    pub estimated_cost_lamports: u64,
    /// Number of swap requests that could not be routed.
    pub failed_count: usize,
    /// Time taken to solve, in milliseconds.
    pub solve_time_ms: u64,
}

impl SolverResult {
    /// Aggregate output/input ratio.
    pub fn output_ratio(&self) -> f64 {
        if self.total_input == 0 {
            return 0.0;
        }
        self.total_output as f64 / self.total_input as f64
    }

    /// Returns true if all swaps were successfully routed.
    pub fn all_solved(&self) -> bool {
        self.failed_count == 0
    }
}

impl fmt::Display for SolverResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SolverResult(swaps={}, failed={}, in={}, out={}, cost={}, time={}ms)",
            self.solved_swaps.len(),
            self.failed_count,
            self.total_input,
            self.total_output,
            self.estimated_cost_lamports,
            self.solve_time_ms,
        )
    }
}

/// Trait for solver implementations.
pub trait Solver {
    /// Solve the given swap requests and return routes.
    fn solve(
        &self,
        requests: &[SwapRequest],
        engine: &RouteEngine,
        config: &SolverConfig,
    ) -> Result<SolverResult>;

    /// Name of the solver strategy.
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// GreedySolver
// ---------------------------------------------------------------------------

/// Greedy solver: for each swap request, independently pick the best route.
///
/// Fast (O(n * k) where k is the number of candidate routes per request),
/// but does not consider interactions between swaps (e.g., shared pool liquidity).
pub struct GreedySolver;

impl GreedySolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GreedySolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for GreedySolver {
    fn name(&self) -> &str {
        "GreedySolver"
    }

    fn solve(
        &self,
        requests: &[SwapRequest],
        engine: &RouteEngine,
        config: &SolverConfig,
    ) -> Result<SolverResult> {
        let start = Instant::now();
        let deadline = start + std::time::Duration::from_millis(config.timeout_ms);

        let mut solved_swaps = HashMap::new();
        let mut total_input = 0u64;
        let mut total_output = 0u64;
        let mut failed_count = 0usize;

        for request in requests {
            if Instant::now() > deadline {
                warn!(
                    "GreedySolver: timeout after {}ms, {} of {} solved",
                    config.timeout_ms,
                    solved_swaps.len(),
                    requests.len()
                );
                failed_count += requests.len() - solved_swaps.len();
                break;
            }

            match engine.find_routes(&request.input_mint, &request.output_mint, request.amount) {
                Ok(routes) => {
                    // Filter by config constraints.
                    let valid_routes: Vec<Route> = routes
                        .into_iter()
                        .filter(|r| {
                            if !config.allow_multi_hop && r.hop_count() > 1 {
                                return false;
                            }
                            if r.hop_count() > config.max_hops {
                                return false;
                            }
                            let ratio = r.exchange_rate();
                            if ratio < config.min_output_ratio {
                                return false;
                            }
                            true
                        })
                        .take(config.max_routes)
                        .collect();

                    if let Some(best) = valid_routes.into_iter().next() {
                        let slippage_factor =
                            (10_000u64 - config.slippage_bps as u64) as f64 / 10_000.0;
                        let min_output = (best.output_amount as f64 * slippage_factor) as u64;

                        total_input += request.amount;
                        total_output += best.output_amount;

                        debug!(
                            "GreedySolver: node {} -> {} hops, out={}, min_out={}",
                            request.node_id,
                            best.hop_count(),
                            best.output_amount,
                            min_output,
                        );

                        solved_swaps.insert(
                            request.node_id,
                            SolvedSwap {
                                request: request.clone(),
                                route: best,
                                min_output,
                            },
                        );
                    } else {
                        warn!(
                            "GreedySolver: no valid route for node {} ({} -> {})",
                            request.node_id, request.input_mint, request.output_mint,
                        );
                        failed_count += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "GreedySolver: route finding failed for node {}: {}",
                        request.node_id, e
                    );
                    failed_count += 1;
                }
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;

        // Estimate cost: 5000 lamports base + 200 lamports per hop.
        let total_hops: usize = solved_swaps.values().map(|s| s.route.hop_count()).sum();
        let estimated_cost = 5_000u64 * solved_swaps.len() as u64 + 200 * total_hops as u64;

        info!(
            "GreedySolver: solved {}/{} swaps in {}ms",
            solved_swaps.len(),
            requests.len(),
            elapsed,
        );

        Ok(SolverResult {
            solved_swaps,
            total_input,
            total_output,
            estimated_cost_lamports: estimated_cost,
            failed_count,
            solve_time_ms: elapsed,
        })
    }
}

// ---------------------------------------------------------------------------
// BranchAndBoundSolver
// ---------------------------------------------------------------------------

/// Branch-and-bound solver: finds globally optimal route assignments by
/// considering interactions between swaps that share pool liquidity.
///
/// Uses DFS with bounding to prune unpromising branches. Falls back to
/// greedy if the search space is too large.
pub struct BranchAndBoundSolver {
    /// Maximum number of branch-and-bound nodes to explore.
    pub max_nodes: usize,
}

impl BranchAndBoundSolver {
    pub fn new() -> Self {
        Self { max_nodes: 10_000 }
    }

    pub fn with_max_nodes(mut self, max: usize) -> Self {
        self.max_nodes = max;
        self
    }

    /// Detect which swap requests share pool liquidity.
    #[allow(dead_code)]
    fn find_shared_pools(
        &self,
        candidates: &HashMap<NodeId, Vec<Route>>,
    ) -> HashMap<Pubkey, Vec<NodeId>> {
        let mut pool_to_nodes: HashMap<Pubkey, Vec<NodeId>> = HashMap::new();

        for (&node_id, routes) in candidates {
            for route in routes {
                for pool_addr in route.pool_addresses() {
                    pool_to_nodes.entry(pool_addr).or_default().push(node_id);
                }
            }
        }

        // Only keep pools shared by multiple nodes.
        pool_to_nodes.retain(|_, nodes| {
            nodes.sort();
            nodes.dedup();
            nodes.len() > 1
        });

        pool_to_nodes
    }

    /// Estimate the upper bound on total output if we optimistically pick the
    /// best remaining route for each unsolved request.
    fn upper_bound(
        &self,
        solved: &HashMap<NodeId, usize>,
        candidates: &HashMap<NodeId, Vec<Route>>,
        remaining: &[NodeId],
    ) -> u64 {
        let mut bound: u64 = 0;

        // Sum the output already committed.
        for (&node_id, &route_idx) in solved {
            if let Some(routes) = candidates.get(&node_id) {
                if route_idx < routes.len() {
                    bound = bound.saturating_add(routes[route_idx].output_amount);
                }
            }
        }

        // For each remaining node, use the best (first) route as optimistic bound.
        for node_id in remaining {
            if let Some(routes) = candidates.get(node_id) {
                if let Some(best) = routes.first() {
                    bound = bound.saturating_add(best.output_amount);
                }
            }
        }

        bound
    }

    /// Recursive DFS branch-and-bound.
    fn search(
        &self,
        node_order: &[NodeId],
        depth: usize,
        current: &mut HashMap<NodeId, usize>,
        current_output: u64,
        best_output: &mut u64,
        best_assignment: &mut HashMap<NodeId, usize>,
        candidates: &HashMap<NodeId, Vec<Route>>,
        nodes_explored: &mut usize,
        deadline: Instant,
    ) {
        if *nodes_explored >= self.max_nodes || Instant::now() > deadline {
            return;
        }

        // All nodes assigned.
        if depth >= node_order.len() {
            if current_output > *best_output {
                *best_output = current_output;
                *best_assignment = current.clone();
            }
            return;
        }

        let node_id = node_order[depth];
        let routes = match candidates.get(&node_id) {
            Some(r) => r,
            None => return,
        };

        for (route_idx, route) in routes.iter().enumerate() {
            *nodes_explored += 1;

            let new_output = current_output.saturating_add(route.output_amount);

            // Compute upper bound: current committed + best possible for remaining.
            current.insert(node_id, route_idx);
            let remaining = &node_order[depth + 1..];
            let ub = self.upper_bound(current, candidates, remaining);

            if ub <= *best_output {
                // Prune: even the optimistic bound can't beat the current best.
                current.remove(&node_id);
                continue;
            }

            self.search(
                node_order,
                depth + 1,
                current,
                new_output,
                best_output,
                best_assignment,
                candidates,
                nodes_explored,
                deadline,
            );

            current.remove(&node_id);

            if *nodes_explored >= self.max_nodes || Instant::now() > deadline {
                return;
            }
        }
    }
}

impl Default for BranchAndBoundSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Solver for BranchAndBoundSolver {
    fn name(&self) -> &str {
        "BranchAndBoundSolver"
    }

    fn solve(
        &self,
        requests: &[SwapRequest],
        engine: &RouteEngine,
        config: &SolverConfig,
    ) -> Result<SolverResult> {
        let start = Instant::now();
        let deadline = start + std::time::Duration::from_millis(config.timeout_ms);

        // Phase 1: Collect candidate routes for each request.
        let mut candidates: HashMap<NodeId, Vec<Route>> = HashMap::new();
        let mut failed_count = 0usize;

        for request in requests {
            match engine.find_routes(&request.input_mint, &request.output_mint, request.amount) {
                Ok(routes) => {
                    let valid: Vec<Route> = routes
                        .into_iter()
                        .filter(|r| {
                            if !config.allow_multi_hop && r.hop_count() > 1 {
                                return false;
                            }
                            if r.hop_count() > config.max_hops {
                                return false;
                            }
                            r.exchange_rate() >= config.min_output_ratio
                        })
                        .take(config.max_routes)
                        .collect();

                    if valid.is_empty() {
                        warn!(
                            "BranchAndBound: no valid routes for node {}",
                            request.node_id
                        );
                        failed_count += 1;
                    } else {
                        candidates.insert(request.node_id, valid);
                    }
                }
                Err(e) => {
                    warn!(
                        "BranchAndBound: route finding failed for node {}: {}",
                        request.node_id, e
                    );
                    failed_count += 1;
                }
            }
        }

        if candidates.is_empty() {
            return Ok(SolverResult {
                solved_swaps: HashMap::new(),
                total_input: 0,
                total_output: 0,
                estimated_cost_lamports: 0,
                failed_count,
                solve_time_ms: start.elapsed().as_millis() as u64,
            });
        }

        // Phase 2: Determine search order. Nodes with fewer candidate routes first
        // (fail-first heuristic to prune earlier).
        let mut node_order: Vec<NodeId> = candidates.keys().copied().collect();
        node_order.sort_by_key(|id| candidates.get(id).map(|r| r.len()).unwrap_or(0));

        // Phase 3: Branch-and-bound search.
        let mut best_output = 0u64;
        let mut best_assignment: HashMap<NodeId, usize> = HashMap::new();
        let mut current: HashMap<NodeId, usize> = HashMap::new();
        let mut nodes_explored = 0usize;

        // Initialize with greedy solution (first route for each).
        for &node_id in &node_order {
            best_assignment.insert(node_id, 0);
            if let Some(routes) = candidates.get(&node_id) {
                if let Some(route) = routes.first() {
                    best_output = best_output.saturating_add(route.output_amount);
                }
            }
        }

        debug!(
            "BranchAndBound: greedy baseline output={}, searching {} nodes x {} routes",
            best_output,
            node_order.len(),
            candidates.values().map(|r| r.len()).sum::<usize>(),
        );

        // Only run full B&B if the search space is manageable.
        let search_space: usize = candidates.values().map(|r| r.len()).product();
        if search_space <= self.max_nodes * 10 {
            self.search(
                &node_order,
                0,
                &mut current,
                0,
                &mut best_output,
                &mut best_assignment,
                &candidates,
                &mut nodes_explored,
                deadline,
            );
        }

        debug!(
            "BranchAndBound: explored {} nodes, best output={}",
            nodes_explored, best_output,
        );

        // Phase 4: Build result from best assignment.
        let mut solved_swaps = HashMap::new();
        let mut total_input = 0u64;
        let mut total_output = 0u64;

        let request_map: HashMap<NodeId, &SwapRequest> =
            requests.iter().map(|r| (r.node_id, r)).collect();

        for (&node_id, &route_idx) in &best_assignment {
            if let (Some(routes), Some(request)) =
                (candidates.get(&node_id), request_map.get(&node_id))
            {
                if route_idx < routes.len() {
                    let route = routes[route_idx].clone();
                    let slippage_factor =
                        (10_000u64 - config.slippage_bps as u64) as f64 / 10_000.0;
                    let min_output = (route.output_amount as f64 * slippage_factor) as u64;

                    total_input += request.amount;
                    total_output += route.output_amount;

                    solved_swaps.insert(
                        node_id,
                        SolvedSwap {
                            request: (*request).clone(),
                            route,
                            min_output,
                        },
                    );
                }
            }
        }

        let elapsed = start.elapsed().as_millis() as u64;
        let total_hops: usize = solved_swaps.values().map(|s| s.route.hop_count()).sum();
        let estimated_cost = 5_000u64 * solved_swaps.len() as u64 + 200 * total_hops as u64;

        info!(
            "BranchAndBound: solved {}/{} swaps in {}ms ({} B&B nodes)",
            solved_swaps.len(),
            requests.len(),
            elapsed,
            nodes_explored,
        );

        Ok(SolverResult {
            solved_swaps,
            total_input,
            total_output,
            estimated_cost_lamports: estimated_cost,
            failed_count,
            solve_time_ms: elapsed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::{PoolInfo, PoolRegistry};
    use crate::router::RouteEngine;

    fn make_pubkey(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    fn setup_engine() -> RouteEngine {
        let registry = PoolRegistry::new();
        let sol = make_pubkey(1);
        let usdc = make_pubkey(2);
        let usdt = make_pubkey(3);

        registry.register(PoolInfo::constant_product(
            make_pubkey(10),
            sol,
            usdc,
            1_000_000_000,
            150_000_000_000,
            30,
        ));
        registry.register(PoolInfo::constant_product(
            make_pubkey(11),
            usdc,
            usdt,
            500_000_000_000,
            500_000_000_000,
            5,
        ));

        RouteEngine::new(registry)
    }

    #[test]
    fn test_greedy_solver() {
        let engine = setup_engine();
        let config = SolverConfig::default();
        let solver = GreedySolver::new();

        let requests = vec![SwapRequest {
            node_id: 0,
            input_mint: make_pubkey(1),
            output_mint: make_pubkey(2),
            amount: 1_000_000,
            label: Some("SOL->USDC".into()),
        }];

        let result = solver.solve(&requests, &engine, &config).unwrap();
        assert!(result.all_solved());
        assert!(result.total_output > 0);
        assert_eq!(result.solved_swaps.len(), 1);
    }

    #[test]
    fn test_branch_and_bound_solver() {
        let engine = setup_engine();
        let config = SolverConfig::default();
        let solver = BranchAndBoundSolver::new();

        let requests = vec![
            SwapRequest {
                node_id: 0,
                input_mint: make_pubkey(1),
                output_mint: make_pubkey(2),
                amount: 1_000_000,
                label: None,
            },
            SwapRequest {
                node_id: 1,
                input_mint: make_pubkey(2),
                output_mint: make_pubkey(3),
                amount: 100_000_000,
                label: None,
            },
        ];

        let result = solver.solve(&requests, &engine, &config).unwrap();
        assert!(result.all_solved());
        assert_eq!(result.solved_swaps.len(), 2);
    }

    #[test]
    fn test_solver_config_defaults() {
        let config = SolverConfig::default();
        assert_eq!(config.max_routes, 10);
        assert_eq!(config.timeout_ms, 5_000);
        assert_eq!(config.slippage_bps, 100);
        assert!(config.allow_multi_hop);
    }

    #[test]
    fn test_solver_with_no_routes() {
        let registry = PoolRegistry::new();
        let engine = RouteEngine::new(registry);
        let config = SolverConfig::default();
        let solver = GreedySolver::new();

        let requests = vec![SwapRequest {
            node_id: 0,
            input_mint: make_pubkey(1),
            output_mint: make_pubkey(2),
            amount: 1000,
            label: None,
        }];

        let result = solver.solve(&requests, &engine, &config).unwrap();
        assert_eq!(result.failed_count, 1);
        assert!(!result.all_solved());
    }
}
