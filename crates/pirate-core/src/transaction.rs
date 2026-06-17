//! Transaction building with real Sapling crypto
//!
//! Integrates with the Rust Sapling primitives for proof generation and signing.

use crate::fees::{apply_dust_policy_add_to_fee, FeeCalculator, CHANGE_DUST_THRESHOLD};
use crate::keys::{ExtendedSpendingKey, PaymentAddress};
use crate::params::sapling_prover;
use crate::selection::{NoteSelector, SelectableNote, SelectionStrategy};
use crate::{Error, Memo, Result};
use pirate_params::{Network, NetworkType};

use incrementalmerkletree::MerklePath;
use sapling::{Anchor as SaplingAnchor, Node as SaplingNode, NOTE_COMMITMENT_TREE_DEPTH};
use zcash_primitives::transaction::{
    builder::{BuildConfig, Builder as TxBuilder},
    TxId,
};
use zcash_proofs::prover::LocalTxProver;
use zcash_protocol::{
    consensus::{BlockHeight, NetworkType as ConsensusNetworkType, NetworkUpgrade, Parameters},
    memo::MemoBytes,
    value::Zatoshis as Amount,
};
use zcash_transparent::builder::TransparentSigningSet;

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

    fn network_type(&self) -> NetworkType {
        self.network.network_type
    }
}

impl Default for PirateNetwork {
    fn default() -> Self {
        Self::mainnet()
    }
}

