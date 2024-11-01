use std::collections::HashMap;
use std::str::FromStr;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tracing::{debug, info, warn};

/// A high-level intent representing what the user wants to accomplish.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Type of intent.
    pub intent_type: IntentType,
    /// Parameters for the intent.
    pub params: IntentParams,
    /// Optional user-provided label.
    pub label: Option<String>,
    /// Maximum acceptable slippage in basis points (for swaps).
    pub max_slippage_bps: Option<u64>,
    /// Priority fee the user is willing to pay.
    pub priority_fee: Option<u64>,
}

impl Intent {
    pub fn new(intent_type: IntentType, params: IntentParams) -> Self {
        Self {
            intent_type,
            params,
            label: None,
            max_slippage_bps: None,
            priority_fee: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_slippage(mut self, bps: u64) -> Self {
        self.max_slippage_bps = Some(bps);
        self
    }

    pub fn with_priority_fee(mut self, fee: u64) -> Self {
        self.priority_fee = Some(fee);
        self
    }
}

/// The type of intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentType {
    /// Swap token A for token B.
    Swap,
    /// Multi-hop swap through intermediate tokens.
    MultiHopSwap,
    /// Stake SOL to a validator.
    Stake,
    /// Unstake SOL from a validator.
    Unstake,
    /// Provide liquidity to a pool.
    ProvideLiquidity,
    /// Remove liquidity from a pool.
    RemoveLiquidity,
    /// Transfer tokens.
    Transfer,
    /// Create a token account.
    CreateAccount,
}

/// Parameters for the various intent types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentParams {
    Swap(SwapParams),
    MultiHopSwap(MultiHopSwapParams),
    Stake(StakeParams),
    Unstake(UnstakeParams),
    ProvideLiquidity(ProvideLiquidityParams),
    RemoveLiquidity(RemoveLiquidityParams),
    Transfer(TransferParams),
    CreateAccount(CreateAccountParams),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapParams {
    pub input_mint: Pubkey,
    pub output_mint: Pubkey,
    pub amount_in: u64,
    pub minimum_amount_out: Option<u64>,
    pub user_wallet: Pubkey,
    pub dex_program: Option<Pubkey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiHopSwapParams {
    pub route: Vec<Pubkey>, // mint addresses: [input, intermediate..., output]
    pub amount_in: u64,
    pub minimum_amount_out: Option<u64>,
    pub user_wallet: Pubkey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeParams {
    pub amount: u64,
    pub validator_vote_account: Pubkey,
    pub user_wallet: Pubkey,
    pub stake_account: Option<Pubkey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnstakeParams {
    pub stake_account: Pubkey,
    pub user_wallet: Pubkey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvideLiquidityParams {
    pub pool: Pubkey,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub amount_a: u64,
    pub amount_b: u64,
    pub user_wallet: Pubkey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveLiquidityParams {
    pub pool: Pubkey,
    pub lp_amount: u64,
    pub user_wallet: Pubkey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferParams {
    pub mint: Pubkey,
    pub amount: u64,
    pub from_wallet: Pubkey,
    pub to_wallet: Pubkey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountParams {
    pub mint: Pubkey,
    pub owner: Pubkey,
}

/// Parses high-level intent descriptions into structured Intent objects.
///
/// Supports parsing from JSON and from a simple DSL string format.
pub struct IntentParser;

impl IntentParser {
    pub fn new() -> Self {
        Self
    }

    /// Parse an intent from a JSON string.
    pub fn parse_json(&self, json: &str) -> Result<Intent> {
        let raw: serde_json::Value =
            serde_json::from_str(json).map_err(|e| anyhow!("Invalid JSON: {}", e))?;

        let intent_type_str = raw
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing 'type' field"))?;

        let intent_type = self.parse_intent_type(intent_type_str)?;

        let params_value = raw
            .get("params")
            .ok_or_else(|| anyhow!("Missing 'params' field"))?;

        let params = self.parse_params(&intent_type, params_value)?;

        let mut intent = Intent::new(intent_type, params);

        if let Some(label) = raw.get("label").and_then(|v| v.as_str()) {
            intent = intent.with_label(label);
        }
        if let Some(slippage) = raw.get("max_slippage_bps").and_then(|v| v.as_u64()) {
            intent = intent.with_slippage(slippage);
        }
        if let Some(fee) = raw.get("priority_fee").and_then(|v| v.as_u64()) {
            intent = intent.with_priority_fee(fee);
        }

        info!("Parsed intent: {:?}", intent.intent_type);
        Ok(intent)
    }

    /// Parse a batch of intents from a JSON array.
    pub fn parse_batch(&self, json: &str) -> Result<Vec<Intent>> {
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(json).map_err(|e| anyhow!("Invalid JSON array: {}", e))?;

        let mut intents = Vec::new();
        for (i, val) in arr.iter().enumerate() {
            let json_str = serde_json::to_string(val)?;
            match self.parse_json(&json_str) {
                Ok(intent) => intents.push(intent),
                Err(e) => {
                    warn!("Failed to parse intent {}: {}", i, e);
                    return Err(anyhow!("Failed to parse intent {}: {}", i, e));
                }
            }
        }
        Ok(intents)
    }

    /// Parse a simple DSL string format.
    /// Format: "swap <amount> <input_mint> for <output_mint> by <wallet>"
    ///         "stake <amount> to <validator> by <wallet>"
    ///         "transfer <amount> <mint> from <wallet> to <wallet>"
    pub fn parse_dsl(&self, input: &str) -> Result<Intent> {
        let tokens: Vec<&str> = input.trim().split_whitespace().collect();
        if tokens.is_empty() {
            return Err(anyhow!("Empty intent string"));
        }

        match tokens[0].to_lowercase().as_str() {
            "swap" => self.parse_swap_dsl(&tokens),
            "stake" => self.parse_stake_dsl(&tokens),
            "unstake" => self.parse_unstake_dsl(&tokens),
            "transfer" => self.parse_transfer_dsl(&tokens),
            "provide-liquidity" | "provide_liquidity" => self.parse_provide_liquidity_dsl(&tokens),
            "remove-liquidity" | "remove_liquidity" => self.parse_remove_liquidity_dsl(&tokens),
            _ => Err(anyhow!("Unknown intent type: {}", tokens[0])),
        }
    }

    