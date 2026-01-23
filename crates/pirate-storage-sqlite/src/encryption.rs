//! Encryption key derivation

use crate::security::derive_key_bytes;
use crate::{Error, Result};
use sha2::{Digest, Sha256};

/// Encryption key for database
pub struct EncryptionKey([u8; 32]);

impl EncryptionKey {
    /// Derive from passphrase using Argon2id + salt
    pub fn from_passphrase(passphrase: &str, salt: &[u8]) -> Result<Self> {
        let key = derive_key_bytes(passphrase, salt)?;
        Ok(Self(key))
    }

    /// Derive using legacy SHA-256 (migration only)
    pub fn from_legacy_password(password: &str) -> Self {
        let hash = Sha256::digest(password.as_bytes());
        Self(hash.into())
    }

    /// Create from raw key bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create from raw key bytes slice
    pub fn from_bytes_slice(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(Error::Encryption("Invalid key length".to_string()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    /// Get key bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
