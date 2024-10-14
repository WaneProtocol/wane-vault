//! Liquidity pool registry, graph representation, and AMM math.
//!
//! This module provides:
//! - `PoolInfo`: metadata for a single liquidity pool (reserves, fees, type).
//! - `PoolGraph`: a graph where nodes are token mints and edges are pools.
//! - Constant-product AMM math (`calculate_output`, `calculate_price_impact`, `calculate_optimal_split`).
//! - Concentrated liquidity (CLMM) math with tick-range calculations.
//! - `PoolRegistry`: a concurrent registry of known pools backed by `DashMap`.
//! - `PoolFetcher`: simulated on-chain pool data fetcher.

use std::collections::{HashMap, HashSet};
use std::fmt;

use anyhow::{anyhow, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info, warn};

/// The type of automated market maker a pool uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PoolType {
    /// Constant-product AMM (x * y = k).
    ConstantProduct,
    /// Concentrated liquidity market maker with tick ranges.
    Clmm,
    /// Central-limit order book.
    Orderbook,
}

impl fmt::Display for PoolType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PoolType::ConstantProduct => write!(f, "ConstantProduct"),
            PoolType::Clmm => write!(f, "CLMM"),
            PoolType::Orderbook => write!(f, "Orderbook"),
        }
    }
}

/// A single tick range within a CLMM pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickRange {
    /// Lower tick boundary (inclusive).
    pub tick_lower: i32,
    /// Upper tick boundary (exclusive).
    pub tick_upper: i32,
    /// Liquidity available within this range.
    pub liquidity: u128,
}

impl TickRange {
    pub fn new(tick_lower: i32, tick_upper: i32, liquidity: u128) -> Self {
        Self {
            tick_lower,
            tick_upper,
            liquidity,
        }
    }

    /// Returns the price at a given tick index.  price = 1.0001^tick.
    pub fn price_at_tick(tick: i32) -> f64 {
        1.0001_f64.powi(tick)
    }

    /// Returns the sqrt-price at a given tick.
    pub fn sqrt_price_at_tick(tick: i32) -> f64 {
        1.0001_f64.powi(tick).sqrt()
    }

    /// Compute the amount of token_a available in this range given current sqrt_price.
    /// Formula: delta_a = L * (1/sqrt_p_lower - 1/sqrt_p_upper)
    pub fn token_a_amount(&self, current_sqrt_price: f64) -> f64 {
        let sqrt_lower = Self::sqrt_price_at_tick(self.tick_lower);
        let sqrt_upper = Self::sqrt_price_at_tick(self.tick_upper);

        let effective_lower = sqrt_lower.max(current_sqrt_price.min(sqrt_upper));
        let effective_upper = sqrt_upper;

        if effective_lower >= effective_upper {
            return 0.0;
        }

        self.liquidity as f64 * (1.0 / effective_lower - 1.0 / effective_upper)
    }

    /// Compute the amount of token_b available in this range given current sqrt_price.
    /// Formula: delta_b = L * (sqrt_p_upper - sqrt_p_lower)
    pub fn token_b_amount(&self, current_sqrt_price: f64) -> f64 {
        let sqrt_lower = Self::sqrt_price_at_tick(self.tick_lower);
        let sqrt_upper = Self::sqrt_price_at_tick(self.tick_upper);

        let effective_lower = sqrt_lower;
        let effective_upper = sqrt_upper.min(current_sqrt_price.max(sqrt_lower));

        if effective_lower >= effective_upper {
            return 0.0;
        }

        self.liquidity as f64 * (effective_upper - effective_lower)
    }
}

