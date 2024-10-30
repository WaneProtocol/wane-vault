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

