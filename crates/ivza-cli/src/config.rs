use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::commitment_config::CommitmentLevel;
use std::path::{Path, PathBuf};

/// Default RPC endpoint (Solana devnet).
const DEFAULT_RPC_URL: &str = "https://api.devnet.solana.com";

/// Default program ID for the iVZA engine.
const DEFAULT_PROGRAM_ID: &str = "iVZAeng1neXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";

/// Default keypair location relative to home.
const DEFAULT_KEYPAIR_REL: &str = ".config/solana/id.json";

/// Configuration directory name under the user's home.
const CONFIG_DIR: &str = ".ivza";

/// Configuration file name.
const CONFIG_FILE: &str = "config.json";

/// CLI configuration for connecting to the iVZA engine on Solana.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    /// Solana RPC endpoint URL.
    pub rpc_url: String,

    /// Path to the signer keypair file.
    pub keypair_path: String,

    /// The on-chain program ID for ivza-engine.
    pub program_id: String,

    /// Transaction commitment level.
    pub commitment: String,

    /// Maximum compute unit price to pay (in micro-lamports).
    #[serde(default = "default_cu_price")]
    pub max_compute_unit_price: u64,

    /// Whether to skip preflight simulation.
    #[serde(default)]
    pub skip_preflight: bool,

    /// Custom timeout in seconds for RPC calls.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_cu_price() -> u64 {
    50_000
}

fn default_timeout() -> u64 {
    30
}

impl Default for CliConfig {
    fn default() -> Self {
        let keypair_path = dirs_default_keypair();
        Self {
            rpc_url: DEFAULT_RPC_URL.to_string(),
            keypair_path,
            program_id: DEFAULT_PROGRAM_ID.to_string(),
            commitment: "confirmed".to_string(),
            max_compute_unit_price: default_cu_price(),
            skip_preflight: false,
            timeout_secs: default_timeout(),
        }
    }
}

impl CliConfig {
    /// Load configuration from disk, environment variables, or defaults.
    ///
    /// Priority: explicit path > ~/.ivza/config.json > env vars > defaults.
    pub fn load(explicit_path: Option<&str>) -> Result<Self> {
        let mut config = if let Some(path) = explicit_path {
            Self::load_from_file(Path::new(path))?
        } else {
            let default_path = Self::default_config_path();