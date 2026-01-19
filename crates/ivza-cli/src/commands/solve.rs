use anyhow::{bail, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use tracing::info;

use crate::config::CliConfig;

/// Arguments for the `solve` subcommand.
#[derive(Args, Debug)]
pub struct SolveArgs {
    /// Path to the intent JSON file. Use "-" for stdin.
    #[arg(short, long)]
    pub input: String,

    /// Maximum number of parallel lanes.
    #[arg(short, long, default_value_t = 4)]
    pub lanes: u8,

    /// Optimization strategy: "balanced" or "critical-path".
    #[arg(short, long, default_value = "balanced")]
    pub strategy: String,

    /// Output the solution as JSON.
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Maximum iterations for the optimizer.
    #[arg(long, default_value_t = 1000)]
    pub max_iterations: u32,
}

/// Intent input format.
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

/// Complete solution output from the solver.
#[derive(Debug, Serialize)]
pub struct SolverSolution {
    pub strategy: String,
    pub iterations_used: u32,
    pub converged: bool,
    pub schedule: Vec<ScheduledNode>,
    pub lane_timelines: Vec<LaneTimeline>,
    pub makespan: u64,
    pub total_cu: u64,
    pub efficiency: f64,
    pub critical_path_cu: u64,
    pub speedup: f64,
}

/// A node placed into the schedule.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduledNode {
    pub node_index: u16,
    pub label: String,
    pub lane: u8,
    pub start_time: u64,
    pub end_time: u64,
    pub estimated_cu: u64,
    pub depth: u16,
}

/// Timeline for a single lane showing ordered execution slots.
#[derive(Debug, Serialize)]
pub struct LaneTimeline {
    pub lane: u8,
    pub slots: Vec<TimeSlot>,
    pub total_cu: u64,
    pub idle_cu: u64,
    pub utilization_pct: f64,
}

/// A time slot within a lane.
#[derive(Debug, Serialize)]
pub struct TimeSlot {
    pub node_index: u16,
    pub label: String,
    pub start: u64,
    pub end: u64,
}

/// Execute the solve command.
pub async fn run(args: SolveArgs, _cfg: &CliConfig) -> Result<()> {
    let raw = read_input(&args.input)?;
    let intent: IntentInput = serde_json::from_str(&raw).context("Failed to parse intent JSON")?;

    if intent.nodes.is_empty() {
        bail!("Intent has no nodes");
    }

    let lane_count = args.lanes.max(1).min(32);

    info!(
        "Solving with strategy='{}', lanes={}, max_iter={}",
        args.strategy, lane_count, args.max_iterations
    );

    let solution = match args.strategy.as_str() {
        "balanced" => solve_balanced(&intent, lane_count, args.max_iterations)?,
        "critical-path" | "critical_path" | "cp" => {
            solve_critical_path(&intent, lane_count, args.max_iterations)?
        }
        other => bail!(
            "Unknown strategy '{}'. Use 'balanced' or 'critical-path'.",
            other
        ),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&solution)?);
    } else {
        print_solution(&solution);
    }

    Ok(())
}

/// Balanced strategy: minimize makespan by distributing load evenly across lanes.
///
/// Uses a list-scheduling heuristic with longest-processing-time-first ordering.
fn solve_balanced(
    intent: &IntentInput,
    lane_count: u8,
    _max_iterations: u32,
) -> Result<SolverSolution> {
    let n = intent.nodes.len();
    let (adj, rev_adj, in_degree) = build_graph(intent)?;
    let topo_order = topological_sort(&adj, &in_degree, n)?;
    let depth = compute_depth(&topo_order, &rev_adj, n);
    let cu_dist = compute_cu_distance(intent, &topo_order, &rev_adj, n);

    // Priority: nodes with higher CU distance (critical path contribution) first.
    let mut priority_order = topo_order.clone();
    priority_order.sort_by(|&a, &b| {
        cu_dist[b as usize]
            .cmp(&cu_dist[a as usize])
            .then_with(|| depth[b as usize].cmp(&depth[a as usize]))
    });

    // Re-sort to respect topological constraints while maintaining priority.
    let schedule = list_schedule(intent, &priority_order, &rev_adj, lane_count, n);

    build_solution(intent, &schedule, &depth, lane_count, "balanced", 1, true)
}

