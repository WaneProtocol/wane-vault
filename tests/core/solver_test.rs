//! Comprehensive tests for the ivza-solver crate.
//!
//! Tests cover: PoolRegistry, PoolGraph, RouteEngine, AMM math, GreedySolver,
//! BranchAndBoundSolver, and the SolverEngine.

use ivza_solver::pool::{
    calculate_output, calculate_input_for_output, calculate_price_impact,
    calculate_optimal_split, PoolGraph, PoolInfo, PoolRegistry, PoolType, PoolFetcher,
    TickRange, calculate_clmm_output,
};
use ivza_solver::router::{Route, RouteConfig, RouteEngine};
use ivza_solver::solver::{GreedySolver, BranchAndBoundSolver, Solver, SolverConfig, SwapRequest};
use ivza_solver::{SolverEngine, SolverStrategy};
use solana_sdk::pubkey::Pubkey;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_pubkey(seed: u8) -> Pubkey {
    Pubkey::new_from_array([seed; 32])
}

fn setup_registry() -> PoolRegistry {
    let registry = PoolRegistry::new();
    let sol = make_pubkey(1);
    let usdc = make_pubkey(2);
    let usdt = make_pubkey(3);
    let ray = make_pubkey(4);

    registry.register(PoolInfo::constant_product(
        make_pubkey(10), sol, usdc, 1_000_000_000, 150_000_000_000, 30,
    ));
    registry.register(PoolInfo::constant_product(
        make_pubkey(11), usdc, usdt, 500_000_000_000, 500_000_000_000, 5,
    ));
    registry.register(PoolInfo::constant_product(
        make_pubkey(12), sol, ray, 2_000_000_000, 10_000_000_000, 30,
    ));
    registry.register(PoolInfo::constant_product(
        make_pubkey(13), ray, usdc, 5_000_000_000, 3_750_000_000, 30,
    ));

    registry
}

// ---------------------------------------------------------------------------
// PoolRegistry tests
// ---------------------------------------------------------------------------

#[test]
fn test_registry_register_and_get() {
    let registry = PoolRegistry::new();
    let pool_addr = make_pubkey(10);
    let pool = PoolInfo::constant_product(
        pool_addr, make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    );
    registry.register(pool);

    assert_eq!(registry.pool_count(), 1);
    let retrieved = registry.get(&pool_addr).unwrap();
    assert_eq!(retrieved.reserve_a, 1000);
    assert_eq!(retrieved.reserve_b, 2000);
}

#[test]
fn test_registry_lookup_by_pair() {
    let registry = PoolRegistry::new();
    let token_a = make_pubkey(1);
    let token_b = make_pubkey(2);

    registry.register(PoolInfo::constant_product(
        make_pubkey(10), token_a, token_b, 1000, 2000, 30,
    ));

    // Both orderings should work
    assert_eq!(registry.pools_for_pair(&token_a, &token_b).len(), 1);
    assert_eq!(registry.pools_for_pair(&token_b, &token_a).len(), 1);
}

#[test]
fn test_registry_lookup_by_token() {
    let registry = PoolRegistry::new();
    let sol = make_pubkey(1);
    let usdc = make_pubkey(2);
    let usdt = make_pubkey(3);

    registry.register(PoolInfo::constant_product(
        make_pubkey(10), sol, usdc, 1000, 2000, 30,
    ));
    registry.register(PoolInfo::constant_product(
        make_pubkey(11), sol, usdt, 3000, 4000, 25,
    ));

    let sol_pools = registry.pools_for_token(&sol);
    assert_eq!(sol_pools.len(), 2);

    let usdc_pools = registry.pools_for_token(&usdc);
    assert_eq!(usdc_pools.len(), 1);
}

#[test]
fn test_registry_update_reserves() {
    let registry = PoolRegistry::new();
    let pool_addr = make_pubkey(10);
    registry.register(PoolInfo::constant_product(
        pool_addr, make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    ));

    registry.update_reserves(&pool_addr, 5000, 10000).unwrap();
    let pool = registry.get(&pool_addr).unwrap();
    assert_eq!(pool.reserve_a, 5000);
    assert_eq!(pool.reserve_b, 10000);
}

