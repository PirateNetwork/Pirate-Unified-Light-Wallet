//! Compact Sapling trial decryption
//! 
//! Given an IVK (derived from EXTSK/DFVK) and a compact output (cmu, epk, truncated
//! ciphertext), attempts to decrypt and recover the note plaintext.
//!
//! References node Sapling trial decryption logic.
//! - pirate/src/zcash/Note.cpp (SaplingNotePlaintext::decrypt)
//! - pirate/src/zcash/NoteEncryption.cpp (AttemptSaplingEncDecryption, KDF_Sapling)
//!
//! For compact blocks, we only have the first 52 bytes of ciphertext (not full 580 bytes).
//! This is sufficient to decrypt the first part of the plaintext (leadbyte + diversifier + value)
//! and verify the note commitment matches, but not enough to decrypt the memo.

use zcash_primitives::{
    sapling::{
        note::ExtractedNoteCommitment,
        note_encryption::sapling_ka_agree,
    },
};
use group::GroupEncoding;
use jubjub::{ExtendedPoint, Fr};
use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use chacha20::ChaCha20;
use blake2b_simd::Params;
use crate::client::CompactSaplingOutput;

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

/// KDF for Sapling (Key Derivation Function)
/// Derives symmetric key from key agreement result and ephemeral key
/// KDF_Sapling as specified by the Sapling protocol.
fn kdf_sapling(dhsecret: &[u8; 32], epk: &[u8; 32]) -> [u8; 32] {
    // KDF_Sapling(K, dhsecret, epk)
    // Uses Blake2b with personalization "Zcash_SaplingKDF"
    let mut hasher = Params::new()
        .hash_length(32)
        .personal(b"Zcash_SaplingKDF")
        .to_state();
    hasher.update(dhsecret);
    hasher.update(epk);
    let k = hasher.finalize();
    
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(k.as_bytes());
    key_bytes
}

/// Attempt to decrypt a compact Sapling output with the given IVK bytes.
///
/// This performs trial decryption using the compact output data (first 52 bytes of ciphertext).
/// This is how Zcash/Pirate light wallets work:
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

    // Convert IVK bytes to jubjub::Fr (SaplingIvk wraps jubjub::Fr in this fork).
    let ivk_fr = Option::from(Fr::from_bytes(ivk_bytes))?;

    // Convert ephemeral key to point
    let mut epk_bytes = [0u8; 32];
    epk_bytes.copy_from_slice(&output.ephemeral_key[..32]);
    let epk = match ExtendedPoint::from_bytes(&epk_bytes).into() {
        Some(p) => p,
        None => {
            tracing::debug!("Invalid ephemeral key bytes");
            return None;
        }
    };

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

    // Extract the compact ciphertext prefix (52 bytes; does not include the 16-byte auth tag).
    let compact_ciphertext = &output.ciphertext[..52];

    // Step 1: Key agreement (librustzcash_sapling_ka_agree).
    let ka = sapling_ka_agree(&ivk_fr, &epk);
    let ka_bytes = ka.to_bytes();

    // Step 2: Derive symmetric key using KDF_Sapling (Blake2b-256, personalization "Zcash_SaplingKDF")
    let key_bytes = kdf_sapling(&ka_bytes, &epk_bytes);

    // Step 3: Decrypt the first 52 bytes using the ChaCha20 keystream (IETF variant).
    //
    // Sapling uses ChaCha20-Poly1305-IETF with nonce=0 and no AD.
    // In RFC8439 AEAD, block counter 0 is reserved for the Poly1305 key, and
    // the message stream uses counter=1. Therefore, to decrypt ciphertext bytes
    // (without the tag), we seek to 64 bytes (1 block).
    let nonce = [0u8; 12];
    let mut chacha = ChaCha20::new((&key_bytes).into(), (&nonce).into());
    chacha.seek(64);

    let mut plaintext_prefix = [0u8; 52];
    plaintext_prefix.copy_from_slice(compact_ciphertext);
    chacha.apply_keystream(&mut plaintext_prefix);

    // Plaintext prefix format:
    // [leadbyte:1][diversifier:11][value:8 LE][rseed:32]
    let leadbyte = plaintext_prefix[0];
    if leadbyte != 0x01 && leadbyte != 0x02 {
        return None;
    }

    let mut diversifier = [0u8; 11];
    diversifier.copy_from_slice(&plaintext_prefix[1..12]);

    let mut value_bytes = [0u8; 8];
    value_bytes.copy_from_slice(&plaintext_prefix[12..20]);
    let value = u64::from_le_bytes(value_bytes);

    let mut rseed_bytes = [0u8; 32];
    rseed_bytes.copy_from_slice(&plaintext_prefix[20..52]);

    // Derive payment address (pk_d) from IVK + diversifier.
    // In this fork, Sapling IVK type is `SaplingIvk`, and it provides `to_payment_address`.
    let sapling_ivk = zcash_primitives::sapling::SaplingIvk(ivk_fr);
    let sapling_div = zcash_primitives::sapling::Diversifier(diversifier);
    let payment_address = sapling_ivk.to_payment_address(sapling_div)?;

    // Construct rseed based on plaintext version.
    let rseed = if leadbyte == 0x02 {
        zcash_primitives::sapling::Rseed::AfterZip212(rseed_bytes)
    } else {
        let rcm = Option::from(Fr::from_bytes(&rseed_bytes))?;
        zcash_primitives::sapling::Rseed::BeforeZip212(rcm)
    };

    // Compute cmu and compare.
    let note_value = zcash_primitives::sapling::value::NoteValue::from_raw(value);
    let note = zcash_primitives::sapling::Note::from_parts(payment_address, note_value, rseed);
    let computed_cmu = note.cmu();
    if computed_cmu != expected_cmu {
        return None;
    }

    return Some(DecryptedCompactNote {
        leadbyte,
        diversifier,
        address: payment_address.to_bytes(),
        value,
        rseed: rseed_bytes,
        memo: None,
    });

    // unreachable
}
