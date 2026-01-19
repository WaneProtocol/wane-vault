use anyhow::{bail, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{read_keypair_file, Signer},
    system_program,
    transaction::Transaction,
};
use std::io::Read;
use std::str::FromStr;
use tracing::{error, info};

use crate::config::CliConfig;

/// Arguments for the `submit` subcommand.
#[derive(Args, Debug)]
pub struct SubmitArgs {
    /// Path to the intent JSON file. Use "-" for stdin.
    #[arg(short, long)]
    pub input: String,

    /// Override the number of parallel lanes.
    #[arg(short, long)]
    pub lanes: Option<u8>,

    /// Dry-run mode: validate and display but do not submit.
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
}

/// Serialized intent format read from file or stdin.
#[derive(Debug, Serialize, Deserialize)]
pub struct IntentInput {
    /// List of transaction nodes with their metadata.
    pub nodes: Vec<NodeInput>,
    /// List of directed edges (from, to) by node index.
    pub edges: Vec<[u16; 2]>,
    /// Requested number of parallel lanes.
    #[serde(default = "default_lanes")]
    pub lanes: u8,
}

fn default_lanes() -> u8 {
    4
}

/// A single transaction node in the intent.
#[derive(Debug, Serialize, Deserialize)]
pub struct NodeInput {
    /// Human-readable label for the node.
    pub label: String,
    /// Estimated compute units for this node.
    #[serde(default)]
    pub estimated_cu: u64,
    /// Serialized instruction data (base58 encoded).
    #[serde(default)]
    pub data: String,
}

/// Seed constants matching the on-chain program.
const SEED_ENGINE_CONFIG: &[u8] = b"engine_config";
const SEED_GRAPH: &[u8] = b"graph";

/// Anchor instruction discriminator for `submit_graph`.
/// sha256("global:submit_graph")[..8]
const SUBMIT_GRAPH_DISCRIMINATOR: [u8; 8] = [0xd0, 0xb8, 0xa5, 0x3e, 0x6c, 0x4f, 0x2a, 0x17];

/// Execute the submit command.
pub async fn run(args: SubmitArgs, cfg: &CliConfig) -> Result<()> {
    // Read the intent from file or stdin.
    let raw = read_input(&args.input)?;
    let intent: IntentInput = serde_json::from_str(&raw).context("Failed to parse intent JSON")?;

    // Basic validation.
    if intent.nodes.is_empty() {
        bail!("Intent has no nodes");
    }
    if intent.nodes.len() > 256 {
        bail!("Intent exceeds maximum of 256 nodes");
    }
    if intent.edges.len() > 1024 {
        bail!("Intent exceeds maximum of 1024 edges");
    }

    let lane_count = args.lanes.unwrap_or(intent.lanes);
    if lane_count == 0 || lane_count > 32 {
        bail!("Lane count must be between 1 and 32");
    }

    // Validate edge references.
    let node_count = intent.nodes.len() as u16;
    for (i, edge) in intent.edges.iter().enumerate() {
        if edge[0] >= node_count || edge[1] >= node_count {
            bail!("Edge {} references out-of-range node index", i);
        }
        if edge[0] == edge[1] {
            bail!("Edge {} is a self-loop", i);
        }
    }

    // Serialize nodes: each node is label_len(2) + label + estimated_cu(8).
    let nodes_data = serialize_nodes(&intent.nodes);
    let edges_data = serialize_edges(&intent.edges);

    // Generate a graph ID from the content hash.
    let graph_id = compute_graph_id(&nodes_data, &edges_data);

    info!(
        "Graph: {} nodes, {} edges, {} lanes, id={}",
        node_count,
        intent.edges.len(),
        lane_count,
        bs58::encode(&graph_id).into_string(),
    );

    if args.dry_run {
        info!("Dry-run mode: skipping on-chain submission");
        print_graph_summary(&intent, &graph_id, lane_count);
        return Ok(());
    }

    // Build and send the transaction.
    let program_id =
        Pubkey::from_str(&cfg.program_id).context("Invalid program ID in configuration")?;

    let keypair = read_keypair_file(&cfg.keypair_path)
        .map_err(|e| anyhow::anyhow!("Failed to read keypair from {}: {}", cfg.keypair_path, e))?;

    let client = RpcClient::new_with_commitment(
        cfg.rpc_url.clone(),
        CommitmentConfig {
            commitment: cfg.commitment_level(),
        },
    );

    // Derive PDAs.
    let (config_pda, _) = Pubkey::find_program_address(&[SEED_ENGINE_CONFIG], &program_id);
    let (graph_pda, _) =
        Pubkey::find_program_address(&[SEED_GRAPH, graph_id.as_ref()], &program_id);

    // Build instruction data.
    let mut ix_data = Vec::new();
    ix_data.extend_from_slice(&SUBMIT_GRAPH_DISCRIMINATOR);
    ix_data.extend_from_slice(&graph_id);
    // nodes_data as borsh Vec<u8>: len(u32) + data
    ix_data.extend_from_slice(&(nodes_data.len() as u32).to_le_bytes());
    ix_data.extend_from_slice(&nodes_data);
    // edges_data as borsh Vec<u8>: len(u32) + data
    ix_data.extend_from_slice(&(edges_data.len() as u32).to_le_bytes());
    ix_data.extend_from_slice(&edges_data);
    ix_data.extend_from_slice(&node_count.to_le_bytes());
    ix_data.extend_from_slice(&(intent.edges.len() as u16).to_le_bytes());
    ix_data.push(lane_count);

    let accounts = vec![
        AccountMeta::new(keypair.pubkey(), true), // owner (signer, mutable)
        AccountMeta::new_readonly(config_pda, false), // engine_config
        AccountMeta::new(graph_pda, false),       // graph
        AccountMeta::new_readonly(system_program::id(), false), // system_program
    ];

    let ix = Instruction {
        program_id,
        accounts,
        data: ix_data,
    };

    let recent_blockhash = client
        .get_latest_blockhash()
        .context("Failed to fetch recent blockhash")?;

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&keypair.pubkey()),
        &[&keypair],
        recent_blockhash,
    );

    info!("Submitting transaction...");
    match client.send_and_confirm_transaction(&tx) {
        Ok(sig) => {
            info!("Graph submitted successfully!");
            println!("Signature: {}", sig);
            println!("Graph ID:  {}", bs58::encode(&graph_id).into_string());
            println!("Graph PDA: {}", graph_pda);
        }
        Err(e) => {
            error!("Transaction failed: {}", e);
            bail!("Failed to submit graph: {}", e);
        }
    }

    Ok(())
}

