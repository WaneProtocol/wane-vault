use anyhow::{Context, Result};
use clap::Args;
use tracing::info;

use crate::config::CliConfig;

/// Arguments for the `config` subcommand.
#[derive(Args, Debug)]
pub struct ConfigArgs {
    /// Subcommand for config operations.
    #[command(subcommand)]
    pub action: ConfigAction,
}

/// Config subcommands.
#[derive(clap::Subcommand, Debug)]
pub enum ConfigAction {
    /// Display the current configuration.
    Show,

    /// Set a configuration value.
    Set(ConfigSetArgs),

    /// Initialize a new configuration file with defaults.
    Init(ConfigInitArgs),

    /// Display the path to the configuration file.
    Path,
}

/// Arguments for `config set`.
#[derive(Args, Debug)]
pub struct ConfigSetArgs {
    /// The configuration key to set.
    pub key: String,

    /// The value to set.
    pub value: String,
}

/// Arguments for `config init`.
#[derive(Args, Debug)]
pub struct ConfigInitArgs {
    /// Overwrite existing configuration file.
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

/// Execute the config command.
pub async fn run(args: ConfigArgs, cfg: &CliConfig) -> Result<()> {
    match args.action {
        ConfigAction::Show => show_config(cfg),
        ConfigAction::Set(set_args) => set_config(cfg, set_args),
        ConfigAction::Init(init_args) => init_config(init_args),
        ConfigAction::Path => show_path(),
    }
}

/// Display the current configuration.
fn show_config(cfg: &CliConfig) -> Result<()> {
    println!("========================================");
    println!("  iVZA CLI Configuration");
    println!("========================================");
    println!("RPC URL:        {}", cfg.rpc_url);
    println!("Keypair:        {}", cfg.keypair_path);
    println!("Program ID:     {}", cfg.program_id);
    println!("Commitment:     {}", cfg.commitment);
    println!(
        "CU Price:       {} micro-lamports",
        cfg.max_compute_unit_price
    );
    println!("Skip Preflight: {}", cfg.skip_preflight);
    println!("Timeout:        {}s", cfg.timeout_secs);
    println!("========================================");
    println!();
    println!(
        "Config file:    {}",
        CliConfig::default_config_path().display()
    );
    println!();
    println!("Environment variable overrides:");
    println!("  IVZA_RPC_URL      -> rpc_url");
    println!("  IVZA_KEYPAIR      -> keypair_path");
    println!("  IVZA_PROGRAM_ID   -> program_id");
    println!("  IVZA_COMMITMENT   -> commitment");
    println!("  IVZA_CU_PRICE     -> max_compute_unit_price");
    Ok(())
}

/// Set a configuration value and save.
fn set_config(cfg: &CliConfig, args: ConfigSetArgs) -> Result<()> {
    let mut config = cfg.clone();

    match args.key.as_str() {
        "rpc_url" | "rpc-url" => {
            config.rpc_url = args.value.clone();
            info!("Set rpc_url = {}", args.value);
        }
        "keypair" | "keypair_path" | "keypair-path" => {
            config.keypair_path = args.value.clone();
            info!("Set keypair_path = {}", args.value);
        }
        "program_id" | "program-id" => {
            // Validate it's a valid base58 pubkey.
            let _ = solana_sdk::pubkey::Pubkey::try_from(args.value.as_str())
                .map_err(|_| anyhow::anyhow!("Invalid pubkey: {}", args.value))?;
            config.program_id = args.value.clone();
            info!("Set program_id = {}", args.value);
        }
        "commitment" => {
            match args.value.as_str() {
                "processed" | "confirmed" | "finalized" => {}
                _ => {
                    anyhow::bail!(
                        "Invalid commitment level '{}'. Use: processed, confirmed, finalized",
                        args.value
                    );
                }
            }
            config.commitment = args.value.clone();
            info!("Set commitment = {}", args.value);
        }
        "cu_price" | "cu-price" | "max_compute_unit_price" => {
            let price: u64 = args.value.parse().context("CU price must be a valid u64")?;
            config.max_compute_unit_price = price;
            info!("Set max_compute_unit_price = {}", price);
        }
        "skip_preflight" | "skip-preflight" => {
            let val: bool = args
                .value
                .parse()
                .context("skip_preflight must be true or false")?;
            config.skip_preflight = val;
            info!("Set skip_preflight = {}", val);
        }
        "timeout" | "timeout_secs" | "timeout-secs" => {
            let secs: u64 = args.value.parse().context("timeout must be a valid u64")?;
            config.timeout_secs = secs;
            info!("Set timeout_secs = {}", secs);
        }
        other => {
            anyhow::bail!("Unknown configuration key: '{}'. Valid keys: rpc_url, keypair, program_id, commitment, cu_price, skip_preflight, timeout", other);
        }
    }

    config.save()?;
    println!(
        "Configuration saved to {}",
        CliConfig::default_config_path().display()
    );
    Ok(())
}

/// Initialize a new configuration file with defaults.
fn init_config(args: ConfigInitArgs) -> Result<()> {
    let path = CliConfig::default_config_path();

    if path.exists() && !args.force {
        anyhow::bail!(
            "Config file already exists at {}. Use --force to overwrite.",
            path.display()
        );
    }

    let config = CliConfig::default();
    config.save()?;
    println!("Configuration initialized at {}", path.display());
    println!();
    show_config(&config)?;
    Ok(())
}

/// Display the configuration file path.
fn show_path() -> Result<()> {
    let path = CliConfig::default_config_path();
    println!("{}", path.display());
    if path.exists() {
        println!("(file exists)");
    } else {
        println!("(file does not exist; run `ivza config init` to create)");
    }
    Ok(())
}
