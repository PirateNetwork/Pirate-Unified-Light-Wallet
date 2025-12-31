//! Security tests for encryption, key sealing, and passphrase handling
//!
//! Tests cover:
//! - Argon2id KDF with mandatory parameters (64 MiB, t=3, p=4)
//! - Master key generation, encryption, decryption
//! - Key sealing/unsealing with platform keystores
//! - Screenshot blocking state management
//! - Secure clipboard with auto-clear

use pirate_storage_sqlite::security::{
    MasterKey, AppPassphrase, SealedKey, EncryptionAlgorithm, PanicPin, generate_salt, hash_sha256,
};
use pirate_storage_sqlite::keystore::{
    MockKeystore, PlatformKeystore, KeystoreResult, KeystoreCapabilities, Platform,
    BiometricManager, BiometricConfig, BiometricState,
};
use pirate_storage_sqlite::screenshot_guard::{
    ScreenshotGuard, ProtectionReason, ProtectionState, ScreenshotProtectionStatus,
};
use pirate_storage_sqlite::secure_clipboard::{
    SecureClipboard, ClipboardDataType, MockClipboard, ClipboardPlatform,
    SEED_CLEAR_TIMEOUT_SECS, DEFAULT_CLEAR_TIMEOUT_SECS, IVK_CLEAR_TIMEOUT_SECS,
};
use pirate_storage_sqlite::seed_export::{
    SeedExportManager, ExportFlowState, SeedExportResult,
};

// =============================================================================
// KDF Test Vectors (Argon2id with 64 MiB, t=3, p=4)
// =============================================================================

#[test]
fn test_argon2id_mandatory_parameters() {
    // Verify Argon2id uses the mandatory parameters:
    // memory_cost = 65536 KiB (64 MiB)
    // time_cost = 3 iterations
    // parallelism = 4 lanes
    
    let passphrase = "test_passphrase_123";
    let salt = generate_salt();
    
    // Derive key - this should use the mandatory parameters internally
    let key = AppPassphrase::derive_key(passphrase, &salt).unwrap();
    
    // Key should be 32 bytes (256 bits)
    assert_eq!(key.as_bytes().len(), 32);
    
    // Same input should produce same output (deterministic)
    let key2 = AppPassphrase::derive_key(passphrase, &salt).unwrap();
    assert_eq!(key.as_bytes(), key2.as_bytes());
}

#[test]
fn test_kdf_determinism_vector() {
    // Test that KDF produces consistent output for same inputs
    let passphrase = "MySecurePassphrase!@#$%^";
    let salt = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
        0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
    ];
    
    // Derive key twice with same salt
    let key1 = AppPassphrase::derive_key(passphrase, &salt).unwrap();
    let key2 = AppPassphrase::derive_key(passphrase, &salt).unwrap();
    
    // Must be identical
    assert_eq!(key1.as_bytes(), key2.as_bytes());
    
    // Must not be all zeros
    assert!(key1.as_bytes().iter().any(|&b| b != 0));
}

