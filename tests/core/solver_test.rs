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

#[test]
fn test_price_impact_increases_with_size() {
    let small = calculate_price_impact(1_000_000, 1_000_000, 1_000, 30);
    let large = calculate_price_impact(1_000_000, 1_000_000, 100_000, 30);
    assert!(large > small);
}

#[test]
fn test_price_impact_zero_input() {
    let impact = calculate_price_impact(1_000_000, 1_000_000, 0, 30);
    assert_eq!(impact, 0.0);
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
    let diff = (result[0] as i64 - result[1] as i64).unsigned_abs();
    assert!(diff <= 2);
}

#[test]
fn test_optimal_split_empty() {
    let result = calculate_optimal_split(&[], 10_000);
    assert!(result.is_empty());
}

// ---------------------------------------------------------------------------
// CLMM math tests
// ---------------------------------------------------------------------------

#[test]
fn test_clmm_basic_swap_a_to_b() {
    let ranges = vec![TickRange::new(-1000, 1000, 1_000_000_000)];
    let (output, new_tick) = calculate_clmm_output(&ranges, 0, 10_000, true, 30);
    assert!(output > 0);
}

#[test]
fn test_clmm_basic_swap_b_to_a() {
    let ranges = vec![TickRange::new(-1000, 1000, 1_000_000_000)];
    let (output, _new_tick) = calculate_clmm_output(&ranges, 0, 10_000, false, 30);
    assert!(output > 0);
}

#[test]
fn test_clmm_zero_amount() {
    let ranges = vec![TickRange::new(-1000, 1000, 1_000_000_000)];
    let (output, _) = calculate_clmm_output(&ranges, 0, 0, true, 30);
    assert_eq!(output, 0);
}

#[test]
fn test_tick_range_price() {
    let price_at_0 = TickRange::price_at_tick(0);
    assert!((price_at_0 - 1.0).abs() < 0.001);

    let price_at_100 = TickRange::price_at_tick(100);
    assert!(price_at_100 > 1.0);
}

// ---------------------------------------------------------------------------
// PoolInfo tests
// ---------------------------------------------------------------------------

#[test]
fn test_pool_info_token_pair_canonical() {
    let a = make_pubkey(1);
    let b = make_pubkey(2);
    let pool = PoolInfo::constant_product(make_pubkey(10), a, b, 1000, 2000, 30);

    let (lo, hi) = pool.token_pair();
    assert!(lo < hi);
}

#[test]
fn test_pool_info_other_token() {
    let a = make_pubkey(1);
    let b = make_pubkey(2);
    let pool = PoolInfo::constant_product(make_pubkey(10), a, b, 1000, 2000, 30);

    assert_eq!(pool.other_token(&a), Some(b));
    assert_eq!(pool.other_token(&b), Some(a));
    assert_eq!(pool.other_token(&make_pubkey(99)), None);
}

#[test]
fn test_pool_info_spot_price() {
    let pool = PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    );
    assert_eq!(pool.spot_price_a_to_b(), 2.0);
    assert_eq!(pool.spot_price_b_to_a(), 0.5);
}

#[test]
fn test_pool_info_invariant_k() {
    let pool = PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    );
    assert_eq!(pool.invariant_k(), 2_000_000);
}

// ---------------------------------------------------------------------------
// RouteEngine tests
// ---------------------------------------------------------------------------

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
}

#[test]
fn test_find_multi_hop_route() {
    let registry = setup_registry();
    let engine = RouteEngine::new(registry);

    let sol = make_pubkey(1);
    let usdt = make_pubkey(3);

    let routes = engine.find_routes(&sol, &usdt, 1_000_000).unwrap();
    let has_multi = routes.iter().any(|r| r.hop_count() >= 2);
    assert!(has_multi);
}

#[test]
fn test_no_route_exists() {
    let registry = PoolRegistry::new();
    registry.register(PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2), 1000, 2000, 30,
    ));
    let engine = RouteEngine::new(registry);

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
fn test_best_route() {
    let registry = setup_registry();
    let engine = RouteEngine::new(registry);

    let best = engine.find_best_route(&make_pubkey(1), &make_pubkey(2), 1_000_000).unwrap();
    assert!(best.output_amount > 0);
    assert!(best.score > 0.0);
}

#[test]
fn test_route_token_path() {
    let registry = setup_registry();
    let engine = RouteEngine::new(registry);

    let sol = make_pubkey(1);
    let usdc = make_pubkey(2);

    let best = engine.find_best_route(&sol, &usdc, 1_000_000).unwrap();
    let path = best.token_path();
    assert_eq!(path.first(), Some(&sol));
    assert_eq!(path.last(), Some(&usdc));
}

// ---------------------------------------------------------------------------
// GreedySolver tests
// ---------------------------------------------------------------------------

#[test]
fn test_greedy_solver_single_swap() {
    let registry = setup_registry();
    let engine = RouteEngine::new(registry);
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
fn test_greedy_solver_multiple_swaps() {
    let registry = setup_registry();
    let engine = RouteEngine::new(registry);
    let config = SolverConfig::default();
    let solver = GreedySolver::new();

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
fn test_greedy_solver_no_routes() {
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
    assert!(!result.all_solved());
    assert_eq!(result.failed_count, 1);
}

// ---------------------------------------------------------------------------
// BranchAndBoundSolver tests
// ---------------------------------------------------------------------------

#[test]
fn test_branch_and_bound_solver() {
    let registry = setup_registry();
    let engine = RouteEngine::new(registry);
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

// ---------------------------------------------------------------------------
// SolverEngine integration tests
// ---------------------------------------------------------------------------

#[test]
fn test_solver_engine_register_and_solve() {
    let mut engine = SolverEngine::new();
    engine.register_pool(PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2),
        1_000_000_000, 150_000_000_000, 30,
    ));

    let requests = vec![SwapRequest {
        node_id: 0,
        input_mint: make_pubkey(1),
        output_mint: make_pubkey(2),
        amount: 1_000_000,
        label: None,
    }];

    let result = engine.solve(&requests, SolverStrategy::Greedy).unwrap();
    assert!(result.all_solved());
}

#[test]
fn test_solver_engine_find_best_route() {
    let mut engine = SolverEngine::new();
    engine.register_pool(PoolInfo::constant_product(
        make_pubkey(10), make_pubkey(1), make_pubkey(2),
        1_000_000_000, 150_000_000_000, 30,
    ));

    let route = engine.find_best_route(&make_pubkey(1), &make_pubkey(2), 1_000_000).unwrap();
    assert!(route.output_amount > 0);
}

#[test]
fn test_solver_engine_fetch_and_register() {
    let mut engine = SolverEngine::new();
    let tokens = vec![make_pubkey(1), make_pubkey(2), make_pubkey(3)];
    engine.fetch_and_register(&tokens);
    assert!(engine.pool_count() >= 2);
}

#[test]
fn test_solver_strategy_default() {
    assert_eq!(SolverStrategy::default(), SolverStrategy::Greedy);
}

#[test]
fn test_pool_fetcher() {
    let fetcher = PoolFetcher::new();
    let tokens = vec![make_pubkey(1), make_pubkey(2), make_pubkey(3)];
    let pools = fetcher.fetch_pools(&tokens);
    assert!(pools.len() >= 2);
}
