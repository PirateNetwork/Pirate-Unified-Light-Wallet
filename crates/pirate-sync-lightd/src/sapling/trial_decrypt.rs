//! Compact Sapling trial decryption
//!
//! Given an IVK (derived from EXTSK/DFVK) and a compact output (cmu, epk, truncated
//! ciphertext), attempts to decrypt and recover the note plaintext.
//!
//! Mirrors node Sapling trial decryption logic.
//!
//! For compact blocks, we only have the first 52 bytes of ciphertext (not full 580 bytes).
//! This is sufficient to decrypt the first part of the plaintext (leadbyte + diversifier + value)
//! and verify the note commitment matches, but not enough to decrypt the memo.

use crate::client::CompactSaplingOutput;
use group::ff::PrimeField;
use jubjub::Fr;
use sapling::{
    keys::PreparedIncomingViewingKey,
    note::ExtractedNoteCommitment,
    note_encryption::{
        try_sapling_compact_note_decryption, CompactOutputDescription, Zip212Enforcement,
    },
    SaplingIvk,
};
use zcash_note_encryption::EphemeralKeyBytes;

/// Decrypted compact note data (minimal fields we need).
pub struct DecryptedCompactNote {
    /// Lead byte indicating plaintext version.
    pub leadbyte: u8,
    /// Diversifier for the recipient address (11 bytes).
    pub diversifier: [u8; 11],
    /// Raw Sapling payment address bytes (43 bytes).
    pub address: [u8; 43],
    /// Note value in arrrtoshis.
    pub value: u64,
    /// Note rseed bytes (32 bytes).
    pub rseed: [u8; 32],
    /// Memo bytes (always `None` for compact trial decryption).
    pub memo: Option<Vec<u8>>, // Always None for compact decryption
}

/// Attempt to decrypt a compact Sapling output with the given IVK bytes.
///
/// This performs trial decryption using the compact output data (first 52 bytes of ciphertext).
/// This is the standard light-wallet flow:
/// 1. Lightwalletd provides compact blocks with only 52 bytes of ciphertext (not full 580 bytes)
/// 2. Light client performs trial decryption to find notes that belong to the wallet
/// 3. If a note matches, the wallet can optionally fetch the full transaction to decrypt the memo
///
/// Returns the decrypted note with value and diversifier if successful.
///
/// **Memo Handling:**
/// - Memo is NOT available from compact decryption (requires full 580-byte ciphertext)
/// - To get memo, light wallets:
///   1. Fetch full transaction using `GetTransaction` RPC (returns RawTransaction)
///   2. Parse the raw transaction to extract the full 580-byte shielded output ciphertext
///   3. Decrypt the full ciphertext using IVK to get the memo
/// - Per ZIP-307, memo decryption requires downloading full transactions, which reduces privacy
/// - Many light wallets defer memo decryption until the memo is actually needed (lazy loading)
pub fn try_decrypt_compact_output(
    ivk_bytes: &[u8; 32],
    output: &CompactSaplingOutput,
) -> Option<DecryptedCompactNote> {
    // Validate input lengths
    if output.cmu.len() != 32 || output.ephemeral_key.len() != 32 || output.ciphertext.len() < 52 {
        return None;
    }

    // Convert IVK bytes to the prepared Sapling incoming viewing key type.
    let ivk_fr = Option::<Fr>::from(Fr::from_repr(*ivk_bytes))?;
    let sapling_ivk = SaplingIvk(ivk_fr);
    let prepared_ivk = PreparedIncomingViewingKey::new(&sapling_ivk);

    // Convert cmu to ExtractedNoteCommitment for validation
    let mut cmu_bytes = [0u8; 32];
    cmu_bytes.copy_from_slice(&output.cmu[..32]);
    let expected_cmu: Option<ExtractedNoteCommitment> =
        ExtractedNoteCommitment::from_bytes(&cmu_bytes).into();
    let expected_cmu = match expected_cmu {
        Some(v) => v,
        None => {
            tracing::debug!("Invalid cmu bytes");
            return None;
        }
    };

    let mut epk_bytes = [0u8; 32];
    epk_bytes.copy_from_slice(&output.ephemeral_key[..32]);
    let mut ciphertext = [0u8; 52];
    ciphertext.copy_from_slice(&output.ciphertext[..52]);
    let compact_output = CompactOutputDescription {
        ephemeral_key: EphemeralKeyBytes(epk_bytes),
        cmu: expected_cmu,
        enc_ciphertext: ciphertext,
    };

    let (note, payment_address) = try_sapling_compact_note_decryption(
        &prepared_ivk,
        &compact_output,
        Zip212Enforcement::GracePeriod,
    )?;

    let (leadbyte, rseed_bytes) = match note.rseed() {
        sapling::Rseed::BeforeZip212(rcm) => {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&rcm.to_repr());
            (0x01, bytes)
        }
        sapling::Rseed::AfterZip212(rseed) => (0x02, *rseed),
    };

    Some(DecryptedCompactNote {
        leadbyte,
        diversifier: payment_address.diversifier().0,
        address: payment_address.to_bytes(),
        value: note.value().inner(),
        rseed: rseed_bytes,
        memo: None,
    })
}
