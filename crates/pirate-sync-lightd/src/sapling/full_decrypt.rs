//! Full Sapling transaction decryption for memo extraction
//!
//! Parses raw transactions from GetTransaction RPC and decrypts full 580-byte
//! ciphertexts to extract memos using the Sapling note-encryption domain.
//!
//! Mirrors node Sapling decryption logic.

use crate::Error;
use group::ff::PrimeField;
use jubjub::Fr;
use sapling::{
    keys::PreparedIncomingViewingKey,
    note_encryption::{try_sapling_note_decryption, Zip212Enforcement},
    SaplingIvk,
};
use zcash_primitives::transaction::Transaction;
use zcash_protocol::consensus::BranchId;

/// Decrypted full note with memo
pub struct DecryptedFullNote {
    /// Note value in arrrtoshis.
    pub value: u64,
    /// Diversifier for the recipient address (11 bytes).
    pub diversifier: [u8; 11],
    /// Memo bytes (512 bytes, may contain null padding).
    pub memo: Vec<u8>, // 512 bytes, may contain null padding
}

/// Parse raw transaction and decrypt memo for a specific output (using IVK bytes)
///
/// Decrypts ciphertexts using the Sapling note decryption flow:
/// 1. Build the Sapling note-encryption domain
/// 2. Trial-decrypt the full output with the prepared incoming viewing key
/// 3. Return the decrypted value, diversifier, and memo bytes
///
/// # Arguments
/// * `raw_tx_bytes` - Raw transaction bytes from GetTransaction RPC
/// * `output_index` - Index of the Sapling output to decrypt
/// * `ivk_bytes` - Incoming viewing key bytes (32 bytes)
/// * `cmu` - Optional note commitment (from compact block, for validation)
///   If None, will use cmu from transaction output
///
/// # Returns
/// Decrypted note with memo, or None if decryption fails
pub fn decrypt_memo_from_raw_tx_with_ivk_bytes(
    raw_tx_bytes: &[u8],
    output_index: usize,
    ivk_bytes: &[u8; 32],
    cmu: Option<&[u8; 32]>,
) -> Result<Option<DecryptedFullNote>, Error> {
    // Parse transaction
    let tx = Transaction::read(raw_tx_bytes, BranchId::Canopy)
        .or_else(|_| Transaction::read(raw_tx_bytes, BranchId::Nu5))
        .map_err(|e| Error::Sync(format!("Failed to parse transaction: {}", e)))?;

    // Get Sapling bundle
    let sapling_bundle = tx
        .sapling_bundle()
        .ok_or_else(|| Error::Sync("Transaction has no Sapling bundle".to_string()))?;

    // Get outputs
    let outputs = sapling_bundle.shielded_outputs();
    if output_index >= outputs.len() {
        return Ok(None);
    }

    let output = &outputs[output_index];

    // Convert IVK bytes to the prepared Sapling incoming viewing key type.
    let ivk_fr = match Option::<Fr>::from(Fr::from_repr(*ivk_bytes)) {
        Some(s) => s,
        None => {
            return Err(Error::Sync("Invalid IVK bytes".to_string()));
        }
    };
    let sapling_ivk = SaplingIvk(ivk_fr);
    let prepared_ivk = PreparedIncomingViewingKey::new(&sapling_ivk);

    // Get cmu from output
    let note_commitment = output.cmu();

    // Validate against provided cmu if given
    if let Some(expected_cmu) = cmu {
        let cmu_bytes = note_commitment.to_bytes();
        if &cmu_bytes[..] != expected_cmu {
            tracing::debug!(
                "CMU mismatch: expected {}, got {}",
                hex::encode(expected_cmu),
                hex::encode(cmu_bytes)
            );
            return Ok(None);
        }
    }

    Ok(
        try_sapling_note_decryption(&prepared_ivk, output, Zip212Enforcement::GracePeriod).map(
            |(note, payment_address, memo)| DecryptedFullNote {
                value: note.value().inner(),
                diversifier: payment_address.diversifier().0,
                memo: memo.to_vec(),
            },
        ),
    )
}

/// Decode memo bytes to UTF-8 string
///
/// Memos are 512 bytes, with UTF-8 text followed by null padding.
/// If the first byte is > 0xF4, it's not a text memo.
pub fn decode_memo(memo: &[u8]) -> Option<String> {
    pirate_core::memo::Memo::decode_display_text(memo)
}
