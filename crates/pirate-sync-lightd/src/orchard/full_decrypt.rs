//! Orchard full transaction decryption for memo extraction
//!
//! Uses zcash_note_encryption for Orchard note handling.
//! References node Orchard full decryption logic.
//! - pirate/src/rust/src/orchard_actions.rs (try_orchard_decrypt_action_ivk)

use orchard::{
    keys::{IncomingViewingKey, PreparedIncomingViewingKey},
    note_encryption::OrchardDomain,
    primitives::redpallas::{Signature, SpendAuth},
    Action as OrchardAction,
};
use zcash_note_encryption::try_note_decryption;
use zcash_primitives::transaction::Transaction;
use zcash_primitives::consensus::BranchId;
use crate::Error;
use hex;
use tracing;

/// Decrypted Orchard note with memo
pub struct DecryptedOrchardFullNote {
    /// Note value in arrrtoshis.
    pub value: u64,
    /// Raw Orchard payment address bytes (43 bytes).
    pub address: [u8; 43],
    /// Memo bytes (512 bytes).
    pub memo: [u8; 512],
    /// Note rho bytes (32 bytes).
    pub rho: [u8; 32],
    /// Note rseed bytes (32 bytes).
    pub rseed: [u8; 32],
}

/// Decrypt an Orchard action from a full transaction
///
/// Uses zcash_note_encryption::try_note_decryption with the node's domain setup.
/// The node uses `orchard::Action<Signature<SpendAuth>>` directly.
///
/// # Arguments
/// * `action` - Orchard action from transaction (orchard::Action<Signature<SpendAuth>>)
/// * `ivk_bytes` - Incoming viewing key bytes (64 bytes for Orchard)
///
/// # Returns
/// Decrypted note with memo, or None if decryption fails
pub fn decrypt_orchard_action_with_ivk_bytes(
    action: &OrchardAction<Signature<SpendAuth>>,
    ivk_bytes: &[u8; 64],
) -> Result<Option<DecryptedOrchardFullNote>, Error> {
    let ivk_ct = IncomingViewingKey::from_bytes(ivk_bytes);
    if !bool::from(ivk_ct.is_some()) {
        return Err(Error::Sync("Invalid Orchard IVK bytes".to_string()));
    }
    let ivk = ivk_ct.unwrap();
    
    let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);
    
    // Create the Orchard domain for this action.
    let domain = OrchardDomain::for_action(action);
    
    // Use zcash_note_encryption::try_note_decryption.
    // Full node signature: try_note_decryption(&domain, &prepared_ivk, action)
    match try_note_decryption(&domain, &prepared_ivk, action) {
        Some((note, payment_address, memo_bytes)) => {
            // Extract value
            let value = note.value().inner();
            
            // Extract address (43 bytes)
            let address = payment_address.to_raw_address_bytes();
            
            // Extract memo (512 bytes)
            let mut memo = [0u8; 512];
            memo.copy_from_slice(&memo_bytes[..512]);
            
            // Extract rho (32 bytes)
            let rho = note.rho().to_bytes();
            
            // Extract rseed (32 bytes)
            let rseed = *note.rseed().as_bytes();
            
            Ok(Some(DecryptedOrchardFullNote {
                value,
                address,
                memo,
                rho,
                rseed,
            }))
        }
        None => Ok(None),
    }
}

/// Parse raw transaction and decrypt Orchard action for a specific action index (using IVK bytes)
///
/// This function parses the raw transaction bytes, extracts the Orchard bundle,
/// and decrypts the action at the given index using the IVK.
///
/// # Arguments
/// * `raw_tx_bytes` - Raw transaction bytes from GetTransaction RPC
/// * `action_index` - Index of the Orchard action to decrypt
/// * `ivk_bytes` - Incoming viewing key bytes (64 bytes for Orchard)
/// * `cmx` - Optional note commitment (from compact block, for validation)
///           If None, will use cmx from transaction action
///
/// # Returns
/// Decrypted note with memo and address, or None if decryption fails
pub fn decrypt_orchard_memo_from_raw_tx_with_ivk_bytes(
    raw_tx_bytes: &[u8],
    action_index: usize,
    ivk_bytes: &[u8; 64],
    cmx: Option<&[u8; 32]>,
) -> Result<Option<DecryptedOrchardFullNote>, Error> {
    // Parse transaction
    let tx = Transaction::read(raw_tx_bytes, BranchId::Nu5)
        .or_else(|_| Transaction::read(raw_tx_bytes, BranchId::Canopy))
        .map_err(|e| Error::Sync(format!("Failed to parse transaction: {}", e)))?;

    // Get Orchard bundle
    let orchard_bundle = tx.orchard_bundle()
        .ok_or_else(|| Error::Sync("Transaction has no Orchard bundle".to_string()))?;

    // Get actions from the bundle
    // The bundle's actions() method returns a slice of actions
    let actions = orchard_bundle.actions();
    if action_index >= actions.len() {
        tracing::debug!(
            "Orchard action index {} out of range ({} actions)",
            action_index,
            actions.len()
        );
        return Ok(None);
    }

    let action = &actions[action_index];

    // Validate commitment if provided
    if let Some(expected_cmx) = cmx {
        let action_cmx_bytes = action.cmx().to_bytes();
        if action_cmx_bytes.as_ref() != expected_cmx {
            tracing::debug!(
                "CMX mismatch: expected {}, got {}",
                hex::encode(expected_cmx),
                hex::encode(action_cmx_bytes.as_ref())
            );
            return Ok(None);
        }
    }

    // Decrypt the action using the existing function
    // The action from the bundle is already an orchard::Action<Signature<SpendAuth>>
    decrypt_orchard_action_with_ivk_bytes(action, ivk_bytes)
}
