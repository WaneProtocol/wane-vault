use std::str::FromStr;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use tracing::{info, warn};

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
        let tokens: Vec<&str> = input.split_whitespace().collect();
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

    fn parse_intent_type(&self, s: &str) -> Result<IntentType> {
        match s.to_lowercase().as_str() {
            "swap" => Ok(IntentType::Swap),
            "multi_hop_swap" | "multihop" | "multi-hop" => Ok(IntentType::MultiHopSwap),
            "stake" => Ok(IntentType::Stake),
            "unstake" => Ok(IntentType::Unstake),
            "provide_liquidity" | "add_liquidity" => Ok(IntentType::ProvideLiquidity),
            "remove_liquidity" => Ok(IntentType::RemoveLiquidity),
            "transfer" => Ok(IntentType::Transfer),
            "create_account" => Ok(IntentType::CreateAccount),
            _ => Err(anyhow!("Unknown intent type: {}", s)),
        }
    }

    fn parse_params(
        &self,
        intent_type: &IntentType,
        value: &serde_json::Value,
    ) -> Result<IntentParams> {
        match intent_type {
            IntentType::Swap => {
                let p: SwapParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::Swap(p))
            }
            IntentType::MultiHopSwap => {
                let p: MultiHopSwapParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::MultiHopSwap(p))
            }
            IntentType::Stake => {
                let p: StakeParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::Stake(p))
            }
            IntentType::Unstake => {
                let p: UnstakeParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::Unstake(p))
            }
            IntentType::ProvideLiquidity => {
                let p: ProvideLiquidityParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::ProvideLiquidity(p))
            }
            IntentType::RemoveLiquidity => {
                let p: RemoveLiquidityParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::RemoveLiquidity(p))
            }
            IntentType::Transfer => {
                let p: TransferParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::Transfer(p))
            }
            IntentType::CreateAccount => {
                let p: CreateAccountParams = serde_json::from_value(value.clone())?;
                Ok(IntentParams::CreateAccount(p))
            }
        }
    }

    // --- DSL parsers ---

    /// "swap <amount> <input_mint> for <output_mint> by <wallet>"
    fn parse_swap_dsl(&self, tokens: &[&str]) -> Result<Intent> {
        if tokens.len() < 6 {
            return Err(anyhow!(
                "Swap DSL format: swap <amount> <input_mint> for <output_mint> by <wallet>"
            ));
        }
        let amount: u64 = tokens[1].parse().map_err(|_| anyhow!("Invalid amount"))?;
        let input_mint = Pubkey::from_str(tokens[2]).map_err(|_| anyhow!("Invalid input_mint"))?;
        // tokens[3] should be "for"
        let output_mint =
            Pubkey::from_str(tokens[4]).map_err(|_| anyhow!("Invalid output_mint"))?;
        // tokens[5] should be "by"
        let wallet = Pubkey::from_str(tokens[6]).map_err(|_| anyhow!("Invalid wallet"))?;

        Ok(Intent::new(
            IntentType::Swap,
            IntentParams::Swap(SwapParams {
                input_mint,
                output_mint,
                amount_in: amount,
                minimum_amount_out: None,
                user_wallet: wallet,
                dex_program: None,
            }),
        ))
    }

    /// "stake <amount> to <validator> by <wallet>"
    fn parse_stake_dsl(&self, tokens: &[&str]) -> Result<Intent> {
        if tokens.len() < 5 {
            return Err(anyhow!(
                "Stake DSL format: stake <amount> to <validator> by <wallet>"
            ));
        }
        let amount: u64 = tokens[1].parse().map_err(|_| anyhow!("Invalid amount"))?;
        let validator =
            Pubkey::from_str(tokens[3]).map_err(|_| anyhow!("Invalid validator pubkey"))?;
        let wallet = Pubkey::from_str(tokens[5]).map_err(|_| anyhow!("Invalid wallet pubkey"))?;

        Ok(Intent::new(
            IntentType::Stake,
            IntentParams::Stake(StakeParams {
                amount,
                validator_vote_account: validator,
                user_wallet: wallet,
                stake_account: None,
            }),
        ))
    }

    /// "unstake <stake_account> by <wallet>"
    fn parse_unstake_dsl(&self, tokens: &[&str]) -> Result<Intent> {
        if tokens.len() < 4 {
            return Err(anyhow!(
                "Unstake DSL format: unstake <stake_account> by <wallet>"
            ));
        }
        let stake_account =
            Pubkey::from_str(tokens[1]).map_err(|_| anyhow!("Invalid stake account"))?;
        let wallet = Pubkey::from_str(tokens[3]).map_err(|_| anyhow!("Invalid wallet pubkey"))?;

        Ok(Intent::new(
            IntentType::Unstake,
            IntentParams::Unstake(UnstakeParams {
                stake_account,
                user_wallet: wallet,
            }),
        ))
    }

    /// "transfer <amount> <mint> from <wallet> to <wallet>"
    fn parse_transfer_dsl(&self, tokens: &[&str]) -> Result<Intent> {
        if tokens.len() < 7 {
            return Err(anyhow!(
                "Transfer DSL format: transfer <amount> <mint> from <from_wallet> to <to_wallet>"
            ));
        }
        let amount: u64 = tokens[1].parse().map_err(|_| anyhow!("Invalid amount"))?;
        let mint = Pubkey::from_str(tokens[2]).map_err(|_| anyhow!("Invalid mint"))?;
        let from = Pubkey::from_str(tokens[4]).map_err(|_| anyhow!("Invalid from wallet"))?;
        let to = Pubkey::from_str(tokens[6]).map_err(|_| anyhow!("Invalid to wallet"))?;

        Ok(Intent::new(
            IntentType::Transfer,
            IntentParams::Transfer(TransferParams {
                mint,
                amount,
                from_wallet: from,
                to_wallet: to,
            }),
        ))
    }

    /// "provide-liquidity <pool> <amount_a> <mint_a> <amount_b> <mint_b> by <wallet>"
    fn parse_provide_liquidity_dsl(&self, tokens: &[&str]) -> Result<Intent> {
        if tokens.len() < 8 {
            return Err(anyhow!(
                "Provide liquidity DSL format: provide-liquidity <pool> <amount_a> <mint_a> <amount_b> <mint_b> by <wallet>"
            ));
        }
        let pool = Pubkey::from_str(tokens[1]).map_err(|_| anyhow!("Invalid pool"))?;
        let amount_a: u64 = tokens[2].parse().map_err(|_| anyhow!("Invalid amount_a"))?;
        let mint_a = Pubkey::from_str(tokens[3]).map_err(|_| anyhow!("Invalid mint_a"))?;
        let amount_b: u64 = tokens[4].parse().map_err(|_| anyhow!("Invalid amount_b"))?;
        let mint_b = Pubkey::from_str(tokens[5]).map_err(|_| anyhow!("Invalid mint_b"))?;
        let wallet = Pubkey::from_str(tokens[7]).map_err(|_| anyhow!("Invalid wallet"))?;

        Ok(Intent::new(
            IntentType::ProvideLiquidity,
            IntentParams::ProvideLiquidity(ProvideLiquidityParams {
                pool,
                token_a_mint: mint_a,
                token_b_mint: mint_b,
                amount_a,
                amount_b,
                user_wallet: wallet,
            }),
        ))
    }

    /// "remove-liquidity <pool> <lp_amount> by <wallet>"
    fn parse_remove_liquidity_dsl(&self, tokens: &[&str]) -> Result<Intent> {
        if tokens.len() < 5 {
            return Err(anyhow!(
                "Remove liquidity DSL format: remove-liquidity <pool> <lp_amount> by <wallet>"
            ));
        }
        let pool = Pubkey::from_str(tokens[1]).map_err(|_| anyhow!("Invalid pool"))?;
        let lp_amount: u64 = tokens[2]
            .parse()
            .map_err(|_| anyhow!("Invalid lp_amount"))?;
        let wallet = Pubkey::from_str(tokens[4]).map_err(|_| anyhow!("Invalid wallet"))?;

        Ok(Intent::new(
            IntentType::RemoveLiquidity,
            IntentParams::RemoveLiquidity(RemoveLiquidityParams {
                pool,
                lp_amount,
                user_wallet: wallet,
            }),
        ))
    }
}

impl Default for IntentParser {
    fn default() -> Self {
        Self::new()
    }
}
