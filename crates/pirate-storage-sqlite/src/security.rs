//! Security and encryption primitives
//!
//! Implements AES-GCM and ChaCha20-Poly1305 encryption for database pages,
//! Argon2id for passphrase derivation, and key zeroization.

use crate::{Error, Result};
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, ParamsBuilder, Version,
};
use chacha20poly1305::ChaCha20Poly1305;
use rand::RngCore;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Encryption algorithm
#[derive(Debug, Clone, Copy)]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM
    AesGcm,
    /// ChaCha20-Poly1305
    ChaCha20Poly1305,
}

/// Master key for database encryption
#[derive(Clone)]
pub struct MasterKey {
    key: Zeroizing<[u8; 32]>,
    algorithm: EncryptionAlgorithm,
}

impl MasterKey {
    /// Generate new random master key
    pub fn generate(algorithm: EncryptionAlgorithm) -> Self {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        
        Self {
            key: Zeroizing::new(key),
            algorithm,
        }
    }

    /// Create from bytes
    pub fn from_bytes(bytes: &[u8], algorithm: EncryptionAlgorithm) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(Error::Encryption("Invalid key length".to_string()));
        }
        
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes);
        
        Ok(Self {
            key: Zeroizing::new(key),
            algorithm,
        })
    }

    /// Get key bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    /// Encrypt data
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            EncryptionAlgorithm::AesGcm => self.encrypt_aes_gcm(plaintext),
            EncryptionAlgorithm::ChaCha20Poly1305 => self.encrypt_chacha20(plaintext),
        }
    }

    /// Decrypt data
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            EncryptionAlgorithm::AesGcm => self.decrypt_aes_gcm(ciphertext),
            EncryptionAlgorithm::ChaCha20Poly1305 => self.decrypt_chacha20(ciphertext),
        }
    }

    fn encrypt_aes_gcm(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new(self.key.as_ref().into());
        
        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| Error::Encryption(e.to_string()))?;
        
        // Format: [version(1)][algorithm(1)][nonce(12)][ciphertext(variable)]
        let mut result = Vec::with_capacity(1 + 1 + 12 + ciphertext.len());
        result.push(1); // Version 1
        result.push(0); // Algorithm: 0 = AES-GCM
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        
        Ok(result)
    }

    fn decrypt_aes_gcm(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Format: [version(1)][algorithm(1)][nonce(12)][ciphertext(variable)]
        if data.len() < 14 {
            return Err(Error::Encryption("Invalid ciphertext length".to_string()));
        }
        
        let version = data[0];
        let algorithm = data[1];
        
        // Support version 1 (current) and version 0 (legacy, no metadata)
        if version == 0 {
            // Legacy format: [nonce(12)][ciphertext]
            if data.len() < 12 {
                return Err(Error::Encryption("Invalid legacy ciphertext length".to_string()));
            }
            let cipher = Aes256Gcm::new(self.key.as_ref().into());
            let nonce = Nonce::from_slice(&data[..12]);
            let ciphertext = &data[12..];
            return cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| Error::Encryption(e.to_string()));
        }
        
        if version != 1 {
            return Err(Error::Encryption(format!("Unsupported encryption version: {}", version)));
        }
        
        if algorithm != 0 {
            return Err(Error::Encryption(format!("Algorithm mismatch: expected AES-GCM (0), got {}", algorithm)));
        }
        
        let cipher = Aes256Gcm::new(self.key.as_ref().into());
        
        // Extract nonce and ciphertext
        let nonce = Nonce::from_slice(&data[2..14]);
        let ciphertext = &data[14..];
        
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| Error::Encryption(e.to_string()))
    }

    fn encrypt_chacha20(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new(self.key.as_ref().into());
        
        // Generate random nonce (12 bytes for ChaCha20)
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);
        
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| Error::Encryption(e.to_string()))?;
        
        // Format: [version(1)][algorithm(1)][nonce(12)][ciphertext(variable)]
        let mut result = Vec::with_capacity(1 + 1 + 12 + ciphertext.len());
        result.push(1); // Version 1
        result.push(1); // Algorithm: 1 = ChaCha20-Poly1305
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        
        Ok(result)
    }

    fn decrypt_chacha20(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Format: [version(1)][algorithm(1)][nonce(12)][ciphertext(variable)]
        if data.len() < 14 {
            return Err(Error::Encryption("Invalid ciphertext length".to_string()));
        }
        
        let version = data[0];
        let algorithm = data[1];
        
        // Support version 1 (current) and version 0 (legacy, no metadata)
        if version == 0 {
            // Legacy format: [nonce(12)][ciphertext]
            if data.len() < 12 {
                return Err(Error::Encryption("Invalid legacy ciphertext length".to_string()));
            }
            let cipher = ChaCha20Poly1305::new(self.key.as_ref().into());
            let nonce = chacha20poly1305::Nonce::from_slice(&data[..12]);
            let ciphertext = &data[12..];
            return cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| Error::Encryption(e.to_string()));
        }
        
        if version != 1 {
            return Err(Error::Encryption(format!("Unsupported encryption version: {}", version)));
        }
        
        if algorithm != 1 {
            return Err(Error::Encryption(format!("Algorithm mismatch: expected ChaCha20-Poly1305 (1), got {}", algorithm)));
        }
        
        let cipher = ChaCha20Poly1305::new(self.key.as_ref().into());
        
        // Extract nonce and ciphertext
        let nonce = chacha20poly1305::Nonce::from_slice(&data[2..14]);
        let ciphertext = &data[14..];
        
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| Error::Encryption(e.to_string()))
    }
}