impl Parameters for PirateNetwork {
    fn network_type(&self) -> ConsensusNetworkType {
        match self.network.network_type {
            NetworkType::Mainnet => ConsensusNetworkType::Main,
            NetworkType::Testnet => ConsensusNetworkType::Test,
            NetworkType::Regtest => ConsensusNetworkType::Regtest,
        }
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

/// Transaction output (Sapling-only for now)
#[derive(Debug, Clone)]
pub struct TransactionOutput {
    /// Recipient address
    pub address: PaymentAddress,
    /// Amount in arrrtoshis
    pub amount: u64,
    /// Optional memo
    pub memo: Option<Memo>,
}

/// Pending transaction (built but not signed)
#[derive(Debug, Clone)]
pub struct PendingTransaction {
    /// Transaction ID (temporary until signed)
    pub temp_id: String,
    /// Outputs
    pub outputs: Vec<TransactionOutput>,
    /// Total input value
    pub input_value: u64,
    /// Total output value
    pub output_value: u64,
    /// Fee
    pub fee: u64,
    /// Change value
    pub change: u64,
}

/// Signed transaction ready for broadcast
#[derive(Debug, Clone)]
pub struct SignedTransaction {
    /// Transaction ID
    pub txid: TxId,
    /// Raw transaction bytes
    pub raw_tx: Vec<u8>,
    /// Transaction size
    pub size: usize,
}

/// Transaction builder with real Sapling integration
#[derive(Debug)]
pub struct TransactionBuilder {
    outputs: Vec<TransactionOutput>,
    fee_override: Option<u64>,
    network: PirateNetwork,
}

impl TransactionBuilder {
    /// Create new transaction builder
    pub fn new() -> Self {
        Self {
            outputs: Vec::new(),
            fee_override: None,
            network: PirateNetwork::default(),
        }
    }

    /// Create a transaction builder for the given network.
    pub fn with_network(network_type: NetworkType) -> Self {
        Self {
            outputs: Vec::new(),
            fee_override: None,
            network: PirateNetwork::new(network_type),
        }
    }

    /// Get number of outputs (for testing)
    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    /// Add output to transaction
    pub fn add_output(
        &mut self,
        address: PaymentAddress,
        amount: u64,
        memo: Option<Memo>,
    ) -> Result<&mut Self> {
        // Validate amount
        if amount == 0 {
            return Err(Error::InvalidAmount("Amount cannot be zero".to_string()));
        }

        // Validate memo if present
        if let Some(ref m) = memo {
            m.validate()?;
        }

        self.outputs.push(TransactionOutput {
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

    /// Build pending transaction (select notes, calculate fee, but don't sign)
    pub fn build_pending(
        &self,
        available_notes: Vec<SelectableNote>,
    ) -> Result<PendingTransaction> {
        // Calculate required output amount
        let output_sum = self.outputs.iter().map(|o| o.amount).sum::<u64>();

        // Calculate base fee (fixed for Pirate, or override)
        let fee_calc = FeeCalculator::new();
        let has_memo = self.outputs.iter().any(|o| o.memo.is_some());
        let estimated_fee = match self.fee_override {
            Some(fee) => {
                fee_calc.validate_fee(fee)?;
                fee
            }
            None => fee_calc.calculate_fee(2, self.outputs.len(), has_memo)?,
        };

        // Select notes
        let selector = NoteSelector::new(SelectionStrategy::SmallestFirst);
        let selection = selector.select_notes(available_notes, output_sum, estimated_fee)?;

        // Recalculate fee with actual input count
        let actual_fee = match self.fee_override {
            Some(fee) => fee,
            None => fee_calc.calculate_fee(selection.notes.len(), self.outputs.len(), has_memo)?,
        };

        // Calculate change
        let total_input = selection.total_value;
        let total_output = output_sum
            .checked_add(actual_fee)
            .ok_or_else(|| Error::AmountOverflow("Output + fee overflow".to_string()))?;

        let change = total_input.checked_sub(total_output).ok_or_else(|| {
            Error::InsufficientFunds(format!("Need {} but have {}", total_output, total_input))
        })?;
        let effective = apply_dust_policy_add_to_fee(actual_fee, change)?;

        Ok(PendingTransaction {
            temp_id: format!("pending-{}", hex::encode(rand::random::<[u8; 16]>())),
            outputs: self.outputs.clone(),
            input_value: total_input,
            output_value: output_sum,
            fee: effective.fee,
            change: effective.change,
        })
    }

    /// Build and sign transaction
    pub async fn build_and_sign(
        &self,
        spending_key: &ExtendedSpendingKey,
        available_notes: Vec<SelectableNote>,
        target_height: u32,
        change_diversifier_index: u32,
    ) -> Result<SignedTransaction> {
        // === Recompute pending transaction with selected notes so we know which inputs to spend ===
        let output_sum = self.outputs.iter().map(|o| o.amount).sum::<u64>();
        let fee_calc = FeeCalculator::new();
        let has_memo = self.outputs.iter().any(|o| o.memo.is_some());

        // First-pass fee estimate (inputs unknown yet)
        let estimated_fee = match self.fee_override {
            Some(fee) => {
                fee_calc.validate_fee(fee)?;
                fee
            }
            None => fee_calc.calculate_fee(2, self.outputs.len(), has_memo)?,
        };

        // Select notes
        let selector = NoteSelector::new(SelectionStrategy::SmallestFirst);
        let selection = selector.select_notes(available_notes, output_sum, estimated_fee)?;

        // Recalculate fee with actual input count
        let actual_fee = match self.fee_override {
            Some(fee) => fee,
            None => fee_calc.calculate_fee(selection.notes.len(), self.outputs.len(), has_memo)?,
        };

        // Calculate change
        let total_input = selection.total_value;
        let total_output = output_sum
            .checked_add(actual_fee)
            .ok_or_else(|| Error::AmountOverflow("Output + fee overflow".to_string()))?;

        let change = total_input.checked_sub(total_output).ok_or_else(|| {
            Error::InsufficientFunds(format!("Need {} but have {}", total_output, total_input))
        })?;
        let effective = apply_dust_policy_add_to_fee(actual_fee, change)?;
        let actual_fee = effective.fee;
        let change = effective.change;

        let pending_outputs = self.outputs.clone();

        // Create prover from cached Sapling parameters (loaded once per process)
        let prover: LocalTxProver = sapling_prover();

        let sapling_anchor = selection
            .notes
            .iter()
            .find_map(|note| {
                let sapling_note = note.note.as_ref()?;
                let merkle_path = note.merkle_path.as_ref()?;
                let cmu = sapling_note.cmu();
                Some(SaplingAnchor::from(
                    merkle_path.root(SaplingNode::from_cmu(&cmu)),
                ))
            })
            .ok_or_else(|| {
                Error::TransactionBuild(
                    "Missing Sapling anchor for selected Sapling spend set".to_string(),
                )
            })?;

        // Create transaction builder.
        let mut tx_builder = TxBuilder::new(
            self.network.clone(),
            BlockHeight::from_u32(target_height),
            BuildConfig::Standard {
                sapling_anchor: Some(sapling_anchor),
                orchard_anchor: None,
            },
        );

        let use_sapling_internal_change = crate::sapling_internal_change_active(
            self.network.network_type(),
            u64::from(target_height),
        );
        let mut first_legacy_sapling_change: Option<sapling::PaymentAddress> = None;

        // Add Sapling spends with witness data
        for note in &selection.notes {
            let sapling_note = note
                .note
                .clone()
                .ok_or_else(|| Error::TransactionBuild("Missing Sapling note data".to_string()))?;
            if first_legacy_sapling_change.is_none() {
                first_legacy_sapling_change = Some(sapling_note.recipient());
            }
            let merkle_path: MerklePath<SaplingNode, { NOTE_COMMITMENT_TREE_DEPTH }> =
                note.merkle_path.clone().ok_or_else(|| {
                    Error::TransactionBuild("Missing Sapling witness path".to_string())
                })?;

            tx_builder
                .add_sapling_spend::<()>(spending_key.full_viewing_key(), sapling_note, merkle_path)
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to add Sapling spend: {:?}", e))
                })?;
        }

        // Add outputs
        let ovk = spending_key.to_extended_fvk().outgoing_viewing_key();
        for output in &pending_outputs {
            let memo_bytes = match &output.memo {
                Some(m) => m.to_memo_bytes()?,
                None => MemoBytes::empty(),
            };

            tx_builder
                .add_sapling_output::<()>(
                    Some(ovk),
                    output.address.inner,
                    Amount::from_u64(output.amount)
                        .map_err(|_| Error::InvalidAmount("Amount out of range".to_string()))?,
                    memo_bytes,
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to add Sapling output: {:?}", e))
                })?;
        }

        // Add change output if needed
        if change >= CHANGE_DUST_THRESHOLD {
            let change_addr = if use_sapling_internal_change {
                spending_key
                    .to_internal_fvk()
                    .derive_address(change_diversifier_index)
                    .inner
            } else {
                first_legacy_sapling_change.ok_or_else(|| {
                    Error::TransactionBuild(
                        "Sapling legacy change requires a selected Sapling spend".to_string(),
                    )
                })?
            };

            tx_builder
                .add_sapling_output::<()>(
                    Some(ovk),
                    change_addr,
                    Amount::from_u64(change).map_err(|_| {
                        Error::InvalidAmount("Change amount out of range".to_string())
                    })?,
                    MemoBytes::empty(),
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to add change output: {:?}", e))
                })?;
        }

        // Build transaction with fixed fee rule
        use zcash_primitives::transaction::fees::fixed::FeeRule;
        let fee_amount = Amount::from_u64(actual_fee)
            .map_err(|_| Error::InvalidAmount("Fee amount out of range".to_string()))?;
        let fee_rule = FeeRule::non_standard(fee_amount);
        let transparent_signing_set = TransparentSigningSet::new();
        let sapling_extsks = [spending_key.inner().clone()];
        let orchard_saks = Vec::<orchard::keys::SpendAuthorizingKey>::new();
        let rng = rand::rngs::OsRng;
        let build_result = tx_builder
            .build(
                &transparent_signing_set,
                &sapling_extsks,
                &orchard_saks,
                rng,
                &prover,
                &prover,
                &fee_rule,
            )
            .map_err(|e| {
                Error::TransactionBuild(format!("Failed to build transaction: {:?}", e))
            })?;
        let tx = build_result.transaction();

        // Serialize transaction to raw bytes
        let mut raw_tx = Vec::new();
        tx.write(&mut raw_tx).map_err(|e| {
            Error::TransactionBuild(format!("Failed to serialize transaction: {:?}", e))
        })?;

        let tx_size = raw_tx.len();
        let txid = tx.txid();

        Ok(SignedTransaction {
            txid,
            raw_tx,
            size: tx_size,
        })
    }
}

impl Default for TransactionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::SelectableNote;

