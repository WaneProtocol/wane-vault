use anyhow::{bail, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Read;
use tracing::info;

use crate::config::CliConfig;

/// Arguments for the `analyze` subcommand.
#[derive(Args, Debug)]
pub struct AnalyzeArgs {
    /// Path to the intent JSON file. Use "-" for stdin.
    #[arg(short, long)]
    pub input: String,

    /// Output the execution plan as JSON.
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Target number of lanes for scheduling.
    #[arg(short, long, default_value_t = 4)]
    pub lanes: u8,
}

/// Input format matching the submit command's IntentInput.
#[derive(Debug, Deserialize)]
struct IntentInput {
    nodes: Vec<NodeInput>,
    edges: Vec<[u16; 2]>,
    #[serde(default = "default_lanes")]
    lanes: u8,
}

fn default_lanes() -> u8 {
    4
}

#[derive(Debug, Deserialize)]
struct NodeInput {
    label: String,
    #[serde(default)]
    estimated_cu: u64,
    #[serde(default)]
    data: String,
}

/// Full analysis result.
#[derive(Debug, Serialize)]
pub struct AnalysisResult {
    /// Summary statistics.
    pub summary: AnalysisSummary,
    /// Critical path through the graph.
    pub critical_path: Vec<CriticalPathNode>,
    /// Lane assignments for each node.
    pub lane_assignments: Vec<LaneAssignment>,
    /// Per-lane statistics.
    pub lane_stats: Vec<LaneStat>,
    /// Topological ordering of nodes.
    pub topological_order: Vec<u16>,
}

#[derive(Debug, Serialize)]
pub struct AnalysisSummary {
    pub node_count: usize,
    pub edge_count: usize,
    pub lane_count: u8,
    pub critical_path_length: u64,
    pub total_cu: u64,
    pub parallelism_degree: f64,
    pub estimated_sequential_cu: u64,
    pub estimated_parallel_cu: u64,
    pub estimated_cu_savings_pct: f64,
    pub graph_density: f64,
    pub max_in_degree: usize,
    pub max_out_degree: usize,
}

#[derive(Debug, Serialize)]
pub struct CriticalPathNode {
    pub index: u16,
    pub label: String,
    pub cumulative_cu: u64,
}

#[derive(Debug, Serialize)]
pub struct LaneAssignment {
    pub node_index: u16,
    pub label: String,
    pub lane: u8,
    pub depth: u16,
    pub estimated_cu: u64,
}

#[derive(Debug, Serialize)]
pub struct LaneStat {
    pub lane: u8,
    pub node_count: usize,
    pub total_cu: u64,
    pub max_depth: u16,
}

/// Execute the analyze command.
pub async fn run(args: AnalyzeArgs, _cfg: &CliConfig) -> Result<()> {
    let raw = read_input(&args.input)?;
    let intent: IntentInput =
        serde_json::from_str(&raw).context("Failed to parse intent JSON")?;

    if intent.nodes.is_empty() {
        bail!("Intent has no nodes");
    }

    let lane_count = args.lanes.max(1).min(32);
    let result = analyze_graph(&intent, lane_count)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_analysis(&result);
    }

    Ok(())
}