/// Metadata for a single liquidity pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    /// On-chain address of the pool.
    pub address: Pubkey,
    /// Mint of token A.
    pub token_a: Pubkey,
    /// Mint of token B.
    pub token_b: Pubkey,
    /// Reserve of token A (in smallest units).
    pub reserve_a: u64,
    /// Reserve of token B (in smallest units).
    pub reserve_b: u64,
    /// Trading fee in basis points (e.g. 30 = 0.30%).
    pub fee_bps: u16,
    /// AMM type.
    pub pool_type: PoolType,
    /// Tick ranges for CLMM pools (empty for other types).
    pub tick_ranges: Vec<TickRange>,
    /// Current tick index for CLMM pools.
    pub current_tick: i32,
    /// Decimals for token A.
    pub decimals_a: u8,
    /// Decimals for token B.
    pub decimals_b: u8,
    /// Whether the pool is currently active.
    pub is_active: bool,
    /// Last updated slot.
    pub last_update_slot: u64,
}

impl PoolInfo {
    /// Create a constant-product pool.
    pub fn constant_product(
        address: Pubkey,
        token_a: Pubkey,
        token_b: Pubkey,
        reserve_a: u64,
        reserve_b: u64,
        fee_bps: u16,
    ) -> Self {
        Self {
            address,
            token_a,
            token_b,
            reserve_a,
            reserve_b,
            fee_bps,
            pool_type: PoolType::ConstantProduct,
            tick_ranges: Vec::new(),
            current_tick: 0,
            decimals_a: 9,
            decimals_b: 9,
            is_active: true,
            last_update_slot: 0,
        }
    }

    /// Create a CLMM pool.
    pub fn clmm(
        address: Pubkey,
        token_a: Pubkey,
        token_b: Pubkey,
        fee_bps: u16,
        current_tick: i32,
        tick_ranges: Vec<TickRange>,
    ) -> Self {
        Self {
            address,
            token_a,
            token_b,
            reserve_a: 0,
            reserve_b: 0,
            fee_bps,
            pool_type: PoolType::Clmm,
            tick_ranges,
            current_tick,
            decimals_a: 9,
            decimals_b: 9,
            is_active: true,
            last_update_slot: 0,
        }
    }

    /// Create an orderbook pool.
    pub fn orderbook(
        address: Pubkey,
        token_a: Pubkey,
        token_b: Pubkey,
        reserve_a: u64,
        reserve_b: u64,
        fee_bps: u16,
    ) -> Self {
        Self {
            address,
            token_a,
            token_b,
            reserve_a,
            reserve_b,
            fee_bps,
            pool_type: PoolType::Orderbook,
            tick_ranges: Vec::new(),
            current_tick: 0,
            decimals_a: 9,
            decimals_b: 9,
            is_active: true,
            last_update_slot: 0,
        }
    }

    pub fn with_decimals(mut self, decimals_a: u8, decimals_b: u8) -> Self {
        self.decimals_a = decimals_a;
        self.decimals_b = decimals_b;
        self
    }

    pub fn with_last_update_slot(mut self, slot: u64) -> Self {
        self.last_update_slot = slot;
        self
    }

    /// Returns the pair of tokens this pool trades between, sorted for canonical ordering.
    pub fn token_pair(&self) -> (Pubkey, Pubkey) {
        if self.token_a < self.token_b {
            (self.token_a, self.token_b)
        } else {
            (self.token_b, self.token_a)
        }
    }

    /// Returns the other token given one side of the pair.
    pub fn other_token(&self, token: &Pubkey) -> Option<Pubkey> {
        if *token == self.token_a {
            Some(self.token_b)
        } else if *token == self.token_b {
            Some(self.token_a)
        } else {
            None
        }
    }

    /// Spot price of token_a denominated in token_b (how much B per 1 A).
    pub fn spot_price_a_to_b(&self) -> f64 {
        if self.reserve_a == 0 {
            return 0.0;
        }
        self.reserve_b as f64 / self.reserve_a as f64
    }

    /// Spot price of token_b denominated in token_a.
    pub fn spot_price_b_to_a(&self) -> f64 {
        if self.reserve_b == 0 {
            return 0.0;
        }
        self.reserve_a as f64 / self.reserve_b as f64
    }