#[test]
fn test_registry_deactivate_pool() {
    let registry = PoolRegistry::new();
    let pool_addr = make_pubkey(10);
    let token_a = make_pubkey(1);
    let token_b = make_pubkey(2);

    registry.register(PoolInfo::constant_product(
        pool_addr, token_a, token_b, 1000, 2000, 30,
    ));
    registry.deactivate(&pool_addr).unwrap();

    // Deactivated pools should not appear in pair lookups
    let pools = registry.pools_for_pair(&token_a, &token_b);
    assert!(pools.is_empty());
}

#[test]
fn test_registry_token_count() {
    let registry = PoolRegistry::new();
    registry.register(PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    ));
    assert_eq!(registry.token_count(), 2);
}

// ---------------------------------------------------------------------------
// PoolGraph tests
// ---------------------------------------------------------------------------

#[test]
fn test_pool_graph_construction() {
    let mut graph = PoolGraph::new();
    let pool = PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    );
    graph.add_pool(&pool);

    assert_eq!(graph.token_count(), 2);
    assert_eq!(graph.edge_count(), 2); // bidirectional
}

#[test]
fn test_pool_graph_reachable_tokens() {
    let mut graph = PoolGraph::new();
    let sol = make_pubkey(1);
    let usdc = make_pubkey(2);
    let usdt = make_pubkey(3);

    graph.add_pool(&PoolInfo::constant_product(
        make_pubkey(10), sol, usdc, 1000, 2000, 30,
    ));
    graph.add_pool(&PoolInfo::constant_product(
        make_pubkey(11), usdc, usdt, 3000, 4000, 25,
    ));

    let reachable = graph.reachable_tokens(&sol);
    assert!(reachable.contains(&usdc));
    assert!(reachable.contains(&usdt));
}

#[test]
fn test_pool_graph_unreachable_token() {
    let mut graph = PoolGraph::new();
    graph.add_pool(&PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    ));

    let reachable = graph.reachable_tokens(&make_pubkey(1));
    assert!(!reachable.contains(&make_pubkey(99)));
}

// ---------------------------------------------------------------------------
// AMM math tests
// ---------------------------------------------------------------------------

#[test]
fn test_calculate_output_basic() {
    let out = calculate_output(1000, 2000, 100, 30);
    assert!(out > 0);
    assert!(out < 200); // Can't get more than the simple ratio
    assert_eq!(out, 181);
}

#[test]
fn test_calculate_output_zero_reserves() {
    assert_eq!(calculate_output(0, 1000, 100, 30), 0);
    assert_eq!(calculate_output(1000, 0, 100, 30), 0);
    assert_eq!(calculate_output(1000, 1000, 0, 30), 0);
}

#[test]
fn test_calculate_output_no_fee() {
    let out = calculate_output(1_000_000, 1_000_000, 100, 0);
    // Without fee: out = 1M * 100 / (1M + 100) ~ 99
    assert!(out > 0);
    assert!(out <= 100);
}

#[test]
fn test_calculate_output_large_amount() {
    // Swapping half the reserve
    let out = calculate_output(1_000_000, 1_000_000, 500_000, 30);
    // Should be significantly less than 500_000 due to price impact
    assert!(out < 500_000);
    assert!(out > 0);
}

#[test]
fn test_calculate_input_for_output_round_trip() {
    let res_in = 1_000_000u64;
    let res_out = 1_000_000u64;
    let desired = 5_000u64;
    let fee = 30u16;

    let needed = calculate_input_for_output(res_in, res_out, desired, fee);
    let actual = calculate_output(res_in, res_out, needed, fee);

    assert!(actual >= desired);
    assert!(actual <= desired + 2); // Ceiling division tolerance
}

#[test]
fn test_calculate_input_for_output_impossible() {
    // Requesting more than the entire reserve
    let needed = calculate_input_for_output(1000, 1000, 1001, 30);
    assert_eq!(needed, u64::MAX);
}