/// Run the full offline analysis pipeline.
fn analyze_graph(intent: &IntentInput, lane_count: u8) -> Result<AnalysisResult> {
    let n = intent.nodes.len();

    // Build adjacency lists and compute in/out degrees.
    let mut adj: Vec<Vec<u16>> = vec![Vec::new(); n];
    let mut rev_adj: Vec<Vec<u16>> = vec![Vec::new(); n];
    let mut in_degree = vec![0u16; n];
    let mut out_degree = vec![0u16; n];

    for edge in &intent.edges {
        let from = edge[0] as usize;
        let to = edge[1] as usize;
        if from >= n || to >= n {
            bail!("Edge references out-of-range node");
        }
        adj[from].push(edge[1]);
        rev_adj[to].push(edge[0]);
        in_degree[to] += 1;
        out_degree[from] += 1;
    }

    // Topological sort (Kahn's).
    let topo_order = topological_sort(&adj, &in_degree, n)?;

    // Compute depth (longest path from any source) for each node.
    let mut depth = vec![0u16; n];
    for &node in &topo_order {
        for &pred in &rev_adj[node as usize] {
            depth[node as usize] = depth[node as usize].max(depth[pred as usize] + 1);
        }
    }

    // Compute critical path (longest CU path).
    let mut cu_dist = vec![0u64; n];
    for &node in &topo_order {
        let node_cu = intent.nodes[node as usize].estimated_cu;
        for &pred in &rev_adj[node as usize] {
            cu_dist[node as usize] = cu_dist[node as usize].max(cu_dist[pred as usize]);
        }
        cu_dist[node as usize] += node_cu;
    }

    // Trace back the critical path from the node with maximum CU distance.
    let critical_path = trace_critical_path(intent, &cu_dist, &rev_adj);

    // Assign nodes to lanes using a greedy load-balancing heuristic.
    // Process nodes in topological order; assign each to the lane with the least load
    // among lanes that satisfy dependency constraints.
    let lane_assignments = assign_lanes(intent, &topo_order, &depth, &adj, &rev_adj, lane_count);

    // Compute per-lane statistics.
    let mut lane_stats: Vec<LaneStat> = (0..lane_count)
        .map(|i| LaneStat {
            lane: i,
            node_count: 0,
            total_cu: 0,
            max_depth: 0,
        })
        .collect();

    for la in &lane_assignments {
        let ls = &mut lane_stats[la.lane as usize];
        ls.node_count += 1;
        ls.total_cu += la.estimated_cu;
        ls.max_depth = ls.max_depth.max(la.depth);
    }

    // Compute summary statistics.
    let total_cu: u64 = intent.nodes.iter().map(|n| n.estimated_cu).sum();
    let critical_path_length = cu_dist.iter().copied().max().unwrap_or(0);
    let max_lane_cu = lane_stats.iter().map(|l| l.total_cu).max().unwrap_or(0);

    let parallelism_degree = if critical_path_length > 0 {
        total_cu as f64 / critical_path_length as f64
    } else {
        1.0
    };

    let savings_pct = if total_cu > 0 {
        ((total_cu as f64 - max_lane_cu as f64) / total_cu as f64) * 100.0
    } else {
        0.0
    };

    let max_possible_edges = if n > 1 { n * (n - 1) / 2 } else { 1 };
    let graph_density = intent.edges.len() as f64 / max_possible_edges as f64;

    let summary = AnalysisSummary {
        node_count: n,
        edge_count: intent.edges.len(),
        lane_count,
        critical_path_length,
        total_cu,
        parallelism_degree,
        estimated_sequential_cu: total_cu,
        estimated_parallel_cu: max_lane_cu,
        estimated_cu_savings_pct: savings_pct,
        graph_density,
        max_in_degree: in_degree.iter().copied().max().unwrap_or(0) as usize,
        max_out_degree: out_degree.iter().copied().max().unwrap_or(0) as usize,
    };

    Ok(AnalysisResult {
        summary,
        critical_path,
        lane_assignments,
        lane_stats,
        topological_order: topo_order,
    })
}