impl Drop for MasterKey {
    fn drop(&mut self) {
        // Zeroizing wrapper handles secure cleanup
    }
}

/// Passphrase strength requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassphraseStrength {
    /// Weak: < 8 characters
    Weak,
    /// Fair: 8-11 characters
    Fair,
    /// Good: 12-15 characters with variety
    Good,
    /// Strong: 16+ characters with variety
    Strong,
}

impl PassphraseStrength {
    /// Check if passphrase meets minimum requirements
    pub fn is_acceptable(&self) -> bool {
        matches!(self, Self::Good | Self::Strong)
    }
}

/// App passphrase with Argon2id derivation
pub struct AppPassphrase {
    hash: String,
}

impl AppPassphrase {
    /// Strong Argon2id parameters (MANDATORY)
    /// Memory: 64 MiB (65536 KiB), Iterations: 3, Parallelism: 4
    const ARGON2_PARAMS: (u32, u32, u32) = (65536, 3, 4); // m_cost (KiB), t_cost, p_cost
    
    /// Minimum passphrase length
    pub const MIN_PASSPHRASE_LENGTH: usize = 12;
    
    /// Recommended passphrase length
    pub const RECOMMENDED_PASSPHRASE_LENGTH: usize = 16;

    /// Evaluate passphrase strength
    pub fn evaluate_strength(passphrase: &str) -> PassphraseStrength {
        let len = passphrase.len();
        let has_lower = passphrase.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = passphrase.chars().any(|c| c.is_ascii_uppercase());
        let has_digit = passphrase.chars().any(|c| c.is_ascii_digit());
        let has_special = passphrase.chars().any(|c| !c.is_alphanumeric());
        
        let variety_score = [has_lower, has_upper, has_digit, has_special]
            .iter()
            .filter(|&&b| b)
            .count();
        
        if len < 8 {
            PassphraseStrength::Weak
        } else if len < 12 {
            PassphraseStrength::Fair
        } else if len < 16 || variety_score < 3 {
            PassphraseStrength::Good
        } else {
            PassphraseStrength::Strong
        }
    }
    
    /// Validate passphrase meets minimum requirements
    pub fn validate(passphrase: &str) -> Result<()> {
        if passphrase.len() < Self::MIN_PASSPHRASE_LENGTH {
            return Err(Error::Security(format!(
                "Passphrase must be at least {} characters",
                Self::MIN_PASSPHRASE_LENGTH
            )));
        }
        
        let strength = Self::evaluate_strength(passphrase);
        if !strength.is_acceptable() {
            return Err(Error::Security(
                "Passphrase too weak. Use at least 12 characters with letters, numbers, and symbols.".to_string()
            ));
        }
        
        Ok(())
    }

