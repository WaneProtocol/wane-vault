//! Route finding engine for token swaps through liquidity pools.
//!
//! Uses Dijkstra's algorithm on a `PoolGraph` to find the cheapest (highest-output)
//! path from an input token to an output token, supporting multi-hop routes.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fmt;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info};

use crate::pool::{
    calculate_clmm_output, calculate_output, calculate_price_impact, PoolRegistry, PoolType,
};

/// Maximum number of hops in a single route.
const MAX_HOPS: usize = 4;

/// Maximum number of candidate routes to evaluate.
const MAX_CANDIDATE_ROUTES: usize = 20;

/// A single hop in a swap route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteHop {
    /// Pool address used for this hop.
    pub pool_address: Pubkey,
    /// Input token mint for this hop.
    pub input_mint: Pubkey,
    /// Output token mint for this hop.
    pub output_mint: Pubkey,
    /// Estimated input amount for this hop.
    pub input_amount: u64,
    /// Estimated output amount from this hop.
    pub output_amount: u64,
    /// Fee in basis points charged by this pool.
    pub fee_bps: u16,
    /// Pool type.
    pub pool_type: PoolType,
    /// Price impact for this hop as a fraction (0.0-1.0).
    pub price_impact: f64,
}

impl fmt::Display for RouteHop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Hop({} -> {}, pool={}, in={}, out={}, impact={:.4}%)",
            &self.input_mint.to_string()[..8],
            &self.output_mint.to_string()[..8],
            &self.pool_address.to_string()[..8],
            self.input_amount,
            self.output_amount,
            self.price_impact * 100.0,
        )
    }
}

/// A complete route from input token to output token, possibly multi-hop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    /// Ordered sequence of hops.
    pub hops: Vec<RouteHop>,
    /// Total input amount (into the first hop).
    pub input_amount: u64,
    /// Total output amount (out of the last hop).
    pub output_amount: u64,
    /// Input token mint.
    pub input_mint: Pubkey,
    /// Output token mint.
    pub output_mint: Pubkey,
    /// Aggregate price impact across all hops.
    pub total_price_impact: f64,
    /// Aggregate fee in basis points (approximate).
    pub total_fee_bps: u16,
    /// Route score: higher is better. Based on output, impact, and depth.
    pub score: f64,
}

impl Route {
    /// Create a new route from a sequence of hops. Computes aggregate metrics.
    pub fn from_hops(hops: Vec<RouteHop>) -> Self {
        if hops.is_empty() {
            return Self {
                hops: Vec::new(),
                input_amount: 0,
                output_amount: 0,
                input_mint: Pubkey::default(),
                output_mint: Pubkey::default(),
                total_price_impact: 0.0,
                total_fee_bps: 0,
                score: 0.0,
            };
        }

        let input_amount = hops[0].input_amount;
        let output_amount = hops.last().map(|h| h.output_amount).unwrap_or(0);
        let input_mint = hops[0].input_mint;
        let output_mint = hops.last().map(|h| h.output_mint).unwrap_or_default();

        // Aggregate price impact: 1 - product(1 - impact_i)
        let surviving = hops
            .iter()
            .map(|h| 1.0 - h.price_impact)
            .fold(1.0, |acc, s| acc * s);
        let total_price_impact = (1.0 - surviving).max(0.0);

        // Aggregate fee: sum of fees (approximate; ignores compounding).
        let total_fee_bps: u16 = hops.iter().map(|h| h.fee_bps).sum::<u16>().min(10_000);

        // Score: output normalized by input, penalized by hops and impact.
        let hop_penalty = 0.995_f64.powi(hops.len() as i32 - 1);
        let score = if input_amount > 0 {
            (output_amount as f64 / input_amount as f64) * hop_penalty * (1.0 - total_price_impact)
        } else {
            0.0
        };

        Self {
            hops,
            input_amount,
            output_amount,
            input_mint,
            output_mint,
            total_price_impact,
            total_fee_bps,
            score,
        }
    }

    /// Number of hops in the route.
    pub fn hop_count(&self) -> usize {
        self.hops.len()
    }

    /// Returns the sequence of token mints visited (including start and end).
    pub fn token_path(&self) -> Vec<Pubkey> {
        let mut path = vec![self.input_mint];
        for hop in &self.hops {
            path.push(hop.output_mint);
        }
        path
    }

    /// Returns all pool addresses used in this route.
    pub fn pool_addresses(&self) -> Vec<Pubkey> {
        self.hops.iter().map(|h| h.pool_address).collect()
    }

    /// Effective exchange rate (output per unit input).
    pub fn exchange_rate(&self) -> f64 {
        if self.input_amount == 0 {
            return 0.0;
        }
        self.output_amount as f64 / self.input_amount as f64
    }
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Route({} hops, in={}, out={}, impact={:.4}%, score={:.6})",
            self.hops.len(),
            self.input_amount,
            self.output_amount,
            self.total_price_impact * 100.0,
            self.score,
        )
    }
}

