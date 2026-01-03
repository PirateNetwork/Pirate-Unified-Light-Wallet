//! Orchard trial decryption using zcash_note_encryption
//! 
//! References node Orchard trial decryption logic.
//! - pirate/src/rust/src/orchard_actions.rs (try_orchard_decrypt_action_ivk)
//! - Uses zcash_note_encryption::try_note_decryption with OrchardDomain

use orchard::{
    keys::{IncomingViewingKey, PreparedIncomingViewingKey},
    note::{ExtractedNoteCommitment, Nullifier},
    note_encryption::{CompactAction, OrchardDomain},
    primitives::redpallas::{Signature, SpendAuth},
    Action as OrchardAction,
};
use zcash_note_encryption::{try_compact_note_decryption, try_note_decryption, EphemeralKeyBytes};
use crate::{Error, client::CompactOrchardAction};

/// Decrypted Orchard note data
pub struct DecryptedOrchardNote {
    /// Note value in arrrtoshis.
    pub value: u64,
    /// Raw Orchard payment address bytes (43 bytes).
    pub address: [u8; 43],
    /// Memo bytes (512 bytes).
    pub memo: [u8; 512],
    /// Note rho (nullifier key material) bytes (32 bytes).
    pub rho: [u8; 32],
    /// Note rseed bytes (32 bytes).
    pub rseed: [u8; 32],
}

/// Attempt to decrypt an Orchard action with the given IVK bytes
///
/// Uses zcash_note_encryption::try_note_decryption with the same domain setup as the node.
/// The node uses `orchard::Action<Signature<SpendAuth>>` directly.
///
/// # Arguments
/// * `action` - Orchard action from transaction (orchard::Action<Signature<SpendAuth>>)
/// * `ivk_bytes` - Incoming viewing key bytes (64 bytes for Orchard)
///
/// # Returns
/// Decrypted note if successful, or None if decryption fails
pub fn try_decrypt_orchard_action(
    action: &OrchardAction<Signature<SpendAuth>>,
    ivk_bytes: &[u8; 64],
) -> Result<Option<DecryptedOrchardNote>, Error> {
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
            
            Ok(Some(DecryptedOrchardNote {
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

/// Decrypted compact Orchard note data (from compact blocks)
pub struct DecryptedCompactOrchardNote {
    /// Diversifier for the recipient address (11 bytes)
    pub diversifier: [u8; 11],
    /// Note value in arrrtoshis
    pub value: u64,
    /// Raw Orchard payment address bytes (43 bytes) - derived from IVK + diversifier (just like Sapling)
    pub address: [u8; 43],
    /// Memo bytes (always None for compact decryption - requires full transaction)
    pub memo: Option<Vec<u8>>,
}

/// Attempt to decrypt a compact Orchard action with the given IVK bytes
///
/// This function performs **trial decryption** using the 52-byte prefix from compact blocks
/// to detect if a note belongs to the wallet. This is the same pattern as Sapling:
///
/// 1. **Trial decryption**: Use the 52-byte prefix to detect if the note belongs to the wallet
/// 2. **If match found**: Call `GetTransaction` to fetch the full transaction
/// 3. **Full decryption**: Use `try_decrypt_orchard_action` with the full Action object to decrypt
///    everything including the memo
///
/// # Arguments
/// * `action` - Compact Orchard action from compact block (contains 52-byte enc_ciphertext prefix)
/// * `ivk_bytes` - Incoming viewing key bytes (64 bytes for Orchard)
///
/// # Returns
/// Decrypted note with value and address if the note belongs to the wallet, or None otherwise.
///
/// Note: Memo is not available from compact decryption - requires fetching full transaction
pub fn try_decrypt_compact_orchard_action(
    action: &CompactOrchardAction,
    ivk_bytes: &[u8; 64],
) -> Result<Option<DecryptedCompactOrchardNote>, Error> {
    // Validate input lengths
    if action.cmx.len() != 32
        || action.nullifier.len() != 32
        || action.ephemeral_key.len() != 32
        || action.enc_ciphertext.len() < 52
    {
        return Ok(None);
    }

    // Convert IVK bytes to IncomingViewingKey
    let ivk_ct = IncomingViewingKey::from_bytes(ivk_bytes);
    if !bool::from(ivk_ct.is_some()) {
        return Err(Error::Sync("Invalid Orchard IVK bytes".to_string()));
    }
    let ivk = ivk_ct.unwrap();
    let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);

    let mut cmx_bytes = [0u8; 32];
    cmx_bytes.copy_from_slice(&action.cmx[..32]);
    let cmx_ct = ExtractedNoteCommitment::from_bytes(&cmx_bytes);
    if !bool::from(cmx_ct.is_some()) {
        return Ok(None);
    }
    let cmx = cmx_ct.unwrap();

    let mut nf_bytes = [0u8; 32];
    nf_bytes.copy_from_slice(&action.nullifier[..32]);
    let nf_ct = Nullifier::from_bytes(&nf_bytes);
    if !bool::from(nf_ct.is_some()) {
        return Ok(None);
    }
    let nullifier = nf_ct.unwrap();

    let mut epk_bytes = [0u8; 32];
    epk_bytes.copy_from_slice(&action.ephemeral_key[..32]);
    let ephemeral_key = EphemeralKeyBytes(epk_bytes);

    let mut enc_ciphertext = [0u8; 52];
    enc_ciphertext.copy_from_slice(&action.enc_ciphertext[..52]);

    let compact_action = CompactAction::from_parts(nullifier, cmx, ephemeral_key, enc_ciphertext);
    let domain = OrchardDomain::for_nullifier(nullifier);

    match try_compact_note_decryption(&domain, &prepared_ivk, &compact_action) {
        Some((note, payment_address)) => {
            let value = note.value().inner();
            let address = payment_address.to_raw_address_bytes();
            let diversifier = *payment_address.diversifier().as_array();
            Ok(Some(DecryptedCompactOrchardNote {
                diversifier,
                value,
                address,
                memo: None,
            }))
        }
        None => Ok(None),
    }
}