    /// Hash passphrase with Argon2id (validates strength first)
    pub fn hash(passphrase: &str) -> Result<Self> {
        // Validate passphrase strength
        Self::validate(passphrase)?;
        
        let salt = SaltString::generate(&mut OsRng);
        
        let params = ParamsBuilder::new()
            .m_cost(Self::ARGON2_PARAMS.0) // 64 MiB
            .t_cost(Self::ARGON2_PARAMS.1) // 3 iterations
            .p_cost(Self::ARGON2_PARAMS.2) // 4 parallelism
            .build()
            .map_err(|e| Error::Encryption(e.to_string()))?;
        
        let argon2 = Argon2::new(
            argon2::Algorithm::Argon2id,
            Version::V0x13,
            params,
        );
        
        let hash = argon2
            .hash_password(passphrase.as_bytes(), &salt)
            .map_err(|e| Error::Encryption(e.to_string()))?
            .to_string();
        
        Ok(Self { hash })
    }

    /// Verify passphrase
    pub fn verify(&self, passphrase: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::new(&self.hash)
            .map_err(|e| Error::Encryption(e.to_string()))?;
        
        let argon2 = Argon2::default();
        
        Ok(argon2
            .verify_password(passphrase.as_bytes(), &parsed_hash)
            .is_ok())
    }

    /// Derive encryption key from passphrase
    pub fn derive_key(passphrase: &str, salt: &[u8]) -> Result<MasterKey> {
        let key_bytes = derive_key_bytes(passphrase, salt)?;
        MasterKey::from_bytes(&key_bytes, EncryptionAlgorithm::ChaCha20Poly1305)
    }

    /// Get hash string for storage
    pub fn hash_string(&self) -> &str {
        &self.hash
    }

    /// Load from stored hash
    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }
}

/// Derive raw key bytes from passphrase using Argon2id.
pub fn derive_key_bytes(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    if salt.len() < 16 {
        return Err(Error::Encryption("Salt too short".to_string()));
    }
    
    let params = ParamsBuilder::new()
        .m_cost(AppPassphrase::ARGON2_PARAMS.0)
        .t_cost(AppPassphrase::ARGON2_PARAMS.1)
        .p_cost(AppPassphrase::ARGON2_PARAMS.2)
        .output_len(32)
        .build()
        .map_err(|e| Error::Encryption(e.to_string()))?;
    
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        Version::V0x13,
        params,
    );
    
    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
        .map_err(|e| Error::Encryption(e.to_string()))?;
    
    let mut out = [0u8; 32];
    out.copy_from_slice(&key[..]);
    Ok(out)
}

/// Sealed master key (encrypted with OS keystore)
pub struct SealedKey {
    /// Encrypted master key
    pub encrypted_key: Vec<u8>,
    /// Key identifier
    pub key_id: String,
    /// Encryption algorithm
    pub algorithm: EncryptionAlgorithm,
}

impl SealedKey {
    /// Create new sealed key
    pub fn new(
        encrypted_key: Vec<u8>,
        key_id: String,
        algorithm: EncryptionAlgorithm,
    ) -> Self {
        Self {
            encrypted_key,
            key_id,
            algorithm,
        }
    }

    /// Serialize for storage
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Version byte
        data.push(1);
        
        // Algorithm
        data.push(match self.algorithm {
            EncryptionAlgorithm::AesGcm => 0,
            EncryptionAlgorithm::ChaCha20Poly1305 => 1,
        });
        
