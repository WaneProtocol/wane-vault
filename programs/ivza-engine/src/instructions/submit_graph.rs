use anchor_lang::prelude::*;

use crate::errors::EngineError;
use crate::events::GraphSubmitted;
use crate::state::*;

/// Accounts required for the `submit_graph` instruction.
#[derive(Accounts)]
#[instruction(
    graph_id: [u8; 32],
    nodes_data: Vec<u8>,
    edges_data: Vec<u8>,
    node_count: u16,
    edge_count: u16,
    lane_count: u8,
)]
pub struct SubmitGraph<'info> {
    /// The owner / submitter of the graph.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// The engine configuration (must not be paused).
    #[account(
        seeds = [SEED_ENGINE_CONFIG],
        bump = engine_config.bump,
    )]
    pub engine_config: Account<'info, EngineConfig>,

    /// The transaction graph PDA to be created.
    #[account(
        init,
        payer = owner,
        space = TransactionGraph::space(nodes_data.len(), edges_data.len()),
        seeds = [SEED_GRAPH, graph_id.as_ref()],
        bump,
    )]
    pub graph: Account<'info, TransactionGraph>,

    /// System program for account creation.
    pub system_program: Program<'info, System>,
}

/// Handler for the `submit_graph` instruction.
///
/// Validates the graph structure, checks engine constraints, and persists the
/// graph on-chain. The graph starts in `Pending` status.
pub fn handler(
    ctx: Context<SubmitGraph>,
    graph_id: [u8; 32],
    nodes_data: Vec<u8>,
    edges_data: Vec<u8>,
    node_count: u16,
    edge_count: u16,
    lane_count: u8,
) -> Result<()> {
    let config = &ctx.accounts.engine_config;

    // Engine must not be paused.
    require!(!config.paused, EngineError::Paused);

    // Validate node count against engine limits.
    require!(node_count > 0, EngineError::InvalidGraph);
    require!(
        node_count <= config.max_nodes_per_graph,
        EngineError::NodeLimitExceeded
    );

    // Validate edge count against hard limits.
    require!(edge_count <= MAX_EDGES, EngineError::EdgeLimitExceeded);

    // Validate lane count.
    require!(lane_count > 0, EngineError::InvalidLaneCount);
    require!(lane_count <= config.max_lanes, EngineError::InvalidMaxLanes);

    // Validate data sizes.
    require!(
        nodes_data.len() <= MAX_NODES_DATA_LEN,
        EngineError::NodeDataTooLarge
    );
    require!(
        edges_data.len() <= MAX_EDGES_DATA_LEN,
        EngineError::EdgeDataTooLarge
    );

    // Validate edges: check for duplicate edges and invalid node references.
    // Edges are encoded as pairs of u16 (from_node, to_node), 4 bytes per edge.
    if edge_count > 0 {
        let expected_edge_bytes = (edge_count as usize) * 4;
        require!(
            edges_data.len() >= expected_edge_bytes,
            EngineError::InvalidGraph
        );

        validate_edges(&edges_data, node_count, edge_count)?;
    }

    let clock = Clock::get()?;

    // Populate the graph account.
    let graph = &mut ctx.accounts.graph;
    graph.graph_id = graph_id;
    graph.owner = ctx.accounts.owner.key();
    graph.nodes_data = nodes_data;
    graph.edges_data = edges_data;
    graph.node_count = node_count;
    graph.edge_count = edge_count;
    graph.status = GraphStatus::Pending;
    graph.created_at = clock.unix_timestamp;
    graph.lane_count = lane_count;
    graph.lanes_executed = 0;
    graph.lanes_succeeded = 0;
    graph.lanes_failed = 0;
    graph.total_compute_units = 0;
    graph.bump = ctx.bumps.graph;
    graph._reserved = [0u8; 32];

    emit!(GraphSubmitted {
        graph_id,
        owner: graph.owner,
        node_count,
        edge_count,
        lane_count,
        created_at: clock.unix_timestamp,
    });

    msg!(
        "Graph submitted: nodes={}, edges={}, lanes={}",
        node_count,
        edge_count,
        lane_count
    );

    Ok(())
}

/// Validate edge data for structural correctness.
///
/// Each edge is 4 bytes: [from_lo, from_hi, to_lo, to_hi] (little-endian u16 pairs).
/// Checks:
/// 1. All node references are within [0, node_count).
/// 2. No duplicate edges.
/// 3. No self-loops (from == to).
/// 4. The graph is acyclic (topological sort).
fn validate_edges(edges_data: &[u8], node_count: u16, edge_count: u16) -> Result<()> {
    let n = node_count as usize;
    let e = edge_count as usize;

    // Parse all edges.
    let mut edges: Vec<(u16, u16)> = Vec::with_capacity(e);
    for i in 0..e {
        let offset = i * 4;
        let from = u16::from_le_bytes([edges_data[offset], edges_data[offset + 1]]);
        let to = u16::from_le_bytes([edges_data[offset + 2], edges_data[offset + 3]]);

        // Check node references are valid.
        require!(from < node_count, EngineError::InvalidEdgeReference);
        require!(to < node_count, EngineError::InvalidEdgeReference);

        // No self-loops.
        require!(from != to, EngineError::InvalidGraph);

        edges.push((from, to));
    }

    // Check for duplicate edges using a sorted copy.
    {
        let mut sorted_edges = edges.clone();
        sorted_edges.sort_unstable();
        for i in 1..sorted_edges.len() {
            require!(
                sorted_edges[i] != sorted_edges[i - 1],
                EngineError::DuplicateEdge
            );
        }
    }

    // Topological sort to detect cycles (Kahn's algorithm).
    {
        let mut in_degree = vec![0u16; n];
        let mut adj: Vec<Vec<u16>> = vec![Vec::new(); n];

        for &(from, to) in &edges {
            adj[from as usize].push(to);
            in_degree[to as usize] += 1;
        }

        let mut queue: Vec<u16> = Vec::new();
        for i in 0..n {
            if in_degree[i] == 0 {
                queue.push(i as u16);
            }
        }

        let mut visited_count: usize = 0;
        while let Some(node) = queue.pop() {
            visited_count += 1;
            for &neighbor in &adj[node as usize] {
                in_degree[neighbor as usize] -= 1;
                if in_degree[neighbor as usize] == 0 {
                    queue.push(neighbor);
                }
            }
        }

        require!(visited_count == n, EngineError::CyclicGraph);
    }

    Ok(())
}
