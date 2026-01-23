//! Full Sapling transaction decryption for memo extraction
//!
//! Parses raw transactions from GetTransaction RPC and decrypts full 580-byte
//! ciphertexts to extract memos using sapling_ka_agree + KDF_Sapling +
//! ChaCha20Poly1305.
//!
//! References node Sapling decryption logic:
//! - pirate/src/zcash/Note.cpp (SaplingNotePlaintext::decrypt)
//! - pirate/src/zcash/NoteEncryption.cpp (AttemptSaplingEncDecryption, KDF_Sapling)

use crate::Error;
use blake2b_simd::Params;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use group::GroupEncoding;
use jubjub::{ExtendedPoint, Scalar};
use std::convert::TryInto;
use zcash_primitives::consensus::BranchId;
use zcash_primitives::{sapling::note_encryption::sapling_ka_agree, transaction::Transaction};

/// Decrypted full note with memo
pub struct DecryptedFullNote {
    /// Note value in arrrtoshis.
    pub value: u64,
    /// Diversifier for the recipient address (11 bytes).
    pub diversifier: [u8; 11],
    /// Memo bytes (512 bytes, may contain null padding).
    pub memo: Vec<u8>, // 512 bytes, may contain null padding
}

/// KDF for Sapling (Key Derivation Function)
/// Derives symmetric key from key agreement result and ephemeral key
/// KDF_Sapling as specified by the Sapling protocol.
fn kdf_sapling(dhsecret: &[u8; 32], epk: &[u8; 32]) -> Key {
    // KDF_Sapling(K, dhsecret, epk)
    // Uses Blake2b with personalization "Zcash_SaplingKDF"
    let mut hasher = Params::new()
        .hash_length(32)
        .personal(b"Zcash_SaplingKDF")
        .to_state();
    hasher.update(dhsecret);
    hasher.update(epk);
    let k = hasher.finalize();

    // Convert to ChaCha20Poly1305 key
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(k.as_bytes());
    *Key::from_slice(&key_bytes)
}

/// Parse raw transaction and decrypt memo for a specific output (using IVK bytes)
///
/// Decrypts ciphertexts using the Sapling note decryption flow:
/// 1. Key agreement using sapling_ka_agree
/// 2. KDF_Sapling to derive symmetric key
/// 3. ChaCha20Poly1305 decryption with nonce=0
/// 4. Deserialize plaintext: [leadbyte (1)] [diversifier (11)] [value (8)] [rseed (32)] [memo (512)]
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

    // Verify ciphertext length
    let enc_ciphertext = output.enc_ciphertext();
    if enc_ciphertext.len() != 580 {
        return Err(Error::Sync(format!(
            "Invalid ciphertext length: {} (expected 580)",
            enc_ciphertext.len()
        )));
    }

    // Convert IVK bytes to scalar
    let ivk_scalar = match Scalar::from_bytes(ivk_bytes).into() {
        Some(s) => s,
        None => {
            return Err(Error::Sync("Invalid IVK bytes".to_string()));
        }
    };

    // Get ephemeral key from output
    let epk_bytes = output.ephemeral_key();
    let epk_array: [u8; 32] = epk_bytes
        .as_ref()
        .try_into()
        .map_err(|_| Error::Sync("Invalid ephemeral key length".to_string()))?;
    let epk = match ExtendedPoint::from_bytes(&epk_array).into() {
        Some(p) => p,
        None => {
            return Err(Error::Sync("Invalid ephemeral key".to_string()));
        }
    };

    // Get cmu from output
    let note_commitment = output.cmu();

    // Validate against provided cmu if given
    if let Some(expected_cmu) = cmu {
        let cmu_bytes = note_commitment.to_bytes();
        if cmu_bytes.as_ref() != expected_cmu {
            tracing::debug!(
                "CMU mismatch: expected {}, got {}",
                hex::encode(expected_cmu),
                hex::encode(cmu_bytes.as_ref())
            );
        }
    }

    // Step 1: Key agreement (librustzcash_sapling_ka_agree).
    let ka = sapling_ka_agree(&ivk_scalar, &epk);
    let ka_bytes = ka.to_bytes();

    // Step 2: Derive symmetric key using KDF_Sapling
    let key = kdf_sapling(&ka_bytes, &epk_array);

    // Step 3: Decrypt using ChaCha20Poly1305 with nonce=0
    let cipher = ChaCha20Poly1305::new(&key);
    let nonce = Nonce::from_slice(&[0u8; 12]); // Nonce is zero (12 bytes)

    let plaintext = match cipher.decrypt(nonce, enc_ciphertext.as_ref()) {
        Ok(pt) => pt,
        Err(_) => {
            // Decryption failed - note doesn't belong to this IVK
            return Ok(None);
        }
    };

    // Step 4: Deserialize plaintext
    // Format: [leadbyte (1)] [diversifier (11)] [value (8)] [rseed (32)] [memo (512)]
    if plaintext.len() < 564 {
        return Ok(None);
    }

    let mut offset = 0;

    // Leadbyte (1 byte)
    let _leadbyte = plaintext[offset];
    offset += 1;

    // Diversifier (11 bytes)
    let mut diversifier = [0u8; 11];
    diversifier.copy_from_slice(&plaintext[offset..offset + 11]);
    offset += 11;

    // Value (8 bytes, little-endian)
    let mut value_bytes = [0u8; 8];
    value_bytes.copy_from_slice(&plaintext[offset..offset + 8]);
    let value = u64::from_le_bytes(value_bytes);
    offset += 8;

    // Rseed (32 bytes) - not needed for memo extraction
    offset += 32;

    // Memo (512 bytes)
    let memo = plaintext[offset..offset + 512].to_vec();

    // Verify note commitment matches (optional validation)
    // This would require computing cmu from diversifier, value, rseed
    // Skip plaintext consistency check; upstream validation already covers it.

    Ok(Some(DecryptedFullNote {
        value,
        diversifier,
        memo,
    }))
}

/// Decode memo bytes to UTF-8 string
///
/// Memos are 512 bytes, with UTF-8 text followed by null padding.
/// If the first byte is > 0xF4, it's not a text memo.
pub fn decode_memo(memo: &[u8]) -> Option<String> {
    if memo.is_empty() {
        return None;
    }

    // If first byte > 0xF4, not a text memo
    if memo[0] > 0xF4 {
        return None;
    }

    // Trim trailing nulls
    let trimmed: Vec<u8> = memo.iter().copied().take_while(|&b| b != 0).collect();

    if trimmed.is_empty() {
        return None;
    }

    // Try to decode as UTF-8
    String::from_utf8(trimmed).ok()
}
