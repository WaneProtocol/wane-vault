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