    /// The constant product k = reserve_a * reserve_b.
    pub fn invariant_k(&self) -> u128 {
        self.reserve_a as u128 * self.reserve_b as u128
    }

    /// Total value locked (in token B units), approximated as 2 * reserve_b.
    pub fn tvl_in_token_b(&self) -> u64 {
        self.reserve_b.saturating_mul(2)
    }
}

impl fmt::Display for PoolInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Pool({}, {}, A={}, B={}, fee={}bps)",
            &self.address.to_string()[..8],
            self.pool_type,
            self.reserve_a,
            self.reserve_b,
            self.fee_bps,
        )
    }
}

// ---------------------------------------------------------------------------
// Constant-product AMM math
// ---------------------------------------------------------------------------

/// Calculate the output amount for a constant-product swap.
///
/// Given input amount `amount_in` of token X with reserves (reserve_in, reserve_out)
/// and a fee in basis points, computes the output of token Y.
///
/// Formula: out = (reserve_out * amount_in_after_fee) / (reserve_in + amount_in_after_fee)
pub fn calculate_output(reserve_in: u64, reserve_out: u64, amount_in: u64, fee_bps: u16) -> u64 {
    if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
        return 0;
    }

    let fee_factor = 10_000u128 - fee_bps as u128;
    let amount_in_after_fee = amount_in as u128 * fee_factor;
    let numerator = reserve_out as u128 * amount_in_after_fee;
    let denominator = reserve_in as u128 * 10_000u128 + amount_in_after_fee;

    if denominator == 0 {
        return 0;
    }

    (numerator / denominator) as u64
}

/// Calculate the input amount required to receive exactly `amount_out` tokens.
///
/// Inverse of `calculate_output`.
pub fn calculate_input_for_output(
    reserve_in: u64,
    reserve_out: u64,
    amount_out: u64,
    fee_bps: u16,
) -> u64 {
    if reserve_in == 0 || reserve_out == 0 || amount_out == 0 || amount_out >= reserve_out {
        return u64::MAX;
    }

    let fee_factor = 10_000u128 - fee_bps as u128;
    let numerator = reserve_in as u128 * amount_out as u128 * 10_000u128;
    let denominator = (reserve_out as u128 - amount_out as u128) * fee_factor;

    if denominator == 0 {
        return u64::MAX;
    }

    // Ceiling division to ensure we provide enough input.
    ((numerator + denominator - 1) / denominator) as u64
}

/// Calculate price impact as a fraction (0.0 to 1.0).
///
/// Price impact = 1 - (effective_price / spot_price).
pub fn calculate_price_impact(
    reserve_in: u64,
    reserve_out: u64,
    amount_in: u64,
    fee_bps: u16,
) -> f64 {
    if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
        return 0.0;
    }

    let spot_price = reserve_out as f64 / reserve_in as f64;
    let output = calculate_output(reserve_in, reserve_out, amount_in, fee_bps);

    if output == 0 {
        return 1.0;
    }

    let effective_price = output as f64 / amount_in as f64;
    let impact = 1.0 - (effective_price / spot_price);
    impact.max(0.0).min(1.0)
}

