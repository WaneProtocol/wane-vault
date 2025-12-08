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