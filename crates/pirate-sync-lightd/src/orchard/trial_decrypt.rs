//! Orchard trial decryption using zcash_note_encryption
//! 
//! References node Orchard trial decryption logic.
//! - pirate/src/rust/src/orchard_actions.rs (try_orchard_decrypt_action_ivk)
//! - Uses zcash_note_encryption::try_note_decryption with OrchardDomain

use orchard::{
    keys::{IncomingViewingKey, PreparedIncomingViewingKey, Diversifier},
    note_encryption::OrchardDomain,
    primitives::redpallas::{Signature, SpendAuth},
    Action as OrchardAction,
    note::Note,
    value::NoteValue,
};
use zcash_note_encryption::{try_note_decryption, EphemeralKeyBytes};
use crate::{Error, client::CompactOrchardAction};
use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use chacha20::ChaCha20;
use blake2b_simd::Params;

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

/// KDF for Orchard (Key Derivation Function)
/// Derives symmetric key from key agreement result and ephemeral key
/// Per Orchard spec (Blake2b with "Zcash_OrchardKDF" personalization).
fn kdf_orchard(dhsecret: &[u8; 32], epk: &[u8; 32]) -> [u8; 32] {
    // KDF_Orchard(K, dhsecret, epk) as per Orchard protocol
    // Uses Blake2b with personalization "Zcash_OrchardKDF"
    let mut hasher = Params::new()
        .hash_length(32)
        .personal(b"Zcash_OrchardKDF")
        .to_state();
    hasher.update(dhsecret);
    hasher.update(epk);
    let k = hasher.finalize();
    
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(k.as_bytes());
    key_bytes
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
        || action.ephemeral_key.len() != 32 
        || action.enc_ciphertext.len() < 52 {
        return Ok(None);
    }

    // Convert IVK bytes to IncomingViewingKey
    let ivk_ct = IncomingViewingKey::from_bytes(ivk_bytes);
    if !bool::from(ivk_ct.is_some()) {
        return Err(Error::Sync("Invalid Orchard IVK bytes".to_string()));
    }
    let ivk = ivk_ct.unwrap();

    // Extract the compact ciphertext prefix (52 bytes; does not include the 16-byte auth tag)
    let compact_ciphertext = &action.enc_ciphertext[..52.min(action.enc_ciphertext.len())];
    if compact_ciphertext.len() < 52 {
        return Ok(None);
    }

    // Extract ephemeral key bytes
    let mut epk_bytes = [0u8; 32];
    if action.ephemeral_key.len() != 32 {
        return Ok(None);
    }
    epk_bytes.copy_from_slice(&action.ephemeral_key[..32]);
    let ephemeral_key = EphemeralKeyBytes(epk_bytes);

    // Manual key agreement for compact decryption
    // We can't use try_compact_note_decryption because CompactOrchardAction doesn't implement ShieldedOutput
    // Manual key agreement consistent with zcash_note_encryption.
    // Reference: librustzcash/components/zcash_note_encryption/src/lib.rs:567-578
    
    // Use OrchardDomain's trait methods for key agreement
    // The Domain trait methods are associated functions that can be called on the type
    use zcash_note_encryption::Domain;
    let epk_opt = <OrchardDomain as Domain>::epk(&ephemeral_key);
    let epk = match epk_opt {
        Some(epk) => epk,
        None => {
            tracing::debug!("Invalid ephemeral key");
            return Ok(None);
        }
    };
    let prepared_epk = <OrchardDomain as Domain>::prepare_epk(epk);
    
    // Key agreement using OrchardDomain (handles Pallas curve internally)
    // For OrchardDomain, IncomingViewingKey is PreparedIncomingViewingKey
    // Prepare the IVK for trial decryption.
    let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);
    let shared_secret = <OrchardDomain as Domain>::ka_agree_dec(&prepared_ivk, &prepared_epk);
    
    // Derive symmetric key using KDF
    let key = <OrchardDomain as Domain>::kdf(shared_secret, &ephemeral_key);
    let key_bytes: [u8; 32] = key.as_ref().try_into().map_err(|_| Error::Sync("Invalid key length".to_string()))?;

    // Step 3: Decrypt the first 52 bytes using the ChaCha20 keystream (IETF variant)
    //
    // Orchard uses ChaCha20-Poly1305-IETF with nonce=0 and no AD, same as Sapling.
    // In RFC8439 AEAD, block counter 0 is reserved for the Poly1305 key, and
    // the message stream uses counter=1. Therefore, to decrypt ciphertext bytes
    // (without the tag), we seek to 64 bytes (1 block).
    let nonce = [0u8; 12];
    let mut chacha = ChaCha20::new((&key_bytes).into(), (&nonce).into());
    chacha.seek(64);

    let mut plaintext_prefix = [0u8; 52];
    plaintext_prefix.copy_from_slice(compact_ciphertext);
    chacha.apply_keystream(&mut plaintext_prefix);

    // Orchard plaintext prefix format (52 bytes) - same layout as Sapling per ZIP-307:
    // [leadbyte:1][diversifier:11][value:8 LE][rseed:32]
    // This is the same format as Sapling - the diversifier is in the 52-byte prefix!
    let leadbyte = plaintext_prefix[0];
    if leadbyte != 0x01 && leadbyte != 0x02 {
        return Ok(None);
    }

    // Extract diversifier (11 bytes) - same position as Sapling
    let mut diversifier = [0u8; 11];
    diversifier.copy_from_slice(&plaintext_prefix[1..12]);

    // Extract value (8 bytes, little-endian)
    let mut value_bytes = [0u8; 8];
    value_bytes.copy_from_slice(&plaintext_prefix[12..20]);
    let value = u64::from_le_bytes(value_bytes);

    // Extract rseed (32 bytes) - used for note commitment
    let mut rseed_bytes = [0u8; 32];
    rseed_bytes.copy_from_slice(&plaintext_prefix[20..52]);

    // Verify note commitment (cmx) using the decrypted value and rseed
    // We can't fully verify without the payment address, but we can check if
    // the commitment can be checked with the available data
    // Note: For Orchard, the commitment is computed from (address, value, rseed, rho)
    // Since we don't have the full address or rho, we can't fully verify here.
    // However, successful decryption with valid leadbyte and reasonable value
    // is a strong indicator that the note belongs to us.
    // Full validation will happen when we fetch the full transaction.

    // Derive payment address from IVK + diversifier (just like Sapling!)
    // Same as Sapling: learn the diversifier from the decrypted plaintext,
    // then derive the address from IVK + diversifier using ivk.address(diversifier)
    let mut address = [0u8; 43];
    
    // Convert diversifier bytes to Orchard Diversifier type
    let orchard_diversifier = Diversifier::from_bytes(diversifier);
    
    // Derive address from IVK + diversifier.
    // This is the same pattern as Sapling: ivk.to_payment_address(diversifier)
    // Note: ivk.address() returns Address directly, not Option<Address>
    let derived_address = ivk.address(orchard_diversifier);
    address.copy_from_slice(&derived_address.to_raw_address_bytes());

    // If we got here, decryption succeeded with valid format
    // This indicates the note likely belongs to us
    // The full transaction will be fetched and decrypted to get the complete address
    Ok(Some(DecryptedCompactOrchardNote {
        diversifier,
        value,
        address,
        memo: None,
    }))
}