/// Critical-path strategy: prioritize nodes on the critical path for earliest execution.
fn solve_critical_path(
    intent: &IntentInput,
    lane_count: u8,
    max_iterations: u32,
) -> Result<SolverSolution> {
    let n = intent.nodes.len();
    let (adj, rev_adj, in_degree) = build_graph(intent)?;
    let topo_order = topological_sort(&adj, &in_degree, n)?;
    let depth = compute_depth(&topo_order, &rev_adj, n);
    let cu_dist = compute_cu_distance(intent, &topo_order, &rev_adj, n);

    // Find the critical path length.
    let cp_length = cu_dist.iter().copied().max().unwrap_or(0);

    // Mark nodes on the critical path.
    let mut on_critical_path = vec![false; n];
    if cp_length > 0 {
        let end_node = cu_dist
            .iter()
            .enumerate()
            .max_by_key(|(_, &d)| d)
            .map(|(i, _)| i)
            .unwrap();
        mark_critical_path(end_node, &cu_dist, &rev_adj, intent, &mut on_critical_path);
    }

    // Sort: critical-path nodes first, then by CU distance.
    let mut priority_order = topo_order.clone();
    priority_order.sort_by(|&a, &b| {
        let a_cp = on_critical_path[a as usize];
        let b_cp = on_critical_path[b as usize];
        b_cp.cmp(&a_cp)
            .then_with(|| cu_dist[b as usize].cmp(&cu_dist[a as usize]))
    });

    // Iterative refinement: try swapping non-critical nodes between lanes.
    let mut best_schedule = list_schedule(intent, &priority_order, &rev_adj, lane_count, n);
    let mut best_makespan = compute_makespan(&best_schedule);

    let mut iterations = 1u32;
    let mut converged = false;

    for _iter in 0..max_iterations.min(500) {
        iterations += 1;
        let mut candidate = best_schedule.clone();
        let mut improved = false;

        // Try moving non-critical nodes to less loaded lanes.
        for i in 0..candidate.len() {
            if on_critical_path[candidate[i].node_index as usize] {
                continue;
            }

            let current_lane = candidate[i].lane;
            let mut best_lane = current_lane;
            let mut best_end = candidate[i].end_time;

            for l in 0..lane_count {
                if l == current_lane {
                    continue;
                }
                // Compute the earliest start on lane l.
                let lane_ready = candidate
                    .iter()
                    .filter(|s| s.lane == l && s.node_index != candidate[i].node_index)
                    .map(|s| s.end_time)
                    .max()
                    .unwrap_or(0);

                let dep_ready = rev_adj[candidate[i].node_index as usize]
                    .iter()
                    .filter_map(|&pred| {
                        candidate
                            .iter()
                            .find(|s| s.node_index == pred)
                            .map(|s| s.end_time)
                    })
                    .max()
                    .unwrap_or(0);

                let start = lane_ready.max(dep_ready);
                let end = start + candidate[i].estimated_cu;

                if end < best_end {
                    best_end = end;
                    best_lane = l;
                    improved = true;
                }
            }

            if best_lane != current_lane {
                let lane_ready = candidate
                    .iter()
                    .filter(|s| s.lane == best_lane && s.node_index != candidate[i].node_index)
                    .map(|s| s.end_time)
                    .max()
                    .unwrap_or(0);

                let dep_ready = rev_adj[candidate[i].node_index as usize]
                    .iter()
                    .filter_map(|&pred| {
                        candidate
                            .iter()
                            .find(|s| s.node_index == pred)
                            .map(|s| s.end_time)
                    })
                    .max()
                    .unwrap_or(0);

                let start = lane_ready.max(dep_ready);
                candidate[i].lane = best_lane;
                candidate[i].start_time = start;
                candidate[i].end_time = start + candidate[i].estimated_cu;
            }
        }

        let new_makespan = compute_makespan(&candidate);
        if new_makespan < best_makespan {
            best_makespan = new_makespan;
            best_schedule = candidate;
        }

        if !improved {
            converged = true;
            break;
        }
    }

    build_solution(
        intent,
        &best_schedule,
        &depth,
        lane_count,
        "critical-path",
        iterations,
        converged,
    )
}