    #[test]
    fn test_builder_creation() {
        let builder = TransactionBuilder::new();
        assert_eq!(builder.outputs.len(), 0);
    }

    #[test]
    fn test_add_output() {
        let mut builder = TransactionBuilder::new();
        let addr = PaymentAddress::test_address();

        builder.add_output(addr, 100_000, None).unwrap();
        assert_eq!(builder.outputs.len(), 1);
    }

    #[test]
    fn test_add_output_zero_amount() {
        let mut builder = TransactionBuilder::new();
        let addr = PaymentAddress::test_address();

        let result = builder.add_output(addr, 0, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_pending() {
        let mut builder = TransactionBuilder::new();
        let addr = PaymentAddress::test_address();

        builder.add_output(addr, 100_000, None).unwrap();

        // Create mock notes
        let notes = vec![SelectableNote::new(150_000, vec![], 0, vec![], 0)];

        let pending = builder.build_pending(notes).unwrap();
        assert_eq!(pending.output_value, 100_000);
        assert!(pending.fee > 0);
        assert!(pending.change > 0);
    }

    #[test]
    fn test_insufficient_funds() {
        let mut builder = TransactionBuilder::new();
        let addr = PaymentAddress::test_address();

        builder.add_output(addr, 100_000, None).unwrap();

        // Create mock notes with insufficient value
        let notes = vec![SelectableNote::new(50_000, vec![], 0, vec![], 0)];

        let result = builder.build_pending(notes);
        assert!(result.is_err());
    }
}