        // Key ID length + key ID
        let key_id_bytes = self.key_id.as_bytes();
        data.extend_from_slice(&(key_id_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(key_id_bytes);
        
        // Encrypted key length + encrypted key
        data.extend_from_slice(&(self.encrypted_key.len() as u32).to_le_bytes());
        data.extend_from_slice(&self.encrypted_key);
        
        data
    }

    /// Deserialize from storage
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() < 10 {
            return Err(Error::Encryption("Invalid sealed key data".to_string()));
        }
        
        let mut pos = 0;
        
        // Version
        let version = data[pos];
        pos += 1;
        if version != 1 {
            return Err(Error::Encryption("Unknown sealed key version".to_string()));
        }
        
        // Algorithm
        let algorithm = match data[pos] {
            0 => EncryptionAlgorithm::AesGcm,
            1 => EncryptionAlgorithm::ChaCha20Poly1305,
            _ => return Err(Error::Encryption("Unknown algorithm".to_string())),
        };
        pos += 1;
        
        // Key ID
        let key_id_len = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        let key_id = String::from_utf8(data[pos..pos + key_id_len].to_vec())
            .map_err(|_| Error::Encryption("Invalid key ID".to_string()))?;
        pos += key_id_len;
        
        // Encrypted key
        let key_len = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        let encrypted_key = data[pos..pos + key_len].to_vec();
        
        Ok(Self {
            encrypted_key,
            key_id,
            algorithm,
        })
    }
}

/// Generate secure random salt
pub fn generate_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Hash data with SHA-256
pub fn hash_sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Panic PIN handler (opens decoy view on entry)
pub struct PanicPin {
    hash: String,
}

impl PanicPin {
    /// Hash PIN with Argon2id (lighter params than main passphrase)
    /// Memory: 16 MiB, Iterations: 2, Parallelism: 2
    const ARGON2_PARAMS: (u32, u32, u32) = (16384, 2, 2);