// ---------------------------------------------------------------------------
// Dijkstra state
// ---------------------------------------------------------------------------

/// State entry for Dijkstra's priority queue.  We maximize output, so we
/// negate the output for min-heap ordering.
#[derive(Debug, Clone)]
struct DijkstraState {
    /// Current token.
    token: Pubkey,
    /// Output amount reaching this token.
    amount: u64,
    /// Hops taken so far.
    hops: Vec<RouteHop>,
}

impl Eq for DijkstraState {}

impl PartialEq for DijkstraState {
    fn eq(&self, other: &Self) -> bool {
        self.amount == other.amount
    }
}

impl Ord for DijkstraState {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse: we want the *largest* amount first in a max-heap.
        self.amount.cmp(&other.amount)
    }
}

impl PartialOrd for DijkstraState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ---------------------------------------------------------------------------
// RouteEngine
// ---------------------------------------------------------------------------

/// Configuration for the route engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    /// Maximum number of hops per route.
    pub max_hops: usize,
    /// Maximum number of routes to return from find_routes.
    pub max_routes: usize,
    /// Minimum output amount to consider a route valid (in output token units).
    pub min_output: u64,
    /// Maximum acceptable price impact (0.0 to 1.0).
    pub max_price_impact: f64,
    /// Whether to include CLMM pools.
    pub include_clmm: bool,
    /// Whether to include orderbook pools.
    pub include_orderbook: bool,
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            max_hops: MAX_HOPS,
            max_routes: MAX_CANDIDATE_ROUTES,
            min_output: 1,
            max_price_impact: 0.50,
            include_clmm: true,
            include_orderbook: true,
        }
    }
}

/// The main route-finding engine.
pub struct RouteEngine {
    /// Pool registry for looking up pool data.
    pub registry: PoolRegistry,
    /// Configuration.
    pub config: RouteConfig,
}

impl RouteEngine {
    pub fn new(registry: PoolRegistry) -> Self {
        Self {
            registry,
            config: RouteConfig::default(),
        }
    }

    pub fn with_config(mut self, config: RouteConfig) -> Self {
        self.config = config;
        self
    }

    /// Find multiple routes from input_mint to output_mint for the given amount.
    ///
    /// Uses a modified Dijkstra's algorithm that explores multiple paths and
    /// returns them sorted by score (best first).
    pub fn find_routes(
        &self,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        amount: u64,
    ) -> Result<Vec<Route>> {
        if input_mint == output_mint {
            return Err(anyhow!("Input and output mints are the same"));
        }
        if amount == 0 {
            return Err(anyhow!("Amount must be greater than zero"));
        }

        let graph = self.registry.build_graph();

        if !graph.tokens.contains(input_mint) {
            return Err(anyhow!("Input mint {} not found in pool graph", input_mint));
        }
        if !graph.tokens.contains(output_mint) {
            return Err(anyhow!(
                "Output mint {} not found in pool graph",
                output_mint
            ));
        }

        let mut completed_routes: Vec<Route> = Vec::new();

        // Dijkstra-like BFS with a max-heap on output amount.
        let mut heap = BinaryHeap::new();
        heap.push(DijkstraState {
            token: *input_mint,
            amount,
            hops: Vec::new(),
        });

        // Track the best amount seen at each (token, hop_count) to prune.
        let mut best_at: HashMap<(Pubkey, usize), u64> = HashMap::new();
        best_at.insert((*input_mint, 0), amount);

        let mut iterations = 0u32;
        let max_iterations = 5_000u32;

        while let Some(state) = heap.pop() {
            iterations += 1;
            if iterations > max_iterations {
                debug!("Route search hit iteration limit");
                break;
            }

            if completed_routes.len() >= self.config.max_routes {
                break;
            }

            // If we've reached the output token, record the route.
            if state.token == *output_mint && !state.hops.is_empty() {
                let route = Route::from_hops(state.hops);
                if route.output_amount >= self.config.min_output
                    && route.total_price_impact <= self.config.max_price_impact
                {
                    completed_routes.push(route);
                }
                continue;
            }

            if state.hops.len() >= self.config.max_hops {
                continue;
            }

            // Explore neighbors.
            let edges = match graph.adjacency.get(&state.token) {
                Some(e) => e,
                None => continue,
            };

            // Avoid revisiting tokens already in the path (no cycles).
            let visited: HashSet<Pubkey> = state
                .hops
                .iter()
                .map(|h| h.input_mint)
                .chain(std::iter::once(state.token))
                .collect();

            for edge in edges {
                if visited.contains(&edge.token_out) {
                    continue;
                }

                // Filter by pool type.
                if !self.config.include_clmm && edge.pool_type == PoolType::Clmm {
                    continue;
                }
                if !self.config.include_orderbook && edge.pool_type == PoolType::Orderbook {
                    continue;
                }

                // Compute output for this edge.
                let output = self.compute_edge_output(edge, state.amount);
                if output == 0 {
                    continue;
                }

                // Prune if we've already found a better path to this token at this depth.
                let key = (edge.token_out, state.hops.len() + 1);
                let prev_best = best_at.get(&key).copied().unwrap_or(0);
                if output <= prev_best {
                    continue;
                }
                best_at.insert(key, output);

                let impact = calculate_price_impact(
                    edge.reserve_in,
                    edge.reserve_out,
                    state.amount,
                    edge.fee_bps,
                );

                let hop = RouteHop {
                    pool_address: edge.pool_address,
                    input_mint: edge.token_in,
                    output_mint: edge.token_out,
                    input_amount: state.amount,
                    output_amount: output,
                    fee_bps: edge.fee_bps,
                    pool_type: edge.pool_type,
                    price_impact: impact,
                };

                let mut new_hops = state.hops.clone();
                new_hops.push(hop);

                heap.push(DijkstraState {
                    token: edge.token_out,
                    amount: output,
                    hops: new_hops,
                });
            }
        }

        // Sort by score descending.
        completed_routes.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

        info!(
            "Found {} routes from {} to {} for amount {}",
            completed_routes.len(),
            &input_mint.to_string()[..8],
            &output_mint.to_string()[..8],
            amount,
        );

        Ok(completed_routes)
    }