/// Read input from a file path or stdin (if path is "-").
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

/// Serialize nodes into a compact binary format.
/// Format per node: label_len(u16 LE) + label_bytes + estimated_cu(u64 LE).
fn serialize_nodes(nodes: &[NodeInput]) -> Vec<u8> {
    let mut buf = Vec::new();
    for node in nodes {
        let label_bytes = node.label.as_bytes();
        let label_len = label_bytes.len().min(u16::MAX as usize) as u16;
        buf.extend_from_slice(&label_len.to_le_bytes());
        buf.extend_from_slice(&label_bytes[..label_len as usize]);
        buf.extend_from_slice(&node.estimated_cu.to_le_bytes());
    }
    buf
}

/// Serialize edges into a compact binary format.
/// Format per edge: from(u16 LE) + to(u16 LE).
fn serialize_edges(edges: &[[u16; 2]]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(edges.len() * 4);
    for edge in edges {
        buf.extend_from_slice(&edge[0].to_le_bytes());
        buf.extend_from_slice(&edge[1].to_le_bytes());
    }
    buf
}

/// Compute a deterministic 32-byte graph ID from the graph content.
fn compute_graph_id(nodes_data: &[u8], edges_data: &[u8]) -> [u8; 32] {
    use solana_sdk::hash::hashv;
    let hash = hashv(&[b"ivza_graph", nodes_data, edges_data]);
    hash.to_bytes()
}

/// Print a human-readable summary of the graph (dry-run mode).
fn print_graph_summary(intent: &IntentInput, graph_id: &[u8; 32], lane_count: u8) {
    println!("========================================");
    println!("  iVZA Graph Summary (dry-run)");
    println!("========================================");
    println!("Graph ID:   {}", bs58::encode(graph_id).into_string());
    println!("Nodes:      {}", intent.nodes.len());
    println!("Edges:      {}", intent.edges.len());
    println!("Lanes:      {}", lane_count);
    println!();
    println!("Nodes:");
    for (i, node) in intent.nodes.iter().enumerate() {
        println!("  [{}] {} (est. {} CU)", i, node.label, node.estimated_cu);
    }
    println!();
    println!("Edges:");
    for edge in &intent.edges {
        println!(
            "  {} -> {}  ({} -> {})",
            edge[0],
            edge[1],
            intent.nodes[edge[0] as usize].label,
            intent.nodes[edge[1] as usize].label,
        );
    }
    println!("========================================");
}