/// Kahn's algorithm for topological sorting.
fn topological_sort(
    adj: &[Vec<u16>],
    in_degree: &[u16],
    n: usize,
) -> Result<Vec<u16>> {
    let mut in_deg = in_degree.to_vec();
    let mut queue: VecDeque<u16> = VecDeque::new();

    for i in 0..n {
        if in_deg[i] == 0 {
            queue.push_back(i as u16);
        }
    }

    let mut order = Vec::with_capacity(n);
    while let Some(node) = queue.pop_front() {
        order.push(node);
        for &neighbor in &adj[node as usize] {
            in_deg[neighbor as usize] -= 1;
            if in_deg[neighbor as usize] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    if order.len() != n {
        bail!("Graph contains a cycle");
    }
    Ok(order)
}

/// Trace the critical path by walking backwards from the maximum CU node.
fn trace_critical_path(
    intent: &IntentInput,
    cu_dist: &[u64],
    rev_adj: &[Vec<u16>],
) -> Vec<CriticalPathNode> {
    if cu_dist.is_empty() {
        return Vec::new();
    }

    let mut path = Vec::new();
    let mut current = cu_dist
        .iter()
        .enumerate()
        .max_by_key(|(_, &d)| d)
        .map(|(i, _)| i)
        .unwrap();

    loop {
        path.push(CriticalPathNode {
            index: current as u16,
            label: intent.nodes[current].label.clone(),
            cumulative_cu: cu_dist[current],
        });

        // Walk to the predecessor with the highest CU distance.
        let best_pred = rev_adj[current]
            .iter()
            .max_by_key(|&&p| cu_dist[p as usize]);

        match best_pred {
            Some(&pred) => current = pred as usize,
            None => break,
        }
    }

    path.reverse();
    path
}

/// Assign nodes to lanes using a greedy load-balancing approach.
///
/// Nodes are processed in topological order. Each node is assigned to the lane
/// with the smallest accumulated CU load, ensuring that all predecessors'
/// lane constraints are satisfiable.
fn assign_lanes(
    intent: &IntentInput,
    topo_order: &[u16],
    depth: &[u16],
    _adj: &[Vec<u16>],
    _rev_adj: &[Vec<u16>],
    lane_count: u8,
) -> Vec<LaneAssignment> {
    let n = intent.nodes.len();
    let mut node_lane = vec![0u8; n];
    let mut lane_load = vec![0u64; lane_count as usize];

    for &node in topo_order {
        let ni = node as usize;
        // Find the lane with the least load.
        let mut best_lane = 0u8;
        let mut best_load = u64::MAX;
        for l in 0..lane_count {
            if lane_load[l as usize] < best_load {
                best_load = lane_load[l as usize];
                best_lane = l;
            }
        }
        node_lane[ni] = best_lane;
        lane_load[best_lane as usize] += intent.nodes[ni].estimated_cu;
    }

    let mut assignments = Vec::with_capacity(n);
    for i in 0..n {
        assignments.push(LaneAssignment {
            node_index: i as u16,
            label: intent.nodes[i].label.clone(),
            lane: node_lane[i],
            depth: depth[i],
            estimated_cu: intent.nodes[i].estimated_cu,
        });
    }
    assignments
}

/// Read input from file or stdin.
fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read from stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path))
    }
}

/// Print the analysis in human-readable format.
fn print_analysis(result: &AnalysisResult) {
    let s = &result.summary;
    println!("========================================");
    println!("  iVZA Graph Analysis");
    println!("========================================");
    println!();
    println!("Graph Topology:");
    println!("  Nodes:           {}", s.node_count);
    println!("  Edges:           {}", s.edge_count);
    println!("  Density:         {:.4}", s.graph_density);
    println!("  Max in-degree:   {}", s.max_in_degree);
    println!("  Max out-degree:  {}", s.max_out_degree);
    println!();
    println!("Execution Plan:");
    println!("  Lanes:           {}", s.lane_count);
    println!("  Parallelism:     {:.2}x", s.parallelism_degree);
    println!("  Sequential CU:   {}", s.estimated_sequential_cu);
    println!("  Parallel CU:     {}", s.estimated_parallel_cu);
    println!("  CU Savings:      {:.1}%", s.estimated_cu_savings_pct);
    println!();
    println!("Critical Path ({} CU):", s.critical_path_length);
    for cp in &result.critical_path {
        println!(
            "  [{:>3}] {:<20} (cumulative: {} CU)",
            cp.index, cp.label, cp.cumulative_cu
        );
    }
    println!();
    println!("Lane Statistics:");
    for ls in &result.lane_stats {
        let bar_len = if s.estimated_parallel_cu > 0 {
            ((ls.total_cu as f64 / s.estimated_parallel_cu as f64) * 20.0) as usize
        } else {
            0
        };
        let bar: String = "#".repeat(bar_len.min(20));
        println!(
            "  Lane {}: {:>3} nodes, {:>8} CU, depth {:>3}  [{}]",
            ls.lane, ls.node_count, ls.total_cu, ls.max_depth, bar
        );
    }
    println!();
    println!("Node Assignments:");
    for la in &result.lane_assignments {
        println!(
            "  [{:>3}] {:<20} -> Lane {} (depth {}, {} CU)",
            la.node_index, la.label, la.lane, la.depth, la.estimated_cu
        );
    }
    println!();
    println!("Topological Order:");
    print!("  ");
    for (i, &node) in result.topological_order.iter().enumerate() {
        if i > 0 {
            print!(" -> ");
        }
        print!("{}", node);
    }
    println!();
    println!("========================================");
}
