//! Shielded transaction builder for Sapling and Orchard
//!
//! This module provides a unified transaction builder that uses
//! `zcash_primitives::transaction::builder::Builder` to construct
//! transactions with both Sapling and Orchard outputs.
//!
//! Based on the Rust builder path in the node (`builder_ffi.rs`), but
//! adapted for lightwalletd anchors.

use crate::fees::FeeCalculator;
use crate::keys::{ExtendedSpendingKey, OrchardExtendedSpendingKey, PaymentAddress};
use crate::params::sapling_prover;
use crate::selection::{NoteSelector, SelectableNote, SelectionStrategy};
use crate::{Error, Memo, Result};
use pirate_params::{Network, NetworkType};

use incrementalmerkletree::MerklePath;
use orchard::tree::Anchor as OrchardAnchor;
use zcash_primitives::{
    consensus::{BlockHeight, NetworkUpgrade, Parameters},
    memo::MemoBytes,
    sapling::{Node as SaplingNode, NOTE_COMMITMENT_TREE_DEPTH},
    transaction::{builder::Builder as TxBuilder, components::Amount, TxId},
};
use zcash_proofs::prover::LocalTxProver;

/// Pirate Chain network parameters
#[derive(Clone, Debug)]
pub struct PirateNetwork {
    network: Network,
}

impl PirateNetwork {
    /// Create parameters for the given network.
    pub fn new(network_type: NetworkType) -> Self {
        Self {
            network: Network::from_type(network_type),
        }
    }

    /// Mainnet parameters.
    pub fn mainnet() -> Self {
        Self::new(NetworkType::Mainnet)
    }
}

impl Default for PirateNetwork {
    fn default() -> Self {
        Self::mainnet()
    }
}

impl Parameters for PirateNetwork {
    fn coin_type(&self) -> u32 {
        self.network.coin_type
    }

    fn address_network(&self) -> Option<zcash_address::Network> {
        match self.network.network_type {
            NetworkType::Mainnet => Some(zcash_address::Network::Main),
            NetworkType::Testnet | NetworkType::Regtest => Some(zcash_address::Network::Test),
        }
    }

    fn hrp_sapling_extended_spending_key(&self) -> &str {
        match self.network.network_type {
            NetworkType::Mainnet => "secret-extended-key-main",
            NetworkType::Testnet => "secret-extended-key-test",
            NetworkType::Regtest => "secret-extended-key-regtest",
        }
    }

    fn hrp_sapling_extended_full_viewing_key(&self) -> &str {
        match self.network.network_type {
            NetworkType::Mainnet => "zxviews",
            NetworkType::Testnet => "zxviewtestsapling",
            NetworkType::Regtest => "zxviewregtestsapling",
        }
    }

    fn hrp_sapling_payment_address(&self) -> &str {
        match self.network.network_type {
            NetworkType::Mainnet => "zs",
            NetworkType::Testnet => "ztestsapling",
            NetworkType::Regtest => "zregtestsapling",
        }
    }

    fn b58_pubkey_address_prefix(&self) -> &[u8] {
        &[0x1C, 0xB8] // Pirate Chain P2PKH prefix
    }

    fn b58_script_address_prefix(&self) -> &[u8] {
        &[0x1C, 0xBD] // Pirate Chain P2SH prefix
    }