/// List scheduling: assign ready nodes to the earliest available lane.
fn list_schedule(
    intent: &IntentInput,
    priority_order: &[u16],
    rev_adj: &[Vec<u16>],
    lane_count: u8,
    n: usize,
) -> Vec<ScheduledNode> {
    let mut lane_ready_time = vec![0u64; lane_count as usize];
    let mut node_end_time = vec![0u64; n];
    let mut scheduled: Vec<ScheduledNode> = Vec::with_capacity(n);

    // We need to process in a dependency-respecting order.
    // Use the priority as a tie-breaker within a BFS-like approach.
    let mut priority_rank: HashMap<u16, usize> = HashMap::new();
    for (rank, &node) in priority_order.iter().enumerate() {
        priority_rank.insert(node, rank);
    }

    // Topological BFS with priority.
    let (adj, _, in_degree) = build_graph_raw(intent, n);
    let mut in_deg = in_degree.clone();
    let mut ready: Vec<u16> = Vec::new();
    for i in 0..n {
        if in_deg[i] == 0 {
            ready.push(i as u16);
        }
    }
    // Sort ready by priority (lower rank = higher priority).
    ready.sort_by_key(|&node| priority_rank.get(&node).copied().unwrap_or(usize::MAX));

    while let Some(node) = ready.first().copied() {
        ready.remove(0);

        let ni = node as usize;
        let cu = intent.nodes[ni].estimated_cu;

        // Earliest start: all predecessors must have finished.
        let dep_ready = rev_adj[ni]
            .iter()
            .map(|&pred| node_end_time[pred as usize])
            .max()
            .unwrap_or(0);

        // Find the lane where this node can start earliest.
        let mut best_lane = 0u8;
        let mut best_start = u64::MAX;
        for l in 0..lane_count {
            let start = lane_ready_time[l as usize].max(dep_ready);
            if start < best_start {
                best_start = start;
                best_lane = l;
            }
        }

        let end_time = best_start + cu;
        lane_ready_time[best_lane as usize] = end_time;
        node_end_time[ni] = end_time;

        scheduled.push(ScheduledNode {
            node_index: node,
            label: intent.nodes[ni].label.clone(),
            lane: best_lane,
            start_time: best_start,
            end_time,
            estimated_cu: cu,
            depth: 0, // filled later
        });

        // Release successors.
        for &succ in &adj[ni] {
            in_deg[succ as usize] -= 1;
            if in_deg[succ as usize] == 0 {
                // Insert in sorted order by priority.
                let rank = priority_rank.get(&succ).copied().unwrap_or(usize::MAX);
                let pos = ready
                    .iter()
                    .position(|&r| priority_rank.get(&r).copied().unwrap_or(usize::MAX) > rank)
                    .unwrap_or(ready.len());
                ready.insert(pos, succ);
            }
        }
    }

    scheduled
}

// --- Helper functions ---

fn build_graph(intent: &IntentInput) -> Result<(Vec<Vec<u16>>, Vec<Vec<u16>>, Vec<u16>)> {
    let n = intent.nodes.len();
    let mut adj = vec![Vec::new(); n];
    let mut rev_adj = vec![Vec::new(); n];
    let mut in_degree = vec![0u16; n];

    for edge in &intent.edges {
        let from = edge[0] as usize;
        let to = edge[1] as usize;
        if from >= n || to >= n {
            bail!("Edge references out-of-range node");
        }
        adj[from].push(edge[1]);
        rev_adj[to].push(edge[0]);
        in_degree[to] += 1;
    }

    Ok((adj, rev_adj, in_degree))
}

fn build_graph_raw(intent: &IntentInput, n: usize) -> (Vec<Vec<u16>>, Vec<Vec<u16>>, Vec<u16>) {
    let mut adj = vec![Vec::new(); n];
    let mut rev_adj = vec![Vec::new(); n];
    let mut in_degree = vec![0u16; n];

    for edge in &intent.edges {
        adj[edge[0] as usize].push(edge[1]);
        rev_adj[edge[1] as usize].push(edge[0]);
        in_degree[edge[1] as usize] += 1;
    }

    (adj, rev_adj, in_degree)
}

