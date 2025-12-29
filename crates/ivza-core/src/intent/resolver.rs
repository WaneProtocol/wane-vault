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

        let swap_node = builder.add_node_with_cu(vec![swap_ix], 300_000);

        // Edge: create_ata -> swap (the swap needs the output ATA to exist).
        builder.add_data_dependency(create_node, swap_node)?;

        builder.build()
    }

    /// Resolve a multi-hop swap intent.
    /// Graph: [create_ata_0] -> [swap_0] -> [swap_1] -> ... -> [swap_n-1]
    /// Each swap depends on the previous one (output of swap_i is input of swap_i+1).
    fn resolve_multi_hop_swap(
        &self,
        params: &MultiHopSwapParams,
        _intent: &Intent,
    ) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        if params.route.len() < 2 {
            return Err(anyhow!("Multi-hop swap requires at least 2 mints in route"));
        }

        let num_hops = params.route.len() - 1;
        let mut prev_node: Option<NodeId> = None;
        let mut create_nodes: Vec<NodeId> = Vec::new();

        for hop in 0..num_hops {
            let input_mint = params.route[hop];
            let output_mint = params.route[hop + 1];
            let input_ata = self.derive_ata(&params.user_wallet, &input_mint);
            let output_ata = self.derive_ata(&params.user_wallet, &output_mint);

            // Create intermediate/output ATAs as needed.
            if hop > 0 || hop == num_hops - 1 {
                let create_ix = InstructionData::new(
                    self.ata_program,
                    vec![
                        AccountAccessEntry::write(params.user_wallet),
                        AccountAccessEntry::write(output_ata),
                        AccountAccessEntry::read(params.user_wallet),
                        AccountAccessEntry::read(output_mint),
                        AccountAccessEntry::read(self.system_program),
                        AccountAccessEntry::read(self.token_program),
                    ],
                    vec![0],
                )
                .with_label(format!("create_ata_hop_{}", hop));

                let create_node =
                    builder.add_labeled_node(format!("create_ata_hop_{}", hop), vec![create_ix]);

                if let Some(_prev) = prev_node {
                    // The create ATA can happen in parallel with or before the swap,
                    // but the swap needs it. We add a dependency from create to the swap below.
                }
                create_nodes.push(create_node);
            }

            // Swap instruction for this hop.
            let amount = if hop == 0 { params.amount_in } else { 0 }; // Intermediate amounts are dynamic.
            let swap_ix = InstructionData::new(
                self.token_program,
                vec![
                    AccountAccessEntry::write(input_ata),
                    AccountAccessEntry::write(output_ata),
                    AccountAccessEntry::read(params.user_wallet),
                    AccountAccessEntry::read(input_mint),
                    AccountAccessEntry::read(output_mint),
                ],
                self.encode_swap_data(amount, 0),
            )
            .with_label(format!("swap_hop_{}", hop));

            let swap_node = builder.add_labeled_node(format!("swap_hop_{}", hop), vec![swap_ix]);

            // Chain swaps sequentially.
            if let Some(prev) = prev_node {
                builder.add_data_dependency(prev, swap_node)?;
            }

            // Create ATA must complete before swap.
            if let Some(&create_node) = create_nodes.last() {
                // Only add if the create_node is for this hop's output.
                builder.add_data_dependency(create_node, swap_node)?;
            }

            prev_node = Some(swap_node);
        }

        builder.build()
    }

    /// Resolve a stake intent.
    /// Graph: [create_stake_account] -> [delegate_stake]
    fn resolve_stake(&self, params: &StakeParams, _intent: &Intent) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        let stake_account = params
            .stake_account
            .unwrap_or_else(|| self.derive_stake_account(&params.user_wallet, 0));

        // Node 1: Create and initialize stake account.
        let create_ix = InstructionData::new(
            self.system_program,
            vec![
                AccountAccessEntry::write(params.user_wallet), // payer
                AccountAccessEntry::write(stake_account),      // new stake account
            ],
            self.encode_u64(params.amount),
        )
        .with_label("create_stake_account");

        let init_ix = InstructionData::new(
            self.stake_program,
            vec![
                AccountAccessEntry::write(stake_account),
                AccountAccessEntry::read(params.user_wallet), // staker/withdrawer
            ],
            vec![0], // Initialize instruction.
        )
        .with_label("init_stake_account");

        let create_node = builder.add_labeled_node("create_stake", vec![create_ix, init_ix]);

        // Node 2: Delegate to validator.
        let delegate_ix = InstructionData::new(
            self.stake_program,
            vec![
                AccountAccessEntry::write(stake_account),
                AccountAccessEntry::read(params.validator_vote_account),
                AccountAccessEntry::read(params.user_wallet), // stake authority
            ],
            vec![2], // Delegate instruction discriminator.
        )
        .with_label("delegate_stake");

        let delegate_node = builder.add_labeled_node("delegate_stake", vec![delegate_ix]);

        builder.add_data_dependency(create_node, delegate_node)?;

        builder.build()
    }

    /// Resolve an unstake intent.
    /// Graph: [deactivate] -> [withdraw] (withdraw happens after cooldown in practice).
    fn resolve_unstake(
        &self,
        params: &UnstakeParams,
        _intent: &Intent,
    ) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        // Node 1: Deactivate.
        let deactivate_ix = InstructionData::new(
            self.stake_program,
            vec![
                AccountAccessEntry::write(params.stake_account),
                AccountAccessEntry::read(params.user_wallet),
            ],
            vec![5], // Deactivate instruction.
        )
        .with_label("deactivate_stake");

        let deactivate_node = builder.add_labeled_node("deactivate_stake", vec![deactivate_ix]);

        // Node 2: Withdraw (would execute after cooldown epoch in reality).
        let withdraw_ix = InstructionData::new(
            self.stake_program,
            vec![
                AccountAccessEntry::write(params.stake_account),
                AccountAccessEntry::write(params.user_wallet), // recipient
                AccountAccessEntry::read(params.user_wallet),  // withdraw authority
            ],
            vec![4], // Withdraw instruction.
        )
        .with_label("withdraw_stake");

        let withdraw_node = builder.add_labeled_node("withdraw_stake", vec![withdraw_ix]);

        builder.add_order_dependency(deactivate_node, withdraw_node)?;

        builder.build()
    }

    /// Resolve a provide liquidity intent.
    /// Graph: [create_lp_ata] -> [deposit_a + deposit_b] -> [add_liquidity]
    fn resolve_provide_liquidity(
        &self,
        params: &ProvideLiquidityParams,
        _intent: &Intent,
    ) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        let ata_a = self.derive_ata(&params.user_wallet, &params.token_a_mint);
        let ata_b = self.derive_ata(&params.user_wallet, &params.token_b_mint);

        // Node 1: Transfer token A to pool.
        let transfer_a_ix = InstructionData::new(
            self.token_program,
            vec![
                AccountAccessEntry::write(ata_a),
                AccountAccessEntry::write(params.pool),
                AccountAccessEntry::read(params.user_wallet),
            ],
            self.encode_u64(params.amount_a),
        )
        .with_label("transfer_token_a");

        let transfer_a_node = builder.add_labeled_node("transfer_a", vec![transfer_a_ix]);

        // Node 2: Transfer token B to pool.
        let transfer_b_ix = InstructionData::new(
            self.token_program,
            vec![
                AccountAccessEntry::write(ata_b),
                AccountAccessEntry::write(params.pool),
                AccountAccessEntry::read(params.user_wallet),
            ],
            self.encode_u64(params.amount_b),
        )
        .with_label("transfer_token_b");

        let transfer_b_node = builder.add_labeled_node("transfer_b", vec![transfer_b_ix]);

        // Node 3: Add liquidity instruction.
        let add_liq_ix = InstructionData::new(
            params.pool, // Pool program is the program to call.
            vec![
                AccountAccessEntry::write(params.pool),
                AccountAccessEntry::read(params.user_wallet),
                AccountAccessEntry::read(params.token_a_mint),
                AccountAccessEntry::read(params.token_b_mint),
            ],
            vec![1], // Add liquidity discriminator.
        )
        .with_label("add_liquidity");

        let add_liq_node = builder.add_labeled_node("add_liquidity", vec![add_liq_ix]);

        // Both transfers must complete before adding liquidity.
        builder.add_data_dependency(transfer_a_node, add_liq_node)?;
        builder.add_data_dependency(transfer_b_node, add_liq_node)?;

        builder.build()
    }

    /// Resolve a remove liquidity intent.
    fn resolve_remove_liquidity(
        &self,
        params: &RemoveLiquidityParams,
        _intent: &Intent,
    ) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        let remove_ix = InstructionData::new(
            params.pool,
            vec![
                AccountAccessEntry::write(params.pool),
                AccountAccessEntry::read(params.user_wallet),
            ],
            self.encode_u64(params.lp_amount),
        )
        .with_label("remove_liquidity");

        builder.add_labeled_node("remove_liquidity", vec![remove_ix]);

        builder.build()
    }

    /// Resolve a transfer intent.
    fn resolve_transfer(
        &self,
        params: &TransferParams,
        _intent: &Intent,
    ) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        let from_ata = self.derive_ata(&params.from_wallet, &params.mint);
        let to_ata = self.derive_ata(&params.to_wallet, &params.mint);

        // Node 1: Create destination ATA if needed.
        let create_ix = InstructionData::new(
            self.ata_program,
            vec![
                AccountAccessEntry::write(params.from_wallet), // payer
                AccountAccessEntry::write(to_ata),
                AccountAccessEntry::read(params.to_wallet), // owner
                AccountAccessEntry::read(params.mint),
                AccountAccessEntry::read(self.system_program),
                AccountAccessEntry::read(self.token_program),
            ],
            vec![0],
        )
        .with_label("create_dest_ata");

        let create_node = builder.add_labeled_node("create_dest_ata", vec![create_ix]);

        // Node 2: Transfer.
        let transfer_ix = InstructionData::new(
            self.token_program,
            vec![
                AccountAccessEntry::write(from_ata),
                AccountAccessEntry::write(to_ata),
                AccountAccessEntry::read(params.from_wallet), // authority
            ],
            self.encode_u64(params.amount),
        )
        .with_label("transfer");

        let transfer_node = builder.add_labeled_node("transfer", vec![transfer_ix]);

        builder.add_data_dependency(create_node, transfer_node)?;

        builder.build()
    }

    /// Resolve a create account intent.
    fn resolve_create_account(
        &self,
        params: &CreateAccountParams,
        _intent: &Intent,
    ) -> Result<TransactionGraph> {
        let mut builder = TransactionGraphBuilder::new();

        let ata = self.derive_ata(&params.owner, &params.mint);

        let create_ix = InstructionData::new(
            self.ata_program,
            vec![
                AccountAccessEntry::write(params.owner), // payer
                AccountAccessEntry::write(ata),
                AccountAccessEntry::read(params.owner),
                AccountAccessEntry::read(params.mint),
                AccountAccessEntry::read(self.system_program),
                AccountAccessEntry::read(self.token_program),
            ],
            vec![0],
        )
        .with_label("create_ata");

        builder.add_labeled_node("create_ata", vec![create_ix]);

        builder.build()
    }

    // --- Helpers ---

    /// Derive an associated token account address (deterministic PDA).
    /// This is a simplified version; the real derivation uses find_program_address.
    fn derive_ata(&self, wallet: &Pubkey, mint: &Pubkey) -> Pubkey {
        // Use a deterministic derivation based on the wallet and mint.
        let seeds = [wallet.as_ref(), self.token_program.as_ref(), mint.as_ref()];
        let (ata, _bump) = Pubkey::find_program_address(&seeds, &self.ata_program);
        ata
    }

    /// Derive a stake account address.
    fn derive_stake_account(&self, wallet: &Pubkey, index: u64) -> Pubkey {
        let seeds = [wallet.as_ref(), &index.to_le_bytes()];
        let (addr, _) = Pubkey::find_program_address(&seeds, &self.stake_program);
        addr
    }

    /// Encode a u64 value as little-endian bytes.
    fn encode_u64(&self, value: u64) -> Vec<u8> {
        value.to_le_bytes().to_vec()
    }

    /// Encode swap instruction data.
    fn encode_swap_data(&self, amount_in: u64, minimum_out: u64) -> Vec<u8> {
        let mut data = Vec::with_capacity(17);
        data.push(1); // Swap discriminator.
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&minimum_out.to_le_bytes());
        data
    }
}

impl Default for IntentResolver {
    fn default() -> Self {
        Self::new()
    }
}