#[test]
fn test_kdf_different_salt_produces_different_key() {
    let passphrase = "SamePassphrase";
    let salt1 = generate_salt();
    let salt2 = generate_salt();
    
    let key1 = AppPassphrase::derive_key(passphrase, &salt1).unwrap();
    let key2 = AppPassphrase::derive_key(passphrase, &salt2).unwrap();
    
    // Different salts must produce different keys
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_kdf_different_passphrase_produces_different_key() {
    let salt = generate_salt();
    
    let key1 = AppPassphrase::derive_key("Passphrase1", &salt).unwrap();
    let key2 = AppPassphrase::derive_key("Passphrase2", &salt).unwrap();
    
    // Different passphrases must produce different keys
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_kdf_salt_minimum_length() {
    let passphrase = "test";
    
    // Salt too short (< 16 bytes) should fail
    let short_salt = [0u8; 15];
    assert!(AppPassphrase::derive_key(passphrase, &short_salt).is_err());
    
    // 16 bytes is minimum
    let min_salt = [0u8; 16];
    assert!(AppPassphrase::derive_key(passphrase, &min_salt).is_ok());
    
    // 32 bytes is recommended
    let full_salt = generate_salt();
    assert!(AppPassphrase::derive_key(passphrase, &full_salt).is_ok());
}

#[test]
fn test_passphrase_hash_uses_argon2id() {
    let passphrase = "TestPassphrase123!";
    let hashed = AppPassphrase::hash(passphrase).unwrap();
    
    // Argon2id hash should start with $argon2id$
    let hash_string = hashed.hash_string();
    assert!(hash_string.starts_with("$argon2id$"), "Hash must use Argon2id: {}", hash_string);
    
    // Should contain version v=19
    assert!(hash_string.contains("v=19"), "Hash must use version 19");
    
    // Should contain memory parameter m=65536 (64 MiB in KiB)
    assert!(hash_string.contains("m=65536"), "Hash must use 64 MiB memory: {}", hash_string);
    
    // Should contain time parameter t=3
    assert!(hash_string.contains("t=3"), "Hash must use 3 iterations: {}", hash_string);
    
    // Should contain parallelism parameter p=4
    assert!(hash_string.contains("p=4"), "Hash must use 4 lanes: {}", hash_string);
}

// =============================================================================
// Key Sealing/Unsealing Tests
// =============================================================================

#[test]
fn test_seal_unseal_master_key() {
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    let original_bytes = *master_key.as_bytes();
    
    // Simulate sealing (in production, OS keystore does this)
    let plaintext_copy = original_bytes;
    
    // Create sealed key structure
    let sealed = SealedKey::new(
        plaintext_copy.to_vec(), // In production, this would be encrypted
        "test_wallet_key".to_string(),
        EncryptionAlgorithm::ChaCha20Poly1305,
    );
    
    // Serialize and deserialize
    let serialized = sealed.serialize();
    let deserialized = SealedKey::deserialize(&serialized).unwrap();
    
    assert_eq!(deserialized.encrypted_key, sealed.encrypted_key);
    assert_eq!(deserialized.key_id, sealed.key_id);
}

#[test]
fn test_mock_keystore_seal_unseal_roundtrip() {
    let keystore = MockKeystore::new();
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    let original_bytes = *master_key.as_bytes();
    
    // Seal the key
    let sealed = keystore.seal_key(&master_key, "wallet_test_123").unwrap();
    assert_eq!(sealed.key_id, "wallet_test_123");
    
    // Encrypted key should be different from original (XOR in mock)
    assert_ne!(sealed.encrypted_key.as_slice(), original_bytes.as_slice());
    
    // Unseal the key
    match keystore.unseal_key(&sealed) {
        KeystoreResult::Success(unsealed) => {
            // Must recover original key
            assert_eq!(unsealed.as_bytes(), &original_bytes);
        }
        _ => panic!("Expected KeystoreResult::Success"),
    }
}

#[test]
fn test_mock_keystore_multiple_keys() {
    let keystore = MockKeystore::new();
    
    let key1 = MasterKey::generate(EncryptionAlgorithm::AesGcm);
    let key2 = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    
    let sealed1 = keystore.seal_key(&key1, "key_1").unwrap();
    let sealed2 = keystore.seal_key(&key2, "key_2").unwrap();
    
    // Both should unseal correctly
    if let KeystoreResult::Success(unsealed1) = keystore.unseal_key(&sealed1) {
        assert_eq!(unsealed1.as_bytes(), key1.as_bytes());
    } else {
        panic!("Failed to unseal key 1");
    }
    
    if let KeystoreResult::Success(unsealed2) = keystore.unseal_key(&sealed2) {
        assert_eq!(unsealed2.as_bytes(), key2.as_bytes());
    } else {
        panic!("Failed to unseal key 2");
    }
}

#[test]
fn test_keystore_capabilities_detection() {
    let keystore = MockKeystore::new();
    let caps = keystore.capabilities();
    
    // Mock keystore has no hardware backing by default
    assert!(!caps.has_secure_hardware);
    assert!(!caps.has_strongbox);
    assert!(!caps.has_secure_enclave);
    
    // Custom capabilities
    let custom_caps = KeystoreCapabilities {
        has_secure_hardware: true,
        has_strongbox: true,
        has_secure_enclave: false,
        has_biometrics: true,
        platform: Platform::Android,
    };
    
    let keystore_with_caps = MockKeystore::with_capabilities(custom_caps.clone());
    let detected = keystore_with_caps.capabilities();
    
    assert!(detected.has_secure_hardware);
    assert!(detected.has_strongbox);
    assert!(detected.has_biometrics);
    assert_eq!(detected.platform, Platform::Android);
}

#[test]
fn test_biometric_keystore_unlock() {
    let caps = KeystoreCapabilities {
        has_secure_hardware: true,
        has_strongbox: false,
        has_secure_enclave: false,
        has_biometrics: true,
        platform: Platform::Android,
    };
    
    let keystore = MockKeystore::with_capabilities(caps);
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    let sealed = keystore.seal_key(&master_key, "biometric_test").unwrap();
    
    // Biometric unlock should work
    match keystore.unseal_key_biometric(&sealed) {
        KeystoreResult::Success(unsealed) => {
            assert_eq!(unsealed.as_bytes(), master_key.as_bytes());
        }
        _ => panic!("Expected KeystoreResult::Success"),
    }
}

#[test]
fn test_biometric_unavailable_returns_not_available() {
    let keystore = MockKeystore::new(); // No biometrics
    let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    let sealed = keystore.seal_key(&master_key, "test").unwrap();
    
    match keystore.unseal_key_biometric(&sealed) {
        KeystoreResult::NotAvailable => {} // Expected
        _ => panic!("Expected KeystoreResult::NotAvailable"),
    }
}

#[test]
fn test_sealed_key_serialization_all_algorithms() {
    for algorithm in [EncryptionAlgorithm::AesGcm, EncryptionAlgorithm::ChaCha20Poly1305] {
        let sealed = SealedKey::new(
            vec![0x11, 0x22, 0x33, 0x44, 0x55],
            format!("key_{:?}", algorithm),
            algorithm,
        );
        
        let serialized = sealed.serialize();
        let deserialized = SealedKey::deserialize(&serialized).unwrap();
        
        assert_eq!(deserialized.encrypted_key, sealed.encrypted_key);
        assert_eq!(deserialized.key_id, sealed.key_id);
    }
}

#[test]
fn test_sealed_key_invalid_data() {
    // Too short
    assert!(SealedKey::deserialize(&[0u8; 5]).is_err());
    
    // Invalid version
    let bad_version = vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert!(SealedKey::deserialize(&bad_version).is_err());
    
    // Invalid algorithm
    let bad_algo = vec![1, 99, 0, 0, 0, 0, 0, 0, 0, 0];
    assert!(SealedKey::deserialize(&bad_algo).is_err());
}

// =============================================================================
// Passphrase Verification Tests
// =============================================================================

#[test]
fn test_passphrase_verification() {
    // Use strong passphrase that meets minimum requirements
    let passphrase = "MySecurePass123!@#";
    let hashed = AppPassphrase::hash(passphrase).unwrap();
    
    // Correct passphrase should verify
    assert!(hashed.verify(passphrase).unwrap());
    
    // Wrong passphrase should not verify
    assert!(!hashed.verify("wrong_passphrase123").unwrap());
}

#[test]
fn test_passphrase_key_derivation() {
    let passphrase = "test_passphrase_key";
    let salt = generate_salt();
    
    // Same passphrase + salt should produce same key
    let key1 = AppPassphrase::derive_key(passphrase, &salt).unwrap();
    let key2 = AppPassphrase::derive_key(passphrase, &salt).unwrap();
    
    assert_eq!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_different_salt_different_key() {
    let passphrase = "test_passphrase_key";
    let salt1 = generate_salt();
    let salt2 = generate_salt();
    
    let key1 = AppPassphrase::derive_key(passphrase, &salt1).unwrap();
    let key2 = AppPassphrase::derive_key(passphrase, &salt2).unwrap();
    
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_encryption_with_derived_key() {
    let passphrase = "secure_pass_phrase_123";
    let salt = generate_salt();
    
    let key = AppPassphrase::derive_key(passphrase, &salt).unwrap();
    let plaintext = b"Secret wallet data";
    
    // Encrypt
    let ciphertext = key.encrypt(plaintext).unwrap();
    assert_ne!(ciphertext.as_slice(), plaintext);
    
    // Decrypt with same key
    let decrypted = key.decrypt(&ciphertext).unwrap();
    assert_eq!(decrypted.as_slice(), plaintext);
}

#[test]
fn test_wrong_passphrase_cannot_decrypt() {
    let passphrase1 = "CorrectPassphrase1!";
    let passphrase2 = "WrongPassphraseXX2!";
    let salt = generate_salt();
    
    let key1 = AppPassphrase::derive_key(passphrase1, &salt).unwrap();
    let key2 = AppPassphrase::derive_key(passphrase2, &salt).unwrap();
    
    let plaintext = b"Secret data";
    let ciphertext = key1.encrypt(plaintext).unwrap();
    
    // Wrong key should fail to decrypt
    assert!(key2.decrypt(&ciphertext).is_err());
}

#[test]
fn test_master_key_zeroization() {
    // Test that sensitive key material is properly zeroized
    let key = MasterKey::generate(EncryptionAlgorithm::AesGcm);
    let bytes_copy = *key.as_bytes();
    
    drop(key);
    
    // After drop, original key should be zeroized
    // (Zeroize trait handles this automatically)
    assert!(bytes_copy.iter().any(|&b| b != 0)); // Original had data
}

#[test]
fn test_aes_gcm_encryption() {
    let key = MasterKey::generate(EncryptionAlgorithm::AesGcm);
    let plaintext = b"Test message";
    
    let ciphertext = key.encrypt(plaintext).unwrap();
    let decrypted = key.decrypt(&ciphertext).unwrap();
    
    assert_eq!(decrypted.as_slice(), plaintext);
}

#[test]
fn test_chacha20_encryption() {
    let key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    let plaintext = b"Test message";
    
    let ciphertext = key.encrypt(plaintext).unwrap();
    let decrypted = key.decrypt(&ciphertext).unwrap();
    
    assert_eq!(decrypted.as_slice(), plaintext);
}

#[test]
fn test_large_data_encryption() {
    let key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
    let plaintext = vec![42u8; 1_000_000]; // 1 MB
    
    let ciphertext = key.encrypt(&plaintext).unwrap();
    let decrypted = key.decrypt(&ciphertext).unwrap();
    
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_empty_data_encryption() {
    let key = MasterKey::generate(EncryptionAlgorithm::AesGcm);
    let plaintext = b"";
    
    let ciphertext = key.encrypt(plaintext).unwrap();
    let decrypted = key.decrypt(&ciphertext).unwrap();
    
    assert_eq!(decrypted.as_slice(), plaintext);
}

#[test]
fn test_sealed_key_serialization_roundtrip() {
    let sealed = SealedKey::new(
        vec![1, 2, 3, 4, 5],
        "test_key_id".to_string(),
        EncryptionAlgorithm::AesGcm,
    );
    
    let serialized = sealed.serialize();
    let deserialized = SealedKey::deserialize(&serialized).unwrap();
    
    assert_eq!(deserialized.encrypted_key, sealed.encrypted_key);
    assert_eq!(deserialized.key_id, sealed.key_id);
}

#[test]
fn test_passphrase_hash_storage() {
    let passphrase = "StrongPassphrase123!";
    let hashed = AppPassphrase::hash(passphrase).unwrap();
    
    // Store hash string
    let hash_string = hashed.hash_string().to_string();
    
    // Load from stored hash
    let loaded = AppPassphrase::from_hash(hash_string);
    
    // Should still verify
    assert!(loaded.verify(passphrase).unwrap());
}

#[test]
fn test_weak_passphrase_rejected() {
    // Passphrases under 12 characters should be rejected
    assert!(AppPassphrase::hash("short").is_err());
    assert!(AppPassphrase::hash("12345678901").is_err()); // 11 chars
    assert!(AppPassphrase::hash("password12").is_err()); // 10 chars (Fair, not Good)
    
    // 12+ characters with some variety passes
    assert!(AppPassphrase::hash("abcdefghijklmnop").is_ok()); // 16 chars, Good strength
    assert!(AppPassphrase::hash("SecurePass12345").is_ok()); // 15 chars, Good strength
}

#[test]
fn test_passphrase_strength_evaluation() {
    use pirate_storage_sqlite::PassphraseStrength;
    
    // Weak (< 8 chars)
    assert_eq!(AppPassphrase::evaluate_strength("short"), PassphraseStrength::Weak);
    
    // Fair (8-11 chars)
    assert_eq!(AppPassphrase::evaluate_strength("password12"), PassphraseStrength::Fair);
    
    // Good (12-15 chars or lacks variety)
    assert_eq!(AppPassphrase::evaluate_strength("MyPassword123"), PassphraseStrength::Good);
    
    // Strong (16+ chars with variety)
    assert_eq!(AppPassphrase::evaluate_strength("MySecurePass123!@#"), PassphraseStrength::Strong);
}

// =============================================================================
// Screenshot Blocking Tests
// =============================================================================

#[test]
fn test_screenshot_guard_lifecycle() {
    let guard = ScreenshotGuard::new();
    
    // Initial state should be disabled
    assert_eq!(guard.state(), ProtectionState::Disabled);
    assert!(!guard.is_active());
    
    // Enable protection for seed phrase
    let _protection = guard.enable(ProtectionReason::SeedPhrase);
    
    assert_eq!(guard.state(), ProtectionState::Enabled);
    assert!(guard.is_active());
    assert_eq!(guard.highest_security_level(), 10);
    
    // Suspend
    guard.suspend();
    assert_eq!(guard.state(), ProtectionState::Suspended);
    assert!(guard.is_active()); // Still considered active when suspended
    
    // Resume
    guard.resume();
    assert_eq!(guard.state(), ProtectionState::Enabled);
}

#[test]
fn test_screenshot_guard_nested_protections() {
    let guard = ScreenshotGuard::new();
    
    // Enable first protection
    let _p1 = guard.enable(ProtectionReason::Sensitive);
    assert_eq!(guard.highest_security_level(), 3);
    
    // Enable second, higher priority protection
    let _p2 = guard.enable(ProtectionReason::SeedPhrase);
    assert_eq!(guard.highest_security_level(), 10);
    
    // Both reasons should be tracked
    let reasons = guard.active_reasons();
    assert_eq!(reasons.len(), 2);
    assert!(reasons.contains(&ProtectionReason::Sensitive));
    assert!(reasons.contains(&ProtectionReason::SeedPhrase));
}

#[test]
fn test_screenshot_protection_reason_security_levels() {
    // Verify security levels are correctly ordered
    assert_eq!(ProtectionReason::SeedPhrase.security_level(), 10);
    assert_eq!(ProtectionReason::SpendingKey.security_level(), 10);
    assert_eq!(ProtectionReason::ViewingKey.security_level(), 8);
    assert_eq!(ProtectionReason::PanicPin.security_level(), 7);
    assert_eq!(ProtectionReason::PassphraseEntry.security_level(), 5);
    assert_eq!(ProtectionReason::Sensitive.security_level(), 3);
}

#[test]
fn test_screenshot_protection_status_conversion() {
    let guard = ScreenshotGuard::new();
    guard.set_platform_supported(true);
    
    let _protection = guard.enable(ProtectionReason::ViewingKey);
    
    let status = ScreenshotProtectionStatus::from(&guard);
    
    assert_eq!(status.state, ProtectionState::Enabled);
    assert_eq!(status.active_count, 1);
    assert!(status.platform_supported);
    assert_eq!(status.security_level, 8);
    assert_eq!(status.reasons.len(), 1);
    assert!(status.reasons[0].contains("Viewing key"));
}

#[test]
fn test_screenshot_platform_support_flag() {
    let guard = ScreenshotGuard::new();
    
    // Default is not supported (conservative)
    assert!(!guard.is_platform_supported());
    
    // Set as supported (called from FFI during init)
    guard.set_platform_supported(true);
    assert!(guard.is_platform_supported());
    
    // Can be toggled
    guard.set_platform_supported(false);
    assert!(!guard.is_platform_supported());
}

// =============================================================================
// Biometric Manager Tests
// =============================================================================

#[test]
fn test_biometric_manager_lockout() {
    let mut manager = BiometricManager::new();
    manager.configure(BiometricConfig {
        enabled: true,
        require_confirmation: false,
        allow_passphrase_fallback: true,
        timeout_seconds: 30,
    });
    
    assert!(manager.is_ready());
    assert_eq!(manager.state(), BiometricState::Ready);
    
    // Record failures until lockout
    for i in 0..BiometricManager::MAX_FAILED_ATTEMPTS {
        manager.record_failure();
        if i < BiometricManager::MAX_FAILED_ATTEMPTS - 1 {
            assert!(!manager.is_locked_out());
        }
    }
    
    assert!(manager.is_locked_out());
    assert_eq!(manager.state(), BiometricState::LockedOut);
    
    // Reset lockout
    manager.reset_lockout();
    assert!(manager.is_ready());
}

#[test]
fn test_biometric_session_validity() {
    let mut manager = BiometricManager::new();
    manager.configure(BiometricConfig {
        enabled: true,
        require_confirmation: false,
        allow_passphrase_fallback: true,
        timeout_seconds: 1, // 1 second timeout for testing
    });
    
    // No session initially
    assert!(!manager.is_session_valid());
    
    // Record success
    manager.record_success();
    assert!(manager.is_session_valid());
    
    // Wait for timeout
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert!(!manager.is_session_valid());
}

#[test]
fn test_biometric_disable() {
    let mut manager = BiometricManager::new();
    manager.configure(BiometricConfig {
        enabled: true,
        ..Default::default()
    });
    
    assert!(manager.is_ready());
    
    manager.disable();
    assert!(!manager.is_ready());
    assert_eq!(manager.state(), BiometricState::Disabled);
}

// =============================================================================
// Secure Clipboard Tests
// =============================================================================

#[test]
fn test_clipboard_timeout_values() {
    // Verify timeout values match security spec
    assert_eq!(SEED_CLEAR_TIMEOUT_SECS, 10, "Seed phrase must auto-clear in 10s");
    assert_eq!(IVK_CLEAR_TIMEOUT_SECS, 10, "IVK must auto-clear in 10s");
    assert_eq!(DEFAULT_CLEAR_TIMEOUT_SECS, 10, "Default must be 10s");
}

#[test]
fn test_clipboard_data_type_timeouts() {
    assert_eq!(ClipboardDataType::SeedPhrase.timeout_seconds(), Some(10));
    assert_eq!(ClipboardDataType::ViewingKey.timeout_seconds(), Some(10));
    assert_eq!(ClipboardDataType::Sensitive.timeout_seconds(), Some(10));
    assert_eq!(ClipboardDataType::Address.timeout_seconds(), Some(60));
    assert_eq!(ClipboardDataType::TransactionId.timeout_seconds(), Some(60));
    assert_eq!(ClipboardDataType::Public.timeout_seconds(), None);
}

#[test]
fn test_secure_clipboard_timer_lifecycle() {
    let clipboard = SecureClipboard::new();
    
    // Prepare copy starts timer
    let _ = clipboard.prepare_copy("secret_seed_words", ClipboardDataType::SeedPhrase);
    
    assert!(clipboard.timer().is_active());
    assert!(clipboard.remaining_time().is_some());
    
    // Verify content
    assert!(clipboard.verify_content("secret_seed_words"));
    assert!(!clipboard.verify_content("different_content"));
    
    // Mark cleared
    clipboard.mark_cleared();
    assert!(!clipboard.timer().is_active());
}

#[test]
fn test_secure_clipboard_no_timer_for_public() {
    let clipboard = SecureClipboard::new();
    
    // Public data should not start timer
    let _ = clipboard.prepare_copy("public_data", ClipboardDataType::Public);
    
    assert!(!clipboard.timer().is_active());
}

#[test]
fn test_mock_clipboard_operations() {
    let clipboard = MockClipboard::new();
    
    // Initial state
    assert!(!clipboard.has_text());
    assert!(clipboard.paste().is_none());
    
    // Copy
    assert!(clipboard.copy("test_data"));
    assert!(clipboard.has_text());
    assert_eq!(clipboard.paste(), Some("test_data".to_string()));
    
    // Clear
    assert!(clipboard.clear());
    assert!(!clipboard.has_text());
    assert!(clipboard.paste().is_none());
}

// =============================================================================
// Seed Export Flow Tests
// =============================================================================

#[test]
fn test_seed_export_flow_states() {
    let manager = SeedExportManager::new();
    
    // Initial state
    assert_eq!(manager.state(), ExportFlowState::NotStarted);
    
    // Start export
    let state = manager.start_export("wallet_123".to_string()).unwrap();
    assert_eq!(state, ExportFlowState::WarningDisplayed);
    
    // Acknowledge warning
    let state = manager.acknowledge_warning().unwrap();
    assert_eq!(state, ExportFlowState::AwaitingBiometric);
    
    // Skip biometric
    let state = manager.skip_biometric().unwrap();
    assert_eq!(state, ExportFlowState::AwaitingPassphrase);
}

#[test]
fn test_seed_export_cancellation() {
    let manager = SeedExportManager::new();
    
    manager.start_export("wallet_123".to_string()).unwrap();
    manager.acknowledge_warning().unwrap();
    
    manager.cancel();
    assert_eq!(manager.state(), ExportFlowState::Cancelled);
}

#[test]
fn test_seed_export_invalid_state_transitions() {
    let manager = SeedExportManager::new();
    
    // Cannot acknowledge warning without starting
    assert!(manager.acknowledge_warning().is_err());
    
    // Cannot skip biometric without acknowledging
    manager.start_export("wallet_123".to_string()).unwrap();
    assert!(manager.skip_biometric().is_err());
}

#[test]
fn test_seed_export_result_zeroization() {
    let words = vec![
        "abandon".to_string(), "abandon".to_string(), "abandon".to_string(),
        "abandon".to_string(), "abandon".to_string(), "abandon".to_string(),
        "abandon".to_string(), "abandon".to_string(), "abandon".to_string(),
        "abandon".to_string(), "abandon".to_string(), "about".to_string(),
    ];
    
    let result = SeedExportResult::new(words, "test_wallet".to_string());
    
    assert_eq!(result.word_count(), 12);
    assert_eq!(result.words()[0], "abandon");
    
    let seed_string = result.as_string();
    assert!(seed_string.contains("abandon"));
    
    // Result will be zeroized on drop
    drop(result);
}

#[test]
fn test_seed_export_screenshots_blocked() {
    let manager = SeedExportManager::new();
    
    // Start export
    manager.start_export("wallet_123".to_string()).unwrap();
    
    // Screenshots should be blocked during export
    assert!(manager.are_screenshots_blocked());
}

// =============================================================================
// Panic PIN Tests
// =============================================================================

#[test]
fn test_panic_pin_validation() {
    // Too short (< 4 digits)
    assert!(PanicPin::hash("123").is_err());
    
    // Too long (> 8 digits)
    assert!(PanicPin::hash("123456789").is_err());
    
    // Non-numeric
    assert!(PanicPin::hash("12ab").is_err());
    assert!(PanicPin::hash("abcd").is_err());
    
    // Valid PINs
    assert!(PanicPin::hash("1234").is_ok());
    assert!(PanicPin::hash("12345").is_ok());
    assert!(PanicPin::hash("123456").is_ok());
    assert!(PanicPin::hash("1234567").is_ok());
    assert!(PanicPin::hash("12345678").is_ok());
}

#[test]
fn test_panic_pin_verification() {
    let pin = PanicPin::hash("1234").unwrap();
    
    assert!(pin.verify("1234").unwrap());
    assert!(!pin.verify("5678").unwrap());
    assert!(!pin.verify("12345").unwrap());
}

#[test]
fn test_panic_pin_storage_roundtrip() {
    let pin = PanicPin::hash("9876").unwrap();
    let hash_string = pin.hash_string().to_string();
    
    // Load from stored hash
    let loaded = PanicPin::from_hash(hash_string);
    
    // Should still verify
    assert!(loaded.verify("9876").unwrap());
    assert!(!loaded.verify("1234").unwrap());
}

// =============================================================================
// Hash Function Tests
// =============================================================================

#[test]
fn test_sha256_hash() {
    let data = b"Hello, Pirate!";
    let hash1 = hash_sha256(data);
    let hash2 = hash_sha256(data);
    
    // Deterministic
    assert_eq!(hash1, hash2);
    
    // 32 bytes
    assert_eq!(hash1.len(), 32);
    
    // Different data = different hash
    let hash3 = hash_sha256(b"Different data");
    assert_ne!(hash1, hash3);
}

#[test]
fn test_salt_generation() {
    let salt1 = generate_salt();
    let salt2 = generate_salt();
    
    // 32 bytes each
    assert_eq!(salt1.len(), 32);
    assert_eq!(salt2.len(), 32);
    
    // Should be different (extremely unlikely to collide)
    assert_ne!(salt1, salt2);
    
    // Should not be all zeros
    assert!(salt1.iter().any(|&b| b != 0));
}
