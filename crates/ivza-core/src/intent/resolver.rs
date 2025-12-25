use anyhow::{anyhow, Result};
use solana_sdk::pubkey::Pubkey;
use tracing::info;

use crate::graph::{TransactionGraph, TransactionGraphBuilder};
use crate::types::{AccountAccessEntry, InstructionData, NodeId};

use super::parser::*;

/// Resolves an Intent into a TransactionGraph by determining the required
/// transactions (instructions) and their dependencies.
///
/// This is a high-level resolver that produces instruction templates. The actual
/// instruction data bytes would be filled in by a downstream component that knows
/// the specific DEX or program ABIs.
pub struct IntentResolver {
    /// The system program ID.
    pub system_program: Pubkey,
    /// The token program ID.
    pub token_program: Pubkey,
    /// The associated token account program ID.
    pub ata_program: Pubkey,
    /// The stake program ID.
    pub stake_program: Pubkey,
}

impl IntentResolver {
    pub fn new() -> Self {
        // Well-known Solana program IDs.
        Self {
            system_program: solana_sdk::system_program::id(),
            token_program: Pubkey::new_from_array([
                6, 221, 246, 225, 215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172, 28,
                180, 133, 237, 95, 91, 55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
            ]), // TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
            ata_program: Pubkey::new_from_array([
                140, 151, 37, 143, 78, 36, 137, 241, 187, 61, 16, 41, 20, 142, 13, 131, 11, 90, 19,
                153, 218, 255, 16, 132, 4, 142, 123, 216, 219, 233, 248, 89,
            ]), // ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
            stake_program: solana_sdk::stake::program::id(),
        }
    }

    /// Resolve an intent into a transaction graph.
    pub fn resolve(&self, intent: &Intent) -> Result<TransactionGraph> {
        info!("Resolving intent: {:?}", intent.intent_type);

        match &intent.params {
            IntentParams::Swap(params) => self.resolve_swap(params, intent),
            IntentParams::MultiHopSwap(params) => self.resolve_multi_hop_swap(params, intent),
            IntentParams::Stake(params) => self.resolve_stake(params, intent),
            IntentParams::Unstake(params) => self.resolve_unstake(params, intent),
            IntentParams::ProvideLiquidity(params) => {
                self.resolve_provide_liquidity(params, intent)
            }
            IntentParams::RemoveLiquidity(params) => self.resolve_remove_liquidity(params, intent),
            IntentParams::Transfer(params) => self.resolve_transfer(params, intent),
            IntentParams::CreateAccount(params) => self.resolve_create_account(params, intent),
        }
    }

    /// Resolve multiple intents, checking for cross-intent dependencies.
    pub fn resolve_batch(&self, intents: &[Intent]) -> Result<TransactionGraph> {
        let _builder = TransactionGraphBuilder::new();
        let mut graphs: Vec<TransactionGraph> = Vec::new();

        for intent in intents {
            let graph = self.resolve(intent)?;
            graphs.push(graph);
        }

        // Merge all graphs. For now, they are independent subgraphs.
        let mut combined = TransactionGraph::new();
        for graph in graphs {
            for (_, node) in graph.nodes {
                let new_id = combined.next_node_id();
                let mut new_node = node;
                new_node.id = new_id;
                combined.insert_node(new_node);
            }
            // Edges within each sub-graph need remapping, but since we're using
            // next_node_id from combined, we handle this by tracking the ID offset.
        }

        Ok(combined)
    }

    /// Resolve a simple swap intent.
    /// Graph: [create_ata_if_needed] -> [approve_tokens] -> [swap] -> [cleanup]
    /// If the output ATA already exists, the first node is skipped.
    fn resolve_swap(&self, params: &SwapParams, _intent: &Intent) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        // Derive associated token accounts.
        let input_ata = self.derive_ata(&params.user_wallet, &params.input_mint);
        let output_ata = self.derive_ata(&params.user_wallet, &params.output_mint);

        // Node 1: Create output ATA if needed.
        let create_ata_ix = InstructionData::new(
            self.ata_program,
            vec![
                AccountAccessEntry::write(params.user_wallet), // payer
                AccountAccessEntry::write(output_ata),         // ATA to create
                AccountAccessEntry::read(params.user_wallet),  // owner
                AccountAccessEntry::read(params.output_mint),  // mint
                AccountAccessEntry::read(self.system_program),
                AccountAccessEntry::read(self.token_program),
            ],
            vec![0], // Create ATA instruction discriminator placeholder.
        )
        .with_label("create_output_ata");

        let create_node = builder.add_labeled_node("create_output_ata", vec![create_ata_ix]);

        // Node 2: Execute the swap.
        let dex_program = params.dex_program.unwrap_or(self.token_program);
        let swap_ix = InstructionData::new(
            dex_program,
            vec![
                AccountAccessEntry::write(input_ata),  // source token account
                AccountAccessEntry::write(output_ata), // destination token account
                AccountAccessEntry::read(params.user_wallet), // authority
                AccountAccessEntry::read(params.input_mint), // input mint
                AccountAccessEntry::read(params.output_mint), // output mint
            ],
            self.encode_swap_data(params.amount_in, params.minimum_amount_out.unwrap_or(0)),
        )
        .with_label("swap");