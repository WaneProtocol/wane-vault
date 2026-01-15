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
            if default_path.exists() {
                Self::load_from_file(&default_path)?
            } else {
                Self::default()
            }
        };

        // Override with environment variables if set.
        if let Ok(url) = std::env::var("IVZA_RPC_URL") {
            config.rpc_url = url;
        }
        if let Ok(kp) = std::env::var("IVZA_KEYPAIR") {
            config.keypair_path = kp;
        }
        if let Ok(pid) = std::env::var("IVZA_PROGRAM_ID") {
            config.program_id = pid;
        }
        if let Ok(c) = std::env::var("IVZA_COMMITMENT") {
            config.commitment = c;
        }
        if let Ok(price) = std::env::var("IVZA_CU_PRICE") {
            if let Ok(v) = price.parse::<u64>() {
                config.max_compute_unit_price = v;
            }
        }

        Ok(config)
    }

    /// Load configuration from a specific file path.
    fn load_from_file(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let config: Self = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;
        Ok(config)
    }

    /// Save the current configuration to disk at the default location.
    pub fn save(&self) -> Result<()> {
        let path = Self::default_config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
        }
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, data)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
    }

    /// Returns the default config file path: ~/.ivza/config.json
    pub fn default_config_path() -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(CONFIG_DIR).join(CONFIG_FILE)
    }

    /// Parse the commitment string into a CommitmentLevel.
    pub fn commitment_level(&self) -> CommitmentLevel {
        match self.commitment.as_str() {
            "processed" => CommitmentLevel::Processed,
            "confirmed" => CommitmentLevel::Confirmed,
            "finalized" => CommitmentLevel::Finalized,
            _ => CommitmentLevel::Confirmed,
        }
    }
}

/// Resolve the default keypair path based on the user's home directory.
fn dirs_default_keypair() -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(DEFAULT_KEYPAIR_REL)
        .to_string_lossy()
        .to_string()
}