    /// Find the single best route (highest score).
    pub fn find_best_route(
        &self,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        amount: u64,
    ) -> Result<Route> {
        let routes = self.find_routes(input_mint, output_mint, amount)?;
        routes.into_iter().next().ok_or_else(|| {
            anyhow!(
                "No valid route found from {} to {}",
                input_mint,
                output_mint
            )
        })
    }

    /// Compute the output for a single pool edge.
    fn compute_edge_output(&self, edge: &crate::pool::PoolEdge, amount_in: u64) -> u64 {
        match edge.pool_type {
            PoolType::ConstantProduct | PoolType::Orderbook => {
                calculate_output(edge.reserve_in, edge.reserve_out, amount_in, edge.fee_bps)
            }
            PoolType::Clmm => {
                // For CLMM we need the full pool info with tick ranges.
                if let Some(pool) = self.registry.get(&edge.pool_address) {
                    if pool.tick_ranges.is_empty() {
                        // Fall back to constant-product approximation.
                        return calculate_output(
                            edge.reserve_in,
                            edge.reserve_out,
                            amount_in,
                            edge.fee_bps,
                        );
                    }
                    let a_to_b = edge.token_in == pool.token_a;
                    let (out, _) = calculate_clmm_output(
                        &pool.tick_ranges,
                        pool.current_tick,
                        amount_in,
                        a_to_b,
                        pool.fee_bps,
                    );
                    out
                } else {
                    calculate_output(edge.reserve_in, edge.reserve_out, amount_in, edge.fee_bps)
                }
            }
        }
    }

    /// Score a route considering output amount, price impact, hop count, and pool depth.
    pub fn score_route(&self, route: &Route) -> f64 {
        if route.input_amount == 0 {
            return 0.0;
        }

        let output_ratio = route.output_amount as f64 / route.input_amount as f64;
        let hop_penalty = 0.995_f64.powi(route.hop_count() as i32 - 1);
        let impact_factor = 1.0 - route.total_price_impact;

        // Pool depth factor: penalize routes through shallow pools.
        let depth_factor = route
            .hops
            .iter()
            .map(|hop| {
                let pool = self.registry.get(&hop.pool_address);
                match pool {
                    Some(p) => {
                        let depth = p.reserve_a.min(p.reserve_b) as f64;
                        let ratio = hop.input_amount as f64 / depth.max(1.0);
                        // If the trade is a large fraction of the pool, penalize.
                        (1.0 - ratio * 0.5).max(0.1)
                    }
                    None => 0.5,
                }
            })
            .fold(1.0, |acc, f| acc * f);

        output_ratio * hop_penalty * impact_factor * depth_factor
    }