    fn activation_height(&self, nu: NetworkUpgrade) -> Option<BlockHeight> {
        match nu {
            NetworkUpgrade::Overwinter => match self.network.network_type {
                NetworkType::Mainnet => Some(BlockHeight::from_u32(152_855)),
                NetworkType::Testnet => Some(BlockHeight::from_u32(207_500)),
                NetworkType::Regtest => Some(BlockHeight::from_u32(50)),
            },
            NetworkUpgrade::Sapling => match self.network.network_type {
                NetworkType::Mainnet => Some(BlockHeight::from_u32(152_855)),
                NetworkType::Testnet => Some(BlockHeight::from_u32(280_000)),
                NetworkType::Regtest => Some(BlockHeight::from_u32(100)),
            },
            NetworkUpgrade::Nu5 => self
                .network
                .orchard_activation_height
                .map(BlockHeight::from_u32),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}

/// Shielded transaction output (Sapling or Orchard)
#[derive(Debug, Clone)]
pub enum ShieldedOutput {
    /// Sapling output
    Sapling {
        /// Destination Sapling address.
        address: PaymentAddress,
        /// Amount in arrrtoshis.
        amount: u64,
        /// Optional memo payload.
        memo: Option<Memo>,
    },
    /// Orchard output
    Orchard {
        /// Destination Orchard address.
        address: orchard::Address,
        /// Amount in arrrtoshis.
        amount: u64,
        /// Optional memo payload.
        memo: Option<Memo>,
    },
}

/// Pending shielded transaction (built but not signed)
#[derive(Debug, Clone)]
pub struct PendingShieldedTransaction {
    /// Transaction ID (temporary until signed)
    pub temp_id: String,
    /// Outputs
    pub outputs: Vec<ShieldedOutput>,
    /// Total input value
    pub input_value: u64,
    /// Total output value
    pub output_value: u64,
    /// Fee
    pub fee: u64,
    /// Change value
    pub change: u64,
}

/// Signed shielded transaction ready for broadcast
#[derive(Debug, Clone)]
pub struct SignedShieldedTransaction {
    /// Transaction ID
    pub txid: TxId,
    /// Raw transaction bytes
    pub raw_tx: Vec<u8>,
    /// Transaction size
    pub size: usize,
}

/// Shielded transaction builder with Sapling and Orchard support
#[derive(Debug)]
pub struct ShieldedBuilder {
    outputs: Vec<ShieldedOutput>,
    fee_override: Option<u64>,
    network: PirateNetwork,
    auto_consolidation_extra_limit: usize,
}

impl ShieldedBuilder {
    /// Create new shielded transaction builder
    pub fn new() -> Self {
        Self {
            outputs: Vec::new(),
            fee_override: None,
            network: PirateNetwork::default(),
            auto_consolidation_extra_limit: 0,
        }
    }

    /// Create new shielded transaction builder for the given network.
    pub fn with_network(network_type: NetworkType) -> Self {
        Self {
            outputs: Vec::new(),
            fee_override: None,
            network: PirateNetwork::new(network_type),
            auto_consolidation_extra_limit: 0,
        }
    }

    /// Set extra notes to include for auto-consolidation.
    pub fn with_auto_consolidation_extra_limit(&mut self, extra_limit: usize) -> &mut Self {
        self.auto_consolidation_extra_limit = extra_limit;
        self
    }

    /// Add Sapling output
    pub fn add_sapling_output(
        &mut self,
        address: PaymentAddress,
        amount: u64,
        memo: Option<Memo>,
    ) -> Result<&mut Self> {
        if amount == 0 {
            return Err(Error::InvalidAmount("Amount cannot be zero".to_string()));
        }

        if let Some(ref m) = memo {
            m.validate()?;
        }

        self.outputs.push(ShieldedOutput::Sapling {
            address,
            amount,
            memo,
        });

        Ok(self)
    }

    /// Add Orchard output
    pub fn add_orchard_output(
        &mut self,
        address: orchard::Address,
        amount: u64,
        memo: Option<Memo>,
    ) -> Result<&mut Self> {
        if amount == 0 {
            return Err(Error::InvalidAmount("Amount cannot be zero".to_string()));
        }

        if let Some(ref m) = memo {
            m.validate()?;
        }

        self.outputs.push(ShieldedOutput::Orchard {
            address,
            amount,
            memo,
        });

        Ok(self)
    }

    /// Set a fixed fee override (in arrrtoshis)
    pub fn with_fee_per_action(&mut self, fee: u64) -> &mut Self {
        self.fee_override = Some(fee);
        self
    }

    /// Build and sign shielded transaction
    ///
    /// # Arguments
    /// * `sapling_spending_key` - Sapling spending key (for Sapling spends/change)
    /// * `orchard_spending_key` - Orchard spending key (for Orchard spends/change, optional)
    /// * `available_notes` - Available notes for spending (both Sapling and Orchard)
    /// * `target_height` - Target block height for transaction
    /// * `orchard_anchor` - Orchard tree anchor (required if any Orchard outputs)
    /// * `change_diversifier_index` - Diversifier index for Sapling change address
    ///
    /// # Returns
    /// Signed transaction ready for broadcast
    pub async fn build_and_sign(
        &self,
        sapling_spending_key: &ExtendedSpendingKey,
        orchard_spending_key: Option<&OrchardExtendedSpendingKey>,
        available_notes: Vec<SelectableNote>,
        target_height: u32,
        orchard_anchor: Option<OrchardAnchor>,
        change_diversifier_index: u32,
    ) -> Result<SignedShieldedTransaction> {
        // Calculate required output amount
        let output_sum: u64 =
            self.outputs
                .iter()
                .map(|o| match o {
                    ShieldedOutput::Sapling { amount, .. }
                    | ShieldedOutput::Orchard { amount, .. } => *amount,
                })
                .sum();

        // Calculate fee
        let fee_calc = FeeCalculator::new();
        let has_memo = self.outputs.iter().any(|o| match o {
            ShieldedOutput::Sapling { memo, .. } | ShieldedOutput::Orchard { memo, .. } => {
                memo.is_some()
            }
        });

        // Estimate fee (fixed for Pirate, or override)
        let estimated_fee = match self.fee_override {
            Some(fee) => {
                fee_calc.validate_fee(fee)?;
                fee
            }
            None => fee_calc.calculate_fee(2, self.outputs.len(), has_memo)?,
        };

        // Select notes
        let selector = NoteSelector::new(SelectionStrategy::SmallestFirst);
        let selection = if self.auto_consolidation_extra_limit > 0 {
            selector.select_notes_with_consolidation(
                available_notes,
                output_sum,
                estimated_fee,
                self.auto_consolidation_extra_limit,
            )?
        } else {
            selector.select_notes(available_notes, output_sum, estimated_fee)?
        };

        // Get note count and check for Orchard spends before moving selection.notes
        let note_count = selection.notes.len();
        let total_input = selection.total_value;
        let has_orchard_spends = selection
            .notes
            .iter()
            .any(|n| n.note_type == crate::selection::NoteType::Orchard);

        // Recalculate fee with actual input count
        let actual_fee = match self.fee_override {
            Some(fee) => fee,
            None => fee_calc.calculate_fee(note_count, self.outputs.len(), has_memo)?,
        };

        // Calculate change
        let total_output = output_sum
            .checked_add(actual_fee)
            .ok_or_else(|| Error::AmountOverflow("Output + fee overflow".to_string()))?;

        let change = total_input.checked_sub(total_output).ok_or_else(|| {
            Error::InsufficientFunds(format!("Need {} but have {}", total_output, total_input))
        })?;

        // Create prover from cached Sapling parameters
        let prover: LocalTxProver = sapling_prover();

        // Create transaction builder with Orchard anchor
        let mut tx_builder = TxBuilder::new(
            self.network.clone(),
            BlockHeight::from_u32(target_height),
            orchard_anchor,
        );

        // Add Sapling and Orchard spends with witness data
        // Note: We iterate by value because Orchard MerklePath doesn't implement Clone
        for note in selection.notes {
            match note.note_type {
                crate::selection::NoteType::Sapling => {
                    if note.diversifier.is_some() {
                        let diversifier = note.diversifier.as_ref().ok_or_else(|| {
                            Error::TransactionBuild("Missing diversifier for note".to_string())
                        })?;
                        let sapling_note = note.note.as_ref().ok_or_else(|| {
                            Error::TransactionBuild("Missing Sapling note data".to_string())
                        })?;
                        let merkle_path: MerklePath<SaplingNode, { NOTE_COMMITMENT_TREE_DEPTH }> =
                            note.merkle_path
                                .as_ref()
                                .ok_or_else(|| {
                                    Error::TransactionBuild(
                                        "Missing Sapling witness path".to_string(),
                                    )
                                })?
                                .clone();

                        tx_builder
                            .add_sapling_spend(
                                sapling_spending_key.inner().clone(),
                                *diversifier,
                                sapling_note.clone(),
                                merkle_path,
                            )
                            .map_err(|e| {
                                Error::TransactionBuild(format!(
                                    "Failed to add Sapling spend: {:?}",
                                    e
                                ))
                            })?;
                    }
                }
                crate::selection::NoteType::Orchard => {
                    // For Orchard spends, we need the note, merkle path, and spending key
                    let orchard_note = note.orchard_note.as_ref().ok_or_else(|| {
                        Error::TransactionBuild("Missing Orchard note data".to_string())
                    })?;
                    let orchard_merkle_path = note.orchard_merkle_path.ok_or_else(|| {
                        Error::TransactionBuild("Missing Orchard merkle path".to_string())
                    })?;
                    let orchard_sk = orchard_spending_key.ok_or_else(|| {
                        Error::TransactionBuild(
                            "Orchard spending key required for Orchard spends".to_string(),
                        )
                    })?;

                    // Extract SpendingKey from OrchardExtendedSpendingKey
                    let sk = &orchard_sk.inner;

                    tx_builder
                        .add_orchard_spend::<()>(*sk, *orchard_note, orchard_merkle_path)
                        .map_err(|e| {
                            Error::TransactionBuild(format!("Failed to add Orchard spend: {:?}", e))
                        })?;
                }
            }
        }

        // Add outputs
        let sapling_ovk = sapling_spending_key
            .to_extended_fvk()
            .outgoing_viewing_key();
        let orchard_ovk = orchard_spending_key.map(|sk| sk.to_extended_fvk().to_ovk());
        for output in &self.outputs {
            match output {
                ShieldedOutput::Sapling {
                    address,
                    amount,
                    memo,
                } => {
                    let memo_bytes = match memo {
                        Some(m) => m.to_memo_bytes()?,
                        None => MemoBytes::empty(),
                    };

                    tx_builder
                        .add_sapling_output(
                            Some(sapling_ovk),
                            address.inner,
                            Amount::from_i64(*amount as i64).map_err(|_| {
                                Error::InvalidAmount("Amount out of range".to_string())
                            })?,
                            memo_bytes,
                        )
                        .map_err(|e| {
                            Error::TransactionBuild(format!(
                                "Failed to add Sapling output: {:?}",
                                e
                            ))
                        })?;
                }
                ShieldedOutput::Orchard {
                    address,
                    amount,
                    memo,
                } => {
                    let memo_bytes = match memo {
                        Some(m) => {
                            // Convert Memo to MemoBytes (same as Sapling)
                            m.to_memo_bytes()?
                        }
                        None => MemoBytes::empty(),
                    };

                    tx_builder
                        .add_orchard_output::<()>(
                            orchard_ovk.clone(),
                            *address,
                            *amount,
                            memo_bytes,
                        )
                        .map_err(|e| {
                            Error::TransactionBuild(format!(
                                "Failed to add Orchard output: {:?}",
                                e
                            ))
                        })?;
                }
            }
        }

        // Add change output if needed
        if change > 10_000 {
            // Dust threshold
            // Determine change address type:
            // - Use Orchard if we have Orchard outputs or Orchard spends
            // - Otherwise use Sapling
            // Note: has_orchard_spends was already computed before moving selection.notes
            let has_orchard_outputs = self
                .outputs
                .iter()
                .any(|o| matches!(o, ShieldedOutput::Orchard { .. }));
            let use_orchard_change = has_orchard_outputs || has_orchard_spends;

            if use_orchard_change {
                // Use Orchard change address
                let orchard_fvk = orchard_spending_key
                    .ok_or_else(|| {
                        Error::TransactionBuild(
                            "Orchard spending key required for Orchard change".to_string(),
                        )
                    })?
                    .to_extended_fvk();
                let change_addr = orchard_fvk.address_at_internal(change_diversifier_index);

                tx_builder
                    .add_orchard_output::<()>(
                        orchard_ovk.clone(),
                        change_addr.inner, // Extract inner orchard::Address
                        change,
                        MemoBytes::empty(),
                    )
                    .map_err(|e| {
                        Error::TransactionBuild(format!(
                            "Failed to add Orchard change output: {:?}",
                            e
                        ))
                    })?;
            } else {
                // Use Sapling change address
                let change_addr = sapling_spending_key
                    .to_internal_fvk()
                    .derive_address(change_diversifier_index)
                    .inner;

                tx_builder
                    .add_sapling_output(
                        Some(sapling_ovk),
                        change_addr,
                        Amount::from_i64(change as i64).map_err(|_| {
                            Error::InvalidAmount("Change amount out of range".to_string())
                        })?,
                        MemoBytes::empty(),
                    )
                    .map_err(|e| {
                        Error::TransactionBuild(format!(
                            "Failed to add Sapling change output: {:?}",
                            e
                        ))
                    })?;
            }
        }

        // Build transaction with fixed fee rule
        use zcash_primitives::transaction::fees::fixed::FeeRule;
        let fee_amount = Amount::from_u64(actual_fee)
            .map_err(|_| Error::InvalidAmount("Fee amount out of range".to_string()))?;
        let fee_rule = FeeRule::non_standard(fee_amount);
        let (tx, _tx_metadata) = tx_builder.build(&prover, &fee_rule).map_err(|e| {
            Error::TransactionBuild(format!("Failed to build transaction: {:?}", e))
        })?;

        // Serialize transaction to raw bytes
        let mut raw_tx = Vec::new();
        tx.write(&mut raw_tx).map_err(|e| {
            Error::TransactionBuild(format!("Failed to serialize transaction: {:?}", e))
        })?;

        let tx_size = raw_tx.len();
        let txid = tx.txid();

        Ok(SignedShieldedTransaction {
            txid,
            raw_tx,
            size: tx_size,
        })
    }
}

impl Default for ShieldedBuilder {
    fn default() -> Self {
        Self::new()
    }
}
