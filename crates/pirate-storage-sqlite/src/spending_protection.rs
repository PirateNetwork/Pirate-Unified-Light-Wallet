//! Optional second-layer protection for spend-capable wallet material.
//!
//! Database and viewing data remain available to synchronization. Spending
//! keys and seed material can additionally be wrapped by a wallet-scoped key
//! that exists only for the lifetime of an unlocked signing session.

use crate::{Error, MasterKey, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::OnceLock;

const ENVELOPE_MAGIC: &[u8; 5] = b"PWSK1";

static SIGNING_SESSION_KEYS: OnceLock<RwLock<HashMap<String, MasterKey>>> = OnceLock::new();

fn sessions() -> &'static RwLock<HashMap<String, MasterKey>> {
    SIGNING_SESSION_KEYS.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Install a wallet-scoped encryption key for the current process session.
pub fn unlock_signing_session(wallet_id: String, key: MasterKey) {
    sessions().write().insert(wallet_id, key);
}

/// Remove one wallet's in-memory signing key.
pub fn lock_signing_session(wallet_id: &str) {
    sessions().write().remove(wallet_id);
}

/// Remove every in-memory signing key.
pub fn lock_all_signing_sessions() {
    sessions().write().clear();
}

/// Return whether a wallet currently has an in-memory signing key.
pub fn is_signing_session_unlocked(wallet_id: &str) -> bool {
    sessions().read().contains_key(wallet_id)
}

/// Wrap plaintext with a specific wallet-scoped key.
pub fn protect_with_key(wallet_id: &str, key: &MasterKey, plaintext: &[u8]) -> Result<Vec<u8>> {
    let wallet_bytes = wallet_id.as_bytes();
    let wallet_len = u16::try_from(wallet_bytes.len())
        .map_err(|_| Error::Validation("wallet id is too long".to_string()))?;
    let ciphertext = key.encrypt(plaintext)?;
    let mut envelope =
        Vec::with_capacity(ENVELOPE_MAGIC.len() + 2 + wallet_bytes.len() + ciphertext.len());
    envelope.extend_from_slice(ENVELOPE_MAGIC);
    envelope.extend_from_slice(&wallet_len.to_le_bytes());
    envelope.extend_from_slice(wallet_bytes);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

/// Wrap plaintext using the wallet's active signing session.
pub fn protect_for_active_session(wallet_id: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
    let sessions = sessions().read();
    let key = sessions.get(wallet_id).ok_or_else(|| {
        Error::Encryption(format!(
            "ERR_SIGNING_SESSION_LOCKED: wallet {wallet_id} must be unlocked before changing spending keys"
        ))
    })?;
    protect_with_key(wallet_id, key, plaintext)
}

/// Reveal a protected value, returning `None` when its wallet is locked.
///
/// Legacy values without the envelope marker are returned unchanged.
pub fn reveal_for_active_session(value: &[u8]) -> Result<Option<Vec<u8>>> {
    if !value.starts_with(ENVELOPE_MAGIC) {
        return Ok(Some(value.to_vec()));
    }
    if value.len() < ENVELOPE_MAGIC.len() + 2 {
        return Err(Error::Encryption(
            "truncated spending-key envelope".to_string(),
        ));
    }
    let length_offset = ENVELOPE_MAGIC.len();
    let wallet_len = u16::from_le_bytes([value[length_offset], value[length_offset + 1]]) as usize;
    let wallet_start = length_offset + 2;
    let ciphertext_start = wallet_start.saturating_add(wallet_len);
    if ciphertext_start >= value.len() {
        return Err(Error::Encryption(
            "invalid spending-key envelope".to_string(),
        ));
    }
    let wallet_id = std::str::from_utf8(&value[wallet_start..ciphertext_start])
        .map_err(|_| Error::Encryption("invalid wallet id in spending-key envelope".to_string()))?;
    let sessions = sessions().read();
    let Some(key) = sessions.get(wallet_id) else {
        return Ok(None);
    };
    key.decrypt(&value[ciphertext_start..]).map(Some)
}

/// Return whether a stored plaintext value uses the wallet-scoped envelope.
pub fn is_protected(value: &[u8]) -> bool {
    value.starts_with(ENVELOPE_MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EncryptionAlgorithm;

    #[test]
    fn protected_material_requires_the_matching_session() {
        lock_all_signing_sessions();
        let key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let envelope = protect_with_key("wallet-a", &key, b"spending-key").unwrap();
        assert_eq!(reveal_for_active_session(&envelope).unwrap(), None);

        unlock_signing_session("wallet-a".to_string(), key);
        assert_eq!(
            reveal_for_active_session(&envelope).unwrap(),
            Some(b"spending-key".to_vec())
        );
        lock_signing_session("wallet-a");
        assert_eq!(reveal_for_active_session(&envelope).unwrap(), None);
    }
}