    /// Hash PIN
    pub fn hash(pin: &str) -> Result<Self> {
        // Validate PIN format
        if pin.len() < 4 || pin.len() > 8 {
            return Err(Error::Encryption("PIN must be 4-8 digits".to_string()));
        }
        
        if !pin.chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::Encryption("PIN must contain only digits".to_string()));
        }
        
        let salt = SaltString::generate(&mut OsRng);
        
        let params = ParamsBuilder::new()
            .m_cost(Self::ARGON2_PARAMS.0)
            .t_cost(Self::ARGON2_PARAMS.1)
            .p_cost(Self::ARGON2_PARAMS.2)
            .build()
            .map_err(|e| Error::Encryption(e.to_string()))?;
        
        let argon2 = Argon2::new(
            argon2::Algorithm::Argon2id,
            Version::V0x13,
            params,
        );
        
        let hash = argon2
            .hash_password(pin.as_bytes(), &salt)
            .map_err(|e| Error::Encryption(e.to_string()))?
            .to_string();
        
        Ok(Self { hash })
    }

    /// Verify PIN
    pub fn verify(&self, pin: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::new(&self.hash)
            .map_err(|e| Error::Encryption(e.to_string()))?;
        
        let argon2 = Argon2::default();
        
        Ok(argon2
            .verify_password(pin.as_bytes(), &parsed_hash)
            .is_ok())
    }

    /// Get hash string for storage
    pub fn hash_string(&self) -> &str {
        &self.hash
    }

    /// Load from stored hash
    pub fn from_hash(hash: String) -> Self {
        Self { hash }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_master_key_generation() {
        let key = MasterKey::generate(EncryptionAlgorithm::AesGcm);
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn test_encryption_decryption_aes_gcm() {
        let key = MasterKey::generate(EncryptionAlgorithm::AesGcm);
        let plaintext = b"Hello, Pirate!";
        
        let ciphertext = key.encrypt(plaintext).unwrap();
        assert_ne!(ciphertext.as_slice(), plaintext);
        
        let decrypted = key.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_encryption_decryption_chacha20() {
        let key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        let plaintext = b"Secret message";
        
        let ciphertext = key.encrypt(plaintext).unwrap();
        assert_ne!(ciphertext.as_slice(), plaintext);
        
        let decrypted = key.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_passphrase_hashing() {
        // Use a strong passphrase that meets minimum requirements
        let passphrase = AppPassphrase::hash("MySecurePass123!").unwrap();
        assert!(passphrase.verify("MySecurePass123!").unwrap());
        assert!(!passphrase.verify("wrong_passphrase").unwrap());
    }
    
    #[test]
    fn test_passphrase_strength_evaluation() {
        // Weak
        assert_eq!(AppPassphrase::evaluate_strength("short"), PassphraseStrength::Weak);
        assert_eq!(AppPassphrase::evaluate_strength("1234567"), PassphraseStrength::Weak);
        
        // Fair
        assert_eq!(AppPassphrase::evaluate_strength("password12"), PassphraseStrength::Fair);
        
        // Good
        assert_eq!(AppPassphrase::evaluate_strength("MyPassword123"), PassphraseStrength::Good);
        
        // Strong
        assert_eq!(AppPassphrase::evaluate_strength("MySecurePass123!@#"), PassphraseStrength::Strong);
    }
    
    #[test]
    fn test_passphrase_validation() {
        // Too short (< 12 chars)
        assert!(AppPassphrase::validate("short").is_err());
        assert!(AppPassphrase::validate("12345678901").is_err()); // 11 chars
        assert!(AppPassphrase::validate("password12").is_err()); // 10 chars
        
        // 12+ chars passes (variety not strictly required at minimum length)
        assert!(AppPassphrase::validate("abcdefghijkl").is_ok()); // 12 chars, Good
        assert!(AppPassphrase::validate("MyPassword123!").is_ok());
        assert!(AppPassphrase::validate("SecurePass12345").is_ok());
    }
    
    #[test]
    fn test_weak_passphrase_rejected() {
        // Short passphrases should be rejected
        assert!(AppPassphrase::hash("weak").is_err());
        assert!(AppPassphrase::hash("12345678").is_err());
    }

    #[test]
    fn test_key_derivation() {
        let salt = generate_salt();
        let key1 = AppPassphrase::derive_key("passphrase", &salt).unwrap();
        let key2 = AppPassphrase::derive_key("passphrase", &salt).unwrap();
        
        // Same passphrase + salt = same key
        assert_eq!(key1.as_bytes(), key2.as_bytes());
    }

    #[test]
    fn test_sealed_key_serialization() {
        let sealed = SealedKey::new(
            vec![1, 2, 3, 4],
            "test_key".to_string(),
            EncryptionAlgorithm::ChaCha20Poly1305,
        );
        
        let serialized = sealed.serialize();
        let deserialized = SealedKey::deserialize(&serialized).unwrap();
        
        assert_eq!(deserialized.encrypted_key, sealed.encrypted_key);
        assert_eq!(deserialized.key_id, sealed.key_id);
    }

    #[test]
    fn test_wrong_key_decryption() {
        let key1 = MasterKey::generate(EncryptionAlgorithm::AesGcm);
        let key2 = MasterKey::generate(EncryptionAlgorithm::AesGcm);
        
        let plaintext = b"Secret";
        let ciphertext = key1.encrypt(plaintext).unwrap();
        
        // Wrong key should fail to decrypt
        assert!(key2.decrypt(&ciphertext).is_err());
    }

    #[test]
    fn test_panic_pin_hashing() {
        let pin = PanicPin::hash("1234").unwrap();
        assert!(pin.verify("1234").unwrap());
        assert!(!pin.verify("5678").unwrap());
    }

    #[test]
    fn test_panic_pin_validation() {
        // Too short
        assert!(PanicPin::hash("123").is_err());
        
        // Too long
        assert!(PanicPin::hash("123456789").is_err());
        
        // Non-numeric
        assert!(PanicPin::hash("12ab").is_err());
        
        // Valid
        assert!(PanicPin::hash("1234").is_ok());
        assert!(PanicPin::hash("12345678").is_ok());
    }
    
    #[test]
    fn test_argon2id_parameters() {
        // Verify the mandatory parameters are set correctly
        assert_eq!(AppPassphrase::ARGON2_PARAMS.0, 65536); // 64 MiB in KiB
        assert_eq!(AppPassphrase::ARGON2_PARAMS.1, 3);     // 3 iterations
        assert_eq!(AppPassphrase::ARGON2_PARAMS.2, 4);     // 4 parallel lanes
    }
}