/// Find the optimal split of `total_amount` across `n` identical pools to minimize
/// aggregate price impact. Returns the per-pool amount.
///
/// For identical constant-product pools, splitting equally is optimal. For different
/// pools, this uses a gradient-descent-like iterative approach.
pub fn calculate_optimal_split(
    pools: &[(u64, u64, u16)], // (reserve_in, reserve_out, fee_bps) per pool
    total_amount: u64,
) -> Vec<u64> {
    let n = pools.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![total_amount];
    }

    // Start with equal split.
    let mut allocations: Vec<f64> = vec![total_amount as f64 / n as f64; n];
    let total = total_amount as f64;

    // Iterative refinement: move allocation towards pools with better marginal output.
    for _iteration in 0..50 {
        let mut marginal_outputs = Vec::with_capacity(n);
        for (i, &(res_in, res_out, fee)) in pools.iter().enumerate() {
            let alloc = allocations[i] as u64;
            let out_base = calculate_output(res_in, res_out, alloc, fee);
            let out_plus = calculate_output(res_in, res_out, alloc.saturating_add(1), fee);
            let marginal = out_plus as f64 - out_base as f64;
            marginal_outputs.push(marginal);
        }

        let avg_marginal: f64 = marginal_outputs.iter().sum::<f64>() / n as f64;
        if avg_marginal <= 0.0 {
            break;
        }

        let mut max_change: f64 = 0.0;
        let step = total * 0.01;

        for i in 0..n {
            let ratio = if avg_marginal > 0.0 {
                marginal_outputs[i] / avg_marginal
            } else {
                1.0
            };
            let new_alloc = (allocations[i] * ratio).max(0.0);
            let change = (new_alloc - allocations[i]).abs();
            if change > max_change {
                max_change = change;
            }
            allocations[i] = new_alloc;
        }

        // Re-normalize to total.
        let current_sum: f64 = allocations.iter().sum();
        if current_sum > 0.0 {
            for a in &mut allocations {
                *a = *a * total / current_sum;
            }
        }

        if max_change < 1.0 {
            break;
        }
    }

    // Convert to integers and fix rounding.
    let mut result: Vec<u64> = allocations.iter().map(|a| *a as u64).collect();
    let assigned: u64 = result.iter().sum();
    let remainder = total_amount.saturating_sub(assigned);
    if !result.is_empty() {
        result[0] = result[0].saturating_add(remainder);
    }
    result
}

// ---------------------------------------------------------------------------
// CLMM math
// ---------------------------------------------------------------------------

/// Calculate output for a CLMM swap across multiple tick ranges.
///
/// Processes the swap by consuming liquidity from each tick range in sequence,
/// moving the price as liquidity is consumed.
pub fn calculate_clmm_output(
    tick_ranges: &[TickRange],
    current_tick: i32,
    amount_in: u64,
    a_to_b: bool,
    fee_bps: u16,
) -> (u64, i32) {
    let fee_factor = (10_000.0 - fee_bps as f64) / 10_000.0;
    let mut remaining = amount_in as f64 * fee_factor;
    let mut total_output: f64 = 0.0;
    let mut current_sqrt_price = TickRange::sqrt_price_at_tick(current_tick);
    let mut final_tick = current_tick;

    // Sort ranges by tick: ascending for a->b (price decreasing), descending for b->a.
    let mut sorted_ranges: Vec<&TickRange> = tick_ranges
        .iter()
        .filter(|r| {
            if a_to_b {
                r.tick_lower <= current_tick
            } else {
                r.tick_upper > current_tick
            }
        })
        .collect();

    if a_to_b {
        sorted_ranges.sort_by(|a, b| b.tick_lower.cmp(&a.tick_lower));
    } else {
        sorted_ranges.sort_by(|a, b| a.tick_lower.cmp(&b.tick_lower));
    }

    for range in &sorted_ranges {
        if remaining <= 0.0 {
            break;
        }

        let liquidity = range.liquidity as f64;
        if liquidity <= 0.0 {
            continue;
        }

        if a_to_b {
            // Selling token A for token B: price moves down.
            let sqrt_lower = TickRange::sqrt_price_at_tick(range.tick_lower);
            let effective_sqrt = current_sqrt_price.max(sqrt_lower);

            // Maximum amount of A that can be swapped in this range.
            // delta_a = L * (1/sqrt_lower - 1/sqrt_current)
            let max_a = if effective_sqrt > sqrt_lower {
                liquidity * (1.0 / sqrt_lower - 1.0 / effective_sqrt)
            } else {
                0.0
            };

            let consumed = remaining.min(max_a);
            if consumed <= 0.0 {
                continue;
            }

            // New sqrt price after consuming `consumed` of token A.
            // 1/sqrt_new = 1/sqrt_current + consumed/L
            let inv_new = 1.0 / effective_sqrt + consumed / liquidity;
            let new_sqrt = 1.0 / inv_new;

            // Output of token B: delta_b = L * (sqrt_current - sqrt_new)
            let delta_b = liquidity * (effective_sqrt - new_sqrt);
            total_output += delta_b.max(0.0);
            remaining -= consumed;
            current_sqrt_price = new_sqrt;
            final_tick = (new_sqrt * new_sqrt).ln() / 1.0001_f64.ln();
            let final_tick_f = final_tick;
            final_tick = final_tick_f as i32;
        } else {
            // Buying token A with token B: price moves up.
            let sqrt_upper = TickRange::sqrt_price_at_tick(range.tick_upper);
            let effective_sqrt = current_sqrt_price.min(sqrt_upper);

            // Maximum amount of B that can be swapped in this range.
            // delta_b = L * (sqrt_upper - sqrt_current)
            let max_b = if sqrt_upper > effective_sqrt {
                liquidity * (sqrt_upper - effective_sqrt)
            } else {
                0.0
            };

            let consumed = remaining.min(max_b);
            if consumed <= 0.0 {
                continue;
            }

            // New sqrt price.
            let new_sqrt = effective_sqrt + consumed / liquidity;

            // Output of token A: delta_a = L * (1/sqrt_current - 1/sqrt_new)
            let delta_a = liquidity * (1.0 / effective_sqrt - 1.0 / new_sqrt);
            total_output += delta_a.max(0.0);
            remaining -= consumed;
            current_sqrt_price = new_sqrt;
            let price = current_sqrt_price * current_sqrt_price;
            final_tick = (price.ln() / 1.0001_f64.ln()) as i32;
        }
    }

    (total_output as u64, final_tick)
}

