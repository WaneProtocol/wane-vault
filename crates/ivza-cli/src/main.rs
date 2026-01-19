use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod commands;
mod config;

use commands::{AnalyzeArgs, ConfigArgs, SolveArgs, StatusArgs, SubmitArgs};

/// iVZA Parallel Execution Engine CLI
///
/// Submit, monitor, and analyze transaction graphs for parallel execution
/// on the Solana blockchain.
#[derive(Parser, Debug)]
#[command(name = "ivza", version, about, long_about = None)]
pub struct Cli {
    /// Path to a custom configuration file.
    #[arg(long, global = true)]
    config: Option<String>,

    /// Override the RPC URL.
    #[arg(long, global = true)]
    rpc_url: Option<String>,

    /// Override the keypair path.
    #[arg(long, global = true)]
    keypair: Option<String>,

    /// Subcommand to execute.
    #[command(subcommand)]
    command: Command,
}

/// Available CLI subcommands.
#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Submit a transaction graph for parallel execution.
    Submit(SubmitArgs),

    /// Query the status of a submitted graph.
    Status(StatusArgs),

    /// Analyze a graph offline without submitting.
    Analyze(AnalyzeArgs),

    /// View or update CLI configuration.
    Config(ConfigArgs),

    /// Run the solver on an intent to produce an optimal execution plan.
    Solve(SolveArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing / logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    // Load configuration, applying any CLI overrides.
    let mut cfg = config::CliConfig::load(cli.config.as_deref())?;
    if let Some(ref url) = cli.rpc_url {
        cfg.rpc_url = url.clone();
    }
    if let Some(ref kp) = cli.keypair {
        cfg.keypair_path = kp.clone();
    }

    // Dispatch to the appropriate subcommand.
    match cli.command {
        Command::Submit(args) => commands::submit::run(args, &cfg).await,
        Command::Status(args) => commands::status::run(args, &cfg).await,
        Command::Analyze(args) => commands::analyze::run(args, &cfg).await,
        Command::Config(args) => commands::config_cmd::run(args, &cfg).await,
        Command::Solve(args) => commands::solve::run(args, &cfg).await,
    }
}