fn topological_sort(adj: &[Vec<u16>], in_degree: &[u16], n: usize) -> Result<Vec<u16>> {
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

fn compute_depth(topo_order: &[u16], rev_adj: &[Vec<u16>], n: usize) -> Vec<u16> {
    let mut depth = vec![0u16; n];
    for &node in topo_order {
        for &pred in &rev_adj[node as usize] {
            depth[node as usize] = depth[node as usize].max(depth[pred as usize] + 1);
        }
    }
    depth
}

fn compute_cu_distance(
    intent: &IntentInput,
    topo_order: &[u16],
    rev_adj: &[Vec<u16>],
    n: usize,
) -> Vec<u64> {
    let mut cu_dist = vec![0u64; n];
    for &node in topo_order {
        let node_cu = intent.nodes[node as usize].estimated_cu;
        for &pred in &rev_adj[node as usize] {
            cu_dist[node as usize] = cu_dist[node as usize].max(cu_dist[pred as usize]);
        }
        cu_dist[node as usize] += node_cu;
    }
    cu_dist
}

fn mark_critical_path(
    node: usize,
    cu_dist: &[u64],
    rev_adj: &[Vec<u16>],
    intent: &IntentInput,
    on_cp: &mut [bool],
) {
    on_cp[node] = true;
    let node_cu = intent.nodes[node].estimated_cu;
    let target = cu_dist[node] - node_cu;

    if target == 0 && rev_adj[node].is_empty() {
        return;
    }

    for &pred in &rev_adj[node] {
        if cu_dist[pred as usize] == target {
            mark_critical_path(pred as usize, cu_dist, rev_adj, intent, on_cp);
            return; // Only follow one critical path predecessor.
        }
    }
}

fn compute_makespan(schedule: &[ScheduledNode]) -> u64 {
    schedule.iter().map(|s| s.end_time).max().unwrap_or(0)
}

fn build_solution(
    intent: &IntentInput,
    schedule: &[ScheduledNode],
    depth: &[u16],
    lane_count: u8,
    strategy: &str,
    iterations: u32,
    converged: bool,
) -> Result<SolverSolution> {
    let mut final_schedule: Vec<ScheduledNode> = schedule
        .iter()
        .map(|s| {
            let mut s = s.clone();
            s.depth = depth[s.node_index as usize];
            s
        })
        .collect();

    final_schedule.sort_by_key(|s| (s.lane, s.start_time));

    let makespan = compute_makespan(&final_schedule);
    let total_cu: u64 = intent.nodes.iter().map(|n| n.estimated_cu).sum();
    let critical_path_cu = compute_cu_distance(
        intent,
        &topological_sort(
            &build_graph_raw(intent, intent.nodes.len()).0,
            &build_graph_raw(intent, intent.nodes.len()).2,
            intent.nodes.len(),
        )?,
        &build_graph_raw(intent, intent.nodes.len()).1,
        intent.nodes.len(),
    )
    .iter()
    .copied()
    .max()
    .unwrap_or(0);

    let efficiency = if makespan > 0 && lane_count > 0 {
        total_cu as f64 / (makespan as f64 * lane_count as f64)
    } else {
        0.0
    };

    let speedup = if makespan > 0 {
        total_cu as f64 / makespan as f64
    } else {
        1.0
    };

    // Build lane timelines.
    let mut lane_timelines: Vec<LaneTimeline> = (0..lane_count)
        .map(|l| {
            let slots: Vec<TimeSlot> = final_schedule
                .iter()
                .filter(|s| s.lane == l)
                .map(|s| TimeSlot {
                    node_index: s.node_index,
                    label: s.label.clone(),
                    start: s.start_time,
                    end: s.end_time,
                })
                .collect();

            let total_cu_lane: u64 = slots.iter().map(|s| s.end - s.start).sum();
            let idle_cu = makespan.saturating_sub(total_cu_lane);
            let utilization = if makespan > 0 {
                (total_cu_lane as f64 / makespan as f64) * 100.0
            } else {
                0.0
            };

            LaneTimeline {
                lane: l,
                slots,
                total_cu: total_cu_lane,
                idle_cu,
                utilization_pct: utilization,
            }
        })
        .collect();

    Ok(SolverSolution {
        strategy: strategy.to_string(),
        iterations_used: iterations,
        converged,
        schedule: final_schedule,
        lane_timelines,
        makespan,
        total_cu,
        efficiency,
        critical_path_cu,
        speedup,
    })
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read from stdin")?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path).with_context(|| format!("Failed to read file: {}", path))
    }
}

/// Print the solution in human-readable format.
fn print_solution(sol: &SolverSolution) {
    println!("========================================");
    println!("  iVZA Solver Solution");
    println!("========================================");
    println!("Strategy:       {}", sol.strategy);
    println!("Converged:      {}", sol.converged);
    println!("Iterations:     {}", sol.iterations_used);
    println!();
    println!("Performance:");
    println!("  Makespan:     {} CU", sol.makespan);
    println!("  Total CU:     {}", sol.total_cu);
    println!("  Critical Path:{} CU", sol.critical_path_cu);
    println!("  Speedup:      {:.2}x", sol.speedup);
    println!("  Efficiency:   {:.1}%", sol.efficiency * 100.0);
    println!();
    println!("Schedule:");
    for s in &sol.schedule {
        println!(
            "  [{:>3}] {:<20} Lane {} | {:>6}..{:<6} ({} CU, depth {})",
            s.node_index, s.label, s.lane, s.start_time, s.end_time, s.estimated_cu, s.depth,
        );
    }
    println!();
    println!("Lane Timelines:");
    for lt in &sol.lane_timelines {
        println!(
            "  Lane {}: {} slots, {} CU active, {} CU idle ({:.1}% util)",
            lt.lane,
            lt.slots.len(),
            lt.total_cu,
            lt.idle_cu,
            lt.utilization_pct,
        );
        for slot in &lt.slots {
            let width =
                ((slot.end - slot.start) as f64 / sol.makespan.max(1) as f64 * 40.0) as usize;
            let bar: String = "=".repeat(width.max(1));
            println!(
                "    {:>6}..{:<6} [{}] {}",
                slot.start, slot.end, bar, slot.label,
            );
        }
    }
    println!("========================================");
}