// ---------------------------------------------------------------------------
// Pool graph
// ---------------------------------------------------------------------------

/// Edge in the pool graph connecting two token mints via a pool.
#[derive(Debug, Clone)]
pub struct PoolEdge {
    pub pool_address: Pubkey,
    pub token_in: Pubkey,
    pub token_out: Pubkey,
    pub reserve_in: u64,
    pub reserve_out: u64,
    pub fee_bps: u16,
    pub pool_type: PoolType,
}

/// Graph representation where nodes are token mints and edges are pools.
///
/// Enables Dijkstra's shortest path to find optimal swap routes.
#[derive(Debug, Clone, Default)]
pub struct PoolGraph {
    /// Adjacency list: token_mint -> list of pool edges.
    pub adjacency: HashMap<Pubkey, Vec<PoolEdge>>,
    /// Set of all known token mints.
    pub tokens: HashSet<Pubkey>,
}

impl PoolGraph {
    pub fn new() -> Self {
        Self {
            adjacency: HashMap::new(),
            tokens: HashSet::new(),
        }
    }

    /// Add a pool to the graph, creating edges in both directions.
    pub fn add_pool(&mut self, pool: &PoolInfo) {
        self.tokens.insert(pool.token_a);
        self.tokens.insert(pool.token_b);

        // A -> B edge.
        self.adjacency
            .entry(pool.token_a)
            .or_default()
            .push(PoolEdge {
                pool_address: pool.address,
                token_in: pool.token_a,
                token_out: pool.token_b,
                reserve_in: pool.reserve_a,
                reserve_out: pool.reserve_b,
                fee_bps: pool.fee_bps,
                pool_type: pool.pool_type,
            });

        // B -> A edge.
        self.adjacency
            .entry(pool.token_b)
            .or_default()
            .push(PoolEdge {
                pool_address: pool.address,
                token_in: pool.token_b,
                token_out: pool.token_a,
                reserve_in: pool.reserve_b,
                reserve_out: pool.reserve_a,
                fee_bps: pool.fee_bps,
                pool_type: pool.pool_type,
            });
    }

