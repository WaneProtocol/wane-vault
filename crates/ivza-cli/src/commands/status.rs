use anyhow::{bail, Context, Result};
use clap::Args;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use std::str::FromStr;
use tracing::info;

use crate::config::CliConfig;

/// Arguments for the `status` subcommand.
#[derive(Args, Debug)]
pub struct StatusArgs {
    /// The graph ID (base58 encoded) or graph PDA address.
    pub graph_id: String,

    /// Show detailed lane execution records.
    #[arg(short, long, default_value_t = false)]
    pub detailed: bool,

    /// Output as JSON instead of human-readable.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Seed constants matching the on-chain program.
const SEED_GRAPH: &[u8] = b"graph";
const SEED_EXECUTION: &[u8] = b"execution";

/// Deserialized on-chain graph state for display.
#[derive(Debug, Serialize, Deserialize)]
pub struct GraphStatusDisplay {
    pub graph_id: String,
    pub owner: String,
    pub node_count: u16,
    pub edge_count: u16,
    pub status: String,
    pub created_at: i64,
    pub lane_count: u8,
    pub lanes_executed: u32,
    pub lanes_succeeded: u8,
    pub lanes_failed: u8,
    pub total_compute_units: u64,
    pub execution_records: Vec<LaneRecordDisplay>,
}

/// Deserialized execution record for display.
#[derive(Debug, Serialize, Deserialize)]
pub struct LaneRecordDisplay {
    pub lane_index: u8,
    pub executor: String,
    pub executed_at: i64,
    pub success: bool,
    pub compute_units_used: u64,
}

/// Execute the status command.
pub async fn run(args: StatusArgs, cfg: &CliConfig) -> Result<()> {
    let program_id =
        Pubkey::from_str(&cfg.program_id).context("Invalid program ID in configuration")?;

    let client = RpcClient::new_with_commitment(
        cfg.rpc_url.clone(),
        CommitmentConfig {
            commitment: cfg.commitment_level(),
        },
    );

    // Determine the graph PDA. Input can be a base58-encoded graph ID or a direct PDA.
    let (graph_pda, graph_id_bytes) = resolve_graph_address(&args.graph_id, &program_id)?;

    info!("Fetching graph account: {}", graph_pda);

    let account_data = client
        .get_account_data(&graph_pda)
        .context("Failed to fetch graph account; it may not exist")?;

    // Parse the on-chain account data (skip 8-byte Anchor discriminator).
    let display = parse_graph_account(&account_data, &graph_id_bytes)?;

    // Fetch lane execution records if detailed mode is requested.
    let mut display = display;
    if args.detailed {
        display.execution_records =
            fetch_execution_records(&client, &graph_id_bytes, display.lane_count, &program_id)?;
    }

    // Output the result.
    if args.json {
        println!("{}", serde_json::to_string_pretty(&display)?);
    } else {
        print_status(&display);
    }

    Ok(())
}

/// Resolve input to a graph PDA and raw graph ID bytes.
fn resolve_graph_address(input: &str, program_id: &Pubkey) -> Result<(Pubkey, [u8; 32])> {
    // Try to decode as a 32-byte base58 graph ID.
    if let Ok(bytes) = bs58::decode(input).into_vec() {
        if bytes.len() == 32 {
            let mut graph_id = [0u8; 32];
            graph_id.copy_from_slice(&bytes);
            let (pda, _) =
                Pubkey::find_program_address(&[SEED_GRAPH, graph_id.as_ref()], program_id);
            return Ok((pda, graph_id));
        }
    }

    // Try as a direct Pubkey (PDA address).
    if let Ok(pubkey) = Pubkey::from_str(input) {
        // We don't know the graph_id; return zeros and read from account data.
        return Ok((pubkey, [0u8; 32]));
    }

    bail!(
        "Invalid graph identifier: '{}'. Provide a base58-encoded graph ID or PDA address.",
        input
    );
}

/// Parse the raw account data into a display-friendly struct.
///
/// Account layout (after 8-byte discriminator):
///   graph_id: [u8; 32]
///   owner: Pubkey (32 bytes)
///   nodes_data: Vec<u8> (4-byte len + data)
///   edges_data: Vec<u8> (4-byte len + data)
///   node_count: u16
///   edge_count: u16
///   status: u8
///   created_at: i64
///   lane_count: u8
///   lanes_executed: u32
///   lanes_succeeded: u8
///   lanes_failed: u8
///   total_compute_units: u64
///   bump: u8
///   _reserved: [u8; 32]
fn parse_graph_account(data: &[u8], _hint_graph_id: &[u8; 32]) -> Result<GraphStatusDisplay> {
    if data.len() < 8 + 32 + 32 + 4 {
        bail!("Account data too short to be a TransactionGraph");
    }

    let mut offset = 8; // skip discriminator

    // graph_id
    let mut graph_id = [0u8; 32];
    graph_id.copy_from_slice(&data[offset..offset + 32]);
    offset += 32;

    // owner
    let owner = Pubkey::try_from(&data[offset..offset + 32])
        .map_err(|_| anyhow::anyhow!("Failed to parse owner pubkey"))?;
    offset += 32;

    // nodes_data (skip over)
    let nodes_len = u32::from_le_bytes(data[offset..offset + 4].try_into()?) as usize;
    offset += 4 + nodes_len;

    // edges_data (skip over)
    let edges_len = u32::from_le_bytes(data[offset..offset + 4].try_into()?) as usize;
    offset += 4 + edges_len;

    // node_count
    let node_count = u16::from_le_bytes(data[offset..offset + 2].try_into()?);
    offset += 2;

    // edge_count
    let edge_count = u16::from_le_bytes(data[offset..offset + 2].try_into()?);
    offset += 2;

    // status
    let status_byte = data[offset];
    offset += 1;

    let status = match status_byte {
        0 => "Pending",
        1 => "Executing",
        2 => "Settled",
        3 => "Failed",
        _ => "Unknown",
    }
    .to_string();

    // created_at
    let created_at = i64::from_le_bytes(data[offset..offset + 8].try_into()?);
    offset += 8;

    // lane_count
    let lane_count = data[offset];
    offset += 1;

    // lanes_executed
    let lanes_executed = u32::from_le_bytes(data[offset..offset + 4].try_into()?);
    offset += 4;

    // lanes_succeeded
    let lanes_succeeded = data[offset];
    offset += 1;

    // lanes_failed
    let lanes_failed = data[offset];
    offset += 1;

    // total_compute_units
    let total_compute_units = u64::from_le_bytes(data[offset..offset + 8].try_into()?);

    Ok(GraphStatusDisplay {
        graph_id: bs58::encode(&graph_id).into_string(),
        owner: owner.to_string(),
        node_count,
        edge_count,
        status,
        created_at,
        lane_count,
        lanes_executed,
        lanes_succeeded,
        lanes_failed,
        total_compute_units,
        execution_records: Vec::new(),
    })
}

/// Fetch execution records for each lane of the graph.
fn fetch_execution_records(
    client: &RpcClient,
    graph_id: &[u8; 32],
    lane_count: u8,
    program_id: &Pubkey,
) -> Result<Vec<LaneRecordDisplay>> {
    let mut records = Vec::new();

    for lane_idx in 0..lane_count {
        let (pda, _) = Pubkey::find_program_address(
            &[SEED_EXECUTION, graph_id.as_ref(), &[lane_idx]],
            program_id,
        );

        match client.get_account_data(&pda) {
            Ok(data) => {
                if let Ok(record) = parse_execution_record(&data, lane_idx) {
                    records.push(record);
                }
            }
            Err(_) => {
                // Lane not yet executed; skip.
            }
        }
    }

    Ok(records)
}

/// Parse execution record account data.
///
/// Layout (after 8-byte discriminator):
///   graph_id: [u8; 32]
///   lane_index: u8
///   executor: Pubkey (32 bytes)
///   executed_at: i64
///   success: bool (u8)
///   compute_units_used: u64
///   bump: u8
///   _reserved: [u8; 16]
fn parse_execution_record(data: &[u8], expected_lane: u8) -> Result<LaneRecordDisplay> {
    if data.len() < 8 + 32 + 1 + 32 + 8 + 1 + 8 {
        bail!("Execution record data too short");
    }

    let mut offset = 8; // discriminator
    offset += 32; // graph_id (skip)

    let lane_index = data[offset];
    offset += 1;

    let executor = Pubkey::try_from(&data[offset..offset + 32])
        .map_err(|_| anyhow::anyhow!("Failed to parse executor pubkey"))?;
    offset += 32;

    let executed_at = i64::from_le_bytes(data[offset..offset + 8].try_into()?);
    offset += 8;

    let success = data[offset] != 0;
    offset += 1;

    let compute_units_used = u64::from_le_bytes(data[offset..offset + 8].try_into()?);

    if lane_index != expected_lane {
        bail!(
            "Lane index mismatch: expected {}, got {}",
            expected_lane,
            lane_index
        );
    }

    Ok(LaneRecordDisplay {
        lane_index,
        executor: executor.to_string(),
        executed_at,
        success,
        compute_units_used,
    })
}

/// Print a human-readable status display.
fn print_status(display: &GraphStatusDisplay) {
    println!("========================================");
    println!("  iVZA Graph Status");
    println!("========================================");
    println!("Graph ID:     {}", display.graph_id);
    println!("Owner:        {}", display.owner);
    println!("Status:       {}", display.status);
    println!("Created:      {} (unix)", display.created_at);
    println!();
    println!("Topology:");
    println!("  Nodes:      {}", display.node_count);
    println!("  Edges:      {}", display.edge_count);
    println!("  Lanes:      {}", display.lane_count);
    println!();
    println!("Execution:");
    let executed_count = display.lanes_executed.count_ones();
    println!("  Progress:   {}/{}", executed_count, display.lane_count);
    println!("  Succeeded:  {}", display.lanes_succeeded);
    println!("  Failed:     {}", display.lanes_failed);
    println!("  Total CU:   {}", display.total_compute_units);

    if !display.execution_records.is_empty() {
        println!();
        println!("Lane Details:");
        for rec in &display.execution_records {
            let status_symbol = if rec.success { "OK" } else { "FAIL" };
            println!(
                "  Lane {}: [{}] executor={} cu={} at={}",
                rec.lane_index,
                status_symbol,
                &rec.executor[..8],
                rec.compute_units_used,
                rec.executed_at,
            );
        }
    }

    // Lane execution bitmask visualization.
    println!();
    print!("Lane Map:     ");
    for i in 0..display.lane_count {
        if display.lanes_executed & (1u32 << i) != 0 {
            if i < display.lanes_succeeded {
                print!("[X]");
            } else {
                print!("[!]");
            }
        } else {
            print!("[ ]");
        }
    }
    println!();
    println!("========================================");
}
