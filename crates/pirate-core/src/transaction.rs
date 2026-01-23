//! Transaction building with real Sapling crypto
//!
//! Integrates with zcash_primitives for proof generation and signing.

use crate::fees::FeeCalculator;
use crate::keys::{ExtendedSpendingKey, PaymentAddress};
use crate::params::sapling_prover;
use crate::selection::{NoteSelector, SelectableNote, SelectionStrategy};
use crate::{Error, Memo, Result};
use pirate_params::{Network, NetworkType};

use incrementalmerkletree::MerklePath;
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
        // Use the workspace zcash_address dependency
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

        Ok(PendingTransaction {
            temp_id: format!("pending-{}", hex::encode(rand::random::<[u8; 16]>())),
            outputs: self.outputs.clone(),
            input_value: total_input,
            output_value: output_sum,
            fee: actual_fee,
            change,
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

        let pending_outputs = self.outputs.clone();

        // Create prover from cached Sapling parameters (loaded once per process)
        let prover: LocalTxProver = sapling_prover();

        // Create transaction builder (Anchor is optional for Sapling-only)
        let mut tx_builder = TxBuilder::new(
            self.network.clone(),
            BlockHeight::from_u32(target_height),
            None, // No anchor specified - will be determined during build
        );

        // Add Sapling spends with witness data
        for note in &selection.notes {
            let diversifier = note.diversifier.ok_or_else(|| {
                Error::TransactionBuild("Missing diversifier for note".to_string())
            })?;
            let sapling_note = note
                .note
                .clone()
                .ok_or_else(|| Error::TransactionBuild("Missing Sapling note data".to_string()))?;
            let merkle_path: MerklePath<SaplingNode, { NOTE_COMMITMENT_TREE_DEPTH }> =
                note.merkle_path.clone().ok_or_else(|| {
                    Error::TransactionBuild("Missing Sapling witness path".to_string())
                })?;

            tx_builder
                .add_sapling_spend(
                    spending_key.inner().clone(),
                    diversifier,
                    sapling_note,
                    merkle_path,
                )
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
                .add_sapling_output(
                    Some(ovk),
                    output.address.inner,
                    Amount::from_i64(output.amount as i64)
                        .map_err(|_| Error::InvalidAmount("Amount out of range".to_string()))?,
                    memo_bytes,
                )
                .map_err(|e| {
                    Error::TransactionBuild(format!("Failed to add Sapling output: {:?}", e))
                })?;
        }

        // Add change output if needed
        if change > 10_000 {
            // Dust threshold
            let change_addr = spending_key
                .to_internal_fvk()
                .derive_address(change_diversifier_index)
                .inner;

            tx_builder
                .add_sapling_output(
                    Some(ovk),
                    change_addr,
                    Amount::from_i64(change as i64).map_err(|_| {
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