    /// Returns all tokens reachable from the given mint.
    pub fn reachable_tokens(&self, from: &Pubkey) -> HashSet<Pubkey> {
        let mut visited = HashSet::new();
        let mut stack = vec![*from];

        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            if let Some(edges) = self.adjacency.get(&current) {
                for edge in edges {
                    if !visited.contains(&edge.token_out) {
                        stack.push(edge.token_out);
                    }
                }
            }
        }

        visited.remove(from);
        visited
    }

    /// Returns the number of unique tokens.
    pub fn token_count(&self) -> usize {
        self.tokens.len()
    }

    /// Returns the total number of directed edges (each pool contributes 2).
    pub fn edge_count(&self) -> usize {
        self.adjacency.values().map(|v| v.len()).sum()
    }
}

// ---------------------------------------------------------------------------
// Pool registry
// ---------------------------------------------------------------------------

/// Thread-safe registry of known liquidity pools.
pub struct PoolRegistry {
    /// Pools keyed by their on-chain address.
    pools: DashMap<Pubkey, PoolInfo>,
    /// Index: (token_a, token_b) -> list of pool addresses.
    pair_index: DashMap<(Pubkey, Pubkey), Vec<Pubkey>>,
    /// Index: token -> list of pool addresses that include this token.
    token_index: DashMap<Pubkey, Vec<Pubkey>>,
}

impl PoolRegistry {
    pub fn new() -> Self {
        Self {
            pools: DashMap::new(),
            pair_index: DashMap::new(),
            token_index: DashMap::new(),
        }
    }

    /// Register a pool in the registry.
    pub fn register(&self, pool: PoolInfo) {
        let addr = pool.address;
        let pair = pool.token_pair();

        self.pair_index.entry(pair).or_default().push(addr);
        self.token_index
            .entry(pool.token_a)
            .or_default()
            .push(addr);
        self.token_index
            .entry(pool.token_b)
            .or_default()
            .push(addr);

        debug!("Registered pool {} for pair {:?}", addr, pair);
        self.pools.insert(addr, pool);
    }

    /// Retrieve a pool by address.
    pub fn get(&self, address: &Pubkey) -> Option<PoolInfo> {
        self.pools.get(address).map(|r| r.value().clone())
    }