    /// Re-quote a route with potentially updated pool reserves.
    pub fn requote_route(&self, route: &Route) -> Result<Route> {
        let mut current_amount = route.input_amount;
        let mut new_hops = Vec::with_capacity(route.hop_count());

        for hop in &route.hops {
            let pool = self
                .registry
                .get(&hop.pool_address)
                .ok_or_else(|| anyhow!("Pool {} no longer available", hop.pool_address))?;

            let (reserve_in, reserve_out) = if hop.input_mint == pool.token_a {
                (pool.reserve_a, pool.reserve_b)
            } else {
                (pool.reserve_b, pool.reserve_a)
            };

            let output = calculate_output(reserve_in, reserve_out, current_amount, pool.fee_bps);
            if output == 0 {
                return Err(anyhow!(
                    "Route hop through pool {} now returns zero output",
                    hop.pool_address
                ));
            }

            let impact =
                calculate_price_impact(reserve_in, reserve_out, current_amount, pool.fee_bps);

            new_hops.push(RouteHop {
                pool_address: hop.pool_address,
                input_mint: hop.input_mint,
                output_mint: hop.output_mint,
                input_amount: current_amount,
                output_amount: output,
                fee_bps: pool.fee_bps,
                pool_type: pool.pool_type,
                price_impact: impact,
            });

            current_amount = output;
        }

        Ok(Route::from_hops(new_hops))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pool::PoolInfo;

    fn make_pubkey(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    fn setup_registry() -> PoolRegistry {
        let registry = PoolRegistry::new();

        let sol = make_pubkey(1);
        let usdc = make_pubkey(2);
        let usdt = make_pubkey(3);
        let ray = make_pubkey(4);

        // SOL/USDC pool with decent liquidity.
        registry.register(PoolInfo::constant_product(
            make_pubkey(10),
            sol,
            usdc,
            1_000_000_000,   // 1B SOL
            150_000_000_000, // 150B USDC (price ~150)
            30,
        ));

        // USDC/USDT pool (stablecoin, tight spread).
        registry.register(PoolInfo::constant_product(
            make_pubkey(11),
            usdc,
            usdt,
            500_000_000_000,
            500_000_000_000,
            5,
        ));

        // SOL/RAY pool.
        registry.register(PoolInfo::constant_product(
            make_pubkey(12),
            sol,
            ray,
            2_000_000_000,
            10_000_000_000,
            30,
        ));

        // RAY/USDC pool.
        registry.register(PoolInfo::constant_product(
            make_pubkey(13),
            ray,
            usdc,
            5_000_000_000,
            3_750_000_000,
            30,
        ));

        registry
    }

    #[test]
    fn test_find_direct_route() {
        let registry = setup_registry();
        let engine = RouteEngine::new(registry);

        let sol = make_pubkey(1);
        let usdc = make_pubkey(2);

        let routes = engine.find_routes(&sol, &usdc, 1_000_000).unwrap();
        assert!(!routes.is_empty());

        let best = &routes[0];
        assert_eq!(best.input_mint, sol);
        assert_eq!(best.output_mint, usdc);
        assert!(best.output_amount > 0);
        assert!(best.hop_count() >= 1);
    }

    #[test]
    fn test_find_multihop_route() {
        let registry = setup_registry();
        let engine = RouteEngine::new(registry);

        let sol = make_pubkey(1);
        let usdt = make_pubkey(3);

        // SOL -> USDC -> USDT is a 2-hop route.
        let routes = engine.find_routes(&sol, &usdt, 1_000_000).unwrap();
        assert!(!routes.is_empty());

        // At least one route should be multi-hop.
        let has_multihop = routes.iter().any(|r| r.hop_count() >= 2);
        assert!(has_multihop);
    }

    #[test]
    fn test_best_route() {
        let registry = setup_registry();
        let engine = RouteEngine::new(registry);

        let sol = make_pubkey(1);
        let usdc = make_pubkey(2);

        let best = engine.find_best_route(&sol, &usdc, 1_000_000).unwrap();
        assert!(best.output_amount > 0);
        assert!(best.score > 0.0);
    }

    #[test]
    fn test_no_route() {
        let registry = PoolRegistry::new();
        // Only register one pool.
        registry.register(PoolInfo::constant_product(
            make_pubkey(10),
            make_pubkey(1),
            make_pubkey(2),
            1000,
            2000,
            30,
        ));
        let engine = RouteEngine::new(registry);

        // Try to route between unconnected tokens.
        let result = engine.find_routes(&make_pubkey(1), &make_pubkey(99), 1000);
        assert!(result.is_err() || result.unwrap().is_empty());
    }

    #[test]
    fn test_routes_sorted_by_score() {
        let registry = setup_registry();
        let engine = RouteEngine::new(registry);

        let sol = make_pubkey(1);
        let usdt = make_pubkey(3);

        let routes = engine.find_routes(&sol, &usdt, 1_000_000).unwrap();
        for window in routes.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn test_requote_route() {
        let registry = setup_registry();
        let engine = RouteEngine::new(registry);

        let sol = make_pubkey(1);
        let usdc = make_pubkey(2);

        let original = engine.find_best_route(&sol, &usdc, 1_000_000).unwrap();
        let requoted = engine.requote_route(&original).unwrap();

        // Same reserves, so requote should give the same output.
        assert_eq!(original.output_amount, requoted.output_amount);
    }
}