    /// Find all pools for a given token pair (order-independent).
    pub fn pools_for_pair(&self, token_a: &Pubkey, token_b: &Pubkey) -> Vec<PoolInfo> {
        let pair = if *token_a < *token_b {
            (*token_a, *token_b)
        } else {
            (*token_b, *token_a)
        };

        self.pair_index
            .get(&pair)
            .map(|addrs| {
                addrs
                    .iter()
                    .filter_map(|addr| self.pools.get(addr).map(|r| r.value().clone()))
                    .filter(|p| p.is_active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find all pools that include a given token.
    pub fn pools_for_token(&self, token: &Pubkey) -> Vec<PoolInfo> {
        self.token_index
            .get(token)
            .map(|addrs| {
                addrs
                    .iter()
                    .filter_map(|addr| self.pools.get(addr).map(|r| r.value().clone()))
                    .filter(|p| p.is_active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Build a PoolGraph from all registered pools.
    pub fn build_graph(&self) -> PoolGraph {
        let mut graph = PoolGraph::new();
        for entry in self.pools.iter() {
            let pool = entry.value();
            if pool.is_active {
                graph.add_pool(pool);
            }
        }
        info!(
            "Built pool graph: {} tokens, {} edges",
            graph.token_count(),
            graph.edge_count()
        );
        graph
    }

    /// Total number of registered pools.
    pub fn pool_count(&self) -> usize {
        self.pools.len()
    }

    /// Total number of known tokens.
    pub fn token_count(&self) -> usize {
        self.token_index.len()
    }

    /// Update pool reserves (e.g., after fetching new on-chain data).
    pub fn update_reserves(
        &self,
        address: &Pubkey,
        reserve_a: u64,
        reserve_b: u64,
    ) -> Result<()> {
        let mut entry = self
            .pools
            .get_mut(address)
            .ok_or_else(|| anyhow!("Pool {} not found in registry", address))?;
        entry.reserve_a = reserve_a;
        entry.reserve_b = reserve_b;
        Ok(())
    }

    /// Deactivate a pool.
    pub fn deactivate(&self, address: &Pubkey) -> Result<()> {
        let mut entry = self
            .pools
            .get_mut(address)
            .ok_or_else(|| anyhow!("Pool {} not found in registry", address))?;
        entry.is_active = false;
        warn!("Deactivated pool {}", address);
        Ok(())
    }
}

impl Default for PoolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Pool fetcher (simulated on-chain data)
// ---------------------------------------------------------------------------

/// Simulated pool data fetcher.  In production this would decode on-chain
/// account data via `solana_client::rpc_client`.  Here it produces
/// deterministic pools from well-known program addresses.
pub struct PoolFetcher {
    /// Known AMM program IDs to scan.
    pub amm_programs: Vec<Pubkey>,
}

impl PoolFetcher {
    pub fn new() -> Self {
        Self {
            amm_programs: Vec::new(),
        }
    }

    pub fn with_program(mut self, program: Pubkey) -> Self {
        self.amm_programs.push(program);
        self
    }

    /// Simulate fetching pools.  Returns a set of realistic-looking pools.
    pub fn fetch_pools(&self, token_mints: &[Pubkey]) -> Vec<PoolInfo> {
        let mut pools = Vec::new();
        let base_fee = 30u16; // 0.30%

        // Create pools between consecutive token pairs and connect them.
        for window in token_mints.windows(2) {
            let token_a = window[0];
            let token_b = window[1];

            // Derive a deterministic pool address from the two tokens.
            let pool_bytes: Vec<u8> = token_a
                .to_bytes()
                .iter()
                .zip(token_b.to_bytes().iter())
                .map(|(a, b)| a ^ b)
                .collect();
            let mut addr_bytes = [0u8; 32];
            for (i, &b) in pool_bytes.iter().enumerate().take(32) {
                addr_bytes[i] = b;
            }
            let pool_address = Pubkey::new_from_array(addr_bytes);

            // Simulate reserves based on the pool address bytes to get variety.
            let reserve_scale = (addr_bytes[0] as u64 + 1) * 1_000_000;
            let reserve_a = reserve_scale * 1_000;
            let reserve_b = reserve_scale * 800;

            pools.push(PoolInfo::constant_product(
                pool_address,
                token_a,
                token_b,
                reserve_a,
                reserve_b,
                base_fee,
            ));
        }

        // If there are at least 3 tokens, also create a "shortcut" pool from first to last.
        if token_mints.len() >= 3 {
            let first = token_mints[0];
            let last = *token_mints.last().unwrap();

            let mut addr_bytes = [0u8; 32];
            for (i, (&a, &b)) in first
                .to_bytes()
                .iter()
                .zip(last.to_bytes().iter())
                .enumerate()
            {
                addr_bytes[i] = a.wrapping_add(b);
            }
            let pool_address = Pubkey::new_from_array(addr_bytes);

            pools.push(PoolInfo::constant_product(
                pool_address,
                first,
                last,
                500_000_000,
                400_000_000,
                25,
            ));
        }

        info!("Fetched {} simulated pools", pools.len());
        pools
    }
}

impl Default for PoolFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pubkey(seed: u8) -> Pubkey {
        Pubkey::new_from_array([seed; 32])
    }

    #[test]
    fn test_constant_product_output() {
        // 1000 * 2000 = 2_000_000
        // Swap 100 in with 30bps fee.
        // after_fee = 100 * 9970 = 997_000 (scaled by 10000)
        // out = (2000 * 997_000) / (1000 * 10_000 + 997_000)
        //     = 1_994_000_000 / 10_997_000
        //     ~ 181
        let out = calculate_output(1000, 2000, 100, 30);
        assert!(out > 0 && out < 200);
        assert_eq!(out, 181);
    }

    #[test]
    fn test_zero_reserves() {
        assert_eq!(calculate_output(0, 1000, 100, 30), 0);
        assert_eq!(calculate_output(1000, 0, 100, 30), 0);
        assert_eq!(calculate_output(1000, 1000, 0, 30), 0);
    }

    #[test]
    fn test_price_impact_increases_with_size() {
        let impact_small = calculate_price_impact(1_000_000, 1_000_000, 1_000, 30);
        let impact_large = calculate_price_impact(1_000_000, 1_000_000, 100_000, 30);
        assert!(impact_large > impact_small);
    }

    #[test]
    fn test_optimal_split_single_pool() {
        let result = calculate_optimal_split(&[(1_000_000, 1_000_000, 30)], 10_000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], 10_000);
    }

    #[test]
    fn test_optimal_split_equal_pools() {
        let pools = vec![
            (1_000_000u64, 1_000_000u64, 30u16),
            (1_000_000, 1_000_000, 30),
        ];
        let result = calculate_optimal_split(&pools, 10_000);
        assert_eq!(result.len(), 2);
        let total: u64 = result.iter().sum();
        assert_eq!(total, 10_000);
        // For identical pools the split should be roughly equal.
        let diff = (result[0] as i64 - result[1] as i64).unsigned_abs();
        assert!(diff <= 2);
    }

    #[test]
    fn test_pool_graph_construction() {
        let token_a = make_pubkey(1);
        let token_b = make_pubkey(2);
        let token_c = make_pubkey(3);
        let pool_ab = PoolInfo::constant_product(make_pubkey(10), token_a, token_b, 1000, 2000, 30);
        let pool_bc = PoolInfo::constant_product(make_pubkey(11), token_b, token_c, 3000, 4000, 25);

        let mut graph = PoolGraph::new();
        graph.add_pool(&pool_ab);
        graph.add_pool(&pool_bc);

        assert_eq!(graph.token_count(), 3);
        assert_eq!(graph.edge_count(), 4); // 2 per pool (bidirectional)

        let reachable = graph.reachable_tokens(&token_a);
        assert!(reachable.contains(&token_b));
        assert!(reachable.contains(&token_c));
    }

    #[test]
    fn test_pool_registry() {
        let registry = PoolRegistry::new();
        let token_a = make_pubkey(1);
        let token_b = make_pubkey(2);
        let pool = PoolInfo::constant_product(make_pubkey(10), token_a, token_b, 1000, 2000, 30);

        registry.register(pool);

        assert_eq!(registry.pool_count(), 1);
        assert_eq!(registry.pools_for_pair(&token_a, &token_b).len(), 1);
        assert_eq!(registry.pools_for_pair(&token_b, &token_a).len(), 1);
        assert_eq!(registry.pools_for_token(&token_a).len(), 1);
    }

    #[test]
    fn test_input_for_output_round_trip() {
        let res_in = 1_000_000u64;
        let res_out = 1_000_000u64;
        let desired_out = 5_000u64;
        let fee = 30u16;

        let needed_in = calculate_input_for_output(res_in, res_out, desired_out, fee);
        let actual_out = calculate_output(res_in, res_out, needed_in, fee);

        // actual_out should be >= desired_out due to ceiling division.
        assert!(actual_out >= desired_out);
        // But not excessively more.
        assert!(actual_out <= desired_out + 2);
    }

    #[test]
    fn test_clmm_basic_swap() {
        let ranges = vec![TickRange::new(-1000, 1000, 1_000_000_000)];
        let (output, _new_tick) = calculate_clmm_output(&ranges, 0, 10_000, true, 30);
        assert!(output > 0);
    }
}
