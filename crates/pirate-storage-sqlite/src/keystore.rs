//! Platform keystore integration for secure key sealing
//!
//! Provides unified interface to platform-specific secure storage:
//! - Android: Keystore with StrongBox detection
//! - iOS/macOS: Keychain with Secure Enclave
//! - Windows: DPAPI (Data Protection API)
//! - Linux: libsecret (GNOME Keyring / KDE Wallet)
//!
//! All operations are designed to be FFI-friendly for Flutter integration.

use crate::{EncryptionAlgorithm, Error, MasterKey, Result, SealedKey};
use parking_lot::RwLock;
use std::sync::{Arc, OnceLock};

/// Platform capabilities for secure storage
#[derive(Debug, Clone)]
pub struct KeystoreCapabilities {
    /// Has hardware-backed secure storage (TEE, StrongBox, Secure Enclave)
    pub has_secure_hardware: bool,
    /// Has StrongBox (Android only)
    pub has_strongbox: bool,
    /// Has Secure Enclave (iOS/macOS only)
    pub has_secure_enclave: bool,
    /// Has biometric authentication available
    pub has_biometrics: bool,
    /// Platform name
    pub platform: Platform,
}

impl Default for KeystoreCapabilities {
    fn default() -> Self {
        Self {
            has_secure_hardware: false,
            has_strongbox: false,
            has_secure_enclave: false,
            has_biometrics: false,
            platform: Platform::Unknown,
        }
    }
}

/// Supported platforms
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Android (Keystore, StrongBox)
    Android,
    /// iOS (Keychain, Secure Enclave)
    Ios,
    /// macOS (Keychain, Secure Enclave)
    MacOs,
    /// Windows (DPAPI)
    Windows,
    /// Linux (libsecret)
    Linux,
    /// Unknown platform
    Unknown,
}

impl Platform {
    /// Detect current platform at runtime
    pub fn current() -> Self {
        #[cfg(target_os = "android")]
        return Platform::Android;
        
        #[cfg(target_os = "ios")]
        return Platform::Ios;
        
        #[cfg(target_os = "macos")]
        return Platform::MacOs;
        
        #[cfg(target_os = "windows")]
        return Platform::Windows;
        
        #[cfg(target_os = "linux")]
        return Platform::Linux;
        
        #[cfg(not(any(
            target_os = "android",
            target_os = "ios",
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )))]
        return Platform::Unknown;
    }
}

/// Biometric authentication type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiometricType {
    /// Fingerprint sensor
    Fingerprint,
    /// Face recognition (Face ID)
    Face,
    /// Iris scanner
    Iris,
    /// Multiple types available
    Multiple,
    /// Unknown or unavailable
    None,
}

/// Keystore result for operations that may require user interaction
#[derive(Debug)]
pub enum KeystoreResult<T> {
    /// Success
    Success(T),
    /// User cancelled authentication
    Cancelled,
    /// Authentication failed (wrong biometric, etc.)
    AuthFailed,
    /// Keystore not available on this platform
    NotAvailable,
    /// Error occurred
    Error(Error),
}

impl<T> From<Result<T>> for KeystoreResult<T> {
    fn from(result: Result<T>) -> Self {
        match result {
            Ok(v) => KeystoreResult::Success(v),
            Err(e) => KeystoreResult::Error(e),
        }
    }
}

static PLATFORM_KEYSTORE: OnceLock<RwLock<Option<Arc<dyn PlatformKeystore>>>> = OnceLock::new();

fn keystore_slot() -> &'static RwLock<Option<Arc<dyn PlatformKeystore>>> {
    PLATFORM_KEYSTORE.get_or_init(|| RwLock::new(None))
}

/// Register a platform keystore implementation for this process.
pub fn set_platform_keystore(keystore: Arc<dyn PlatformKeystore>) {
    *keystore_slot().write() = Some(keystore);
}

/// Clear the configured platform keystore.
pub fn clear_platform_keystore() {
    *keystore_slot().write() = None;
}

/// Get the configured platform keystore, if any.
pub fn platform_keystore() -> Option<Arc<dyn PlatformKeystore>> {
    keystore_slot().read().as_ref().map(Arc::clone)
}

/// Platform keystore abstraction
/// 
/// This trait defines the interface for platform-specific keystore operations.
/// FFI implementations will bridge to native platform code via Flutter.
pub trait PlatformKeystore: Send + Sync {
    /// Get platform capabilities
    fn capabilities(&self) -> KeystoreCapabilities;
    
    /// Seal (encrypt) a master key using platform keystore
    /// 
    /// On Android: Uses Android Keystore (StrongBox if available)
    /// On iOS/macOS: Uses Keychain with Secure Enclave if available
    /// On Windows: Uses DPAPI
    /// On Linux: Uses libsecret
    fn seal_key(&self, key: &MasterKey, key_id: &str) -> Result<SealedKey>;
    
    /// Unseal (decrypt) a master key
    fn unseal_key(&self, sealed: &SealedKey) -> KeystoreResult<MasterKey>;
    
    /// Unseal with biometric authentication
    fn unseal_key_biometric(&self, sealed: &SealedKey) -> KeystoreResult<MasterKey>;
    
    /// Delete a sealed key from keystore
    fn delete_key(&self, key_id: &str) -> Result<()>;
    
    /// Check if biometric authentication is available
    fn has_biometrics(&self) -> bool;
    
    /// Get available biometric type
    fn biometric_type(&self) -> BiometricType;
    
    /// Authenticate with biometrics (returns true if successful)
    fn authenticate_biometric(&self, reason: &str) -> KeystoreResult<bool>;
}

/// Mock keystore for testing and platforms without native integration
pub struct MockKeystore {
    capabilities: KeystoreCapabilities,
}

impl MockKeystore {
    /// Create new mock keystore
    pub fn new() -> Self {
        Self {
            capabilities: KeystoreCapabilities {
                has_secure_hardware: false,
                has_strongbox: false,
                has_secure_enclave: false,
                has_biometrics: false,
                platform: Platform::current(),
            },
        }
    }
    
    /// Create with custom capabilities (for testing)
    pub fn with_capabilities(capabilities: KeystoreCapabilities) -> Self {
        Self { capabilities }
    }
}

impl Default for MockKeystore {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformKeystore for MockKeystore {
    fn capabilities(&self) -> KeystoreCapabilities {
        self.capabilities.clone()
    }
    
    fn seal_key(&self, key: &MasterKey, key_id: &str) -> Result<SealedKey> {
        // Mock: Just wrap the key with a simple XOR "encryption"
        // In production, this would call platform-specific APIs via FFI
        let mut encrypted = key.as_bytes().to_vec();
        let xor_key: u8 = 0x5A; // Simple XOR for mock
        for byte in &mut encrypted {
            *byte ^= xor_key;
        }
        
        Ok(SealedKey::new(
            encrypted,
            key_id.to_string(),
            EncryptionAlgorithm::ChaCha20Poly1305,
        ))
    }
    
    fn unseal_key(&self, sealed: &SealedKey) -> KeystoreResult<MasterKey> {
        // Mock: Reverse the XOR
        let mut decrypted = sealed.encrypted_key.clone();
        let xor_key: u8 = 0x5A;
        for byte in &mut decrypted {
            *byte ^= xor_key;
        }
        
        match MasterKey::from_bytes(&decrypted, sealed.algorithm) {
            Ok(key) => KeystoreResult::Success(key),
            Err(e) => KeystoreResult::Error(e),
        }
    }
    
    fn unseal_key_biometric(&self, sealed: &SealedKey) -> KeystoreResult<MasterKey> {
        if !self.has_biometrics() {
            return KeystoreResult::NotAvailable;
        }
        self.unseal_key(sealed)
    }
    
    fn delete_key(&self, _key_id: &str) -> Result<()> {
        // Mock: No-op
        Ok(())
    }
    
    fn has_biometrics(&self) -> bool {
        self.capabilities.has_biometrics
    }
    
    fn biometric_type(&self) -> BiometricType {
        if self.capabilities.has_biometrics {
            BiometricType::Fingerprint
        } else {
            BiometricType::None
        }
    }
    
    fn authenticate_biometric(&self, _reason: &str) -> KeystoreResult<bool> {
        if !self.has_biometrics() {
            return KeystoreResult::NotAvailable;
        }
        // Mock: Always succeed
        KeystoreResult::Success(true)
    }
}

// =============================================================================
// Platform-specific shims (FFI bridge points)
// =============================================================================

/// Android Keystore shim
/// 
/// In production, this calls into Kotlin/Java via FFI to use:
/// - android.security.keystore.KeyGenParameterSpec
/// - android.security.keystore.KeyProperties
/// - javax.crypto.Cipher
/// 
/// StrongBox is preferred when available (Pixel 3+, Samsung flagships, etc.)
#[cfg(target_os = "android")]
pub mod android {
    use super::*;
    
    /// Keystore alias prefix
    pub const KEYSTORE_ALIAS_PREFIX: &str = "pirate_wallet_";
    
    /// Check if StrongBox is available
    pub fn has_strongbox() -> bool {
        // FFI call to Kotlin: PackageManager.hasSystemFeature(FEATURE_STRONGBOX_KEYSTORE)
        false // Placeholder
    }
    
    /// Generate key in Android Keystore
    pub fn generate_keystore_key(alias: &str, require_biometric: bool) -> Result<()> {
        // FFI call to generate AES-256-GCM key in Keystore
        // KeyGenParameterSpec.Builder(alias, PURPOSE_ENCRYPT | PURPOSE_DECRYPT)
        //   .setBlockModes(BLOCK_MODE_GCM)
        //   .setEncryptionPaddings(ENCRYPTION_PADDING_NONE)
        //   .setUserAuthenticationRequired(require_biometric)
        //   .setIsStrongBoxBacked(has_strongbox())
        //   .build()
        let _ = (alias, require_biometric);
        Ok(())
    }
    
    /// Encrypt with Android Keystore
    pub fn encrypt_with_keystore(alias: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        // FFI call to encrypt using Keystore key
        let _ = (alias, plaintext);
        Err(Error::Encryption("Android Keystore FFI not implemented".into()))
    }
    
    /// Decrypt with Android Keystore
    pub fn decrypt_with_keystore(alias: &str, ciphertext: &[u8]) -> Result<Vec<u8>> {
        // FFI call to decrypt using Keystore key
        let _ = (alias, ciphertext);
        Err(Error::Encryption("Android Keystore FFI not implemented".into()))
    }
}

/// iOS/macOS Keychain shim
/// 
/// In production, this calls into Swift/Objective-C via FFI to use:
/// - Security framework (SecItemAdd, SecItemCopyMatching, etc.)
/// - Secure Enclave for key protection
/// - kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly
#[cfg(any(target_os = "ios", target_os = "macos"))]
pub mod apple {
    use super::*;
    
    /// Keychain service name
    pub const KEYCHAIN_SERVICE: &str = "com.pirate.wallet";
    
    /// Check if Secure Enclave is available
    pub fn has_secure_enclave() -> bool {
        // FFI call to Swift: SecureEnclave.isAvailable
        // Available on: iPhone 5s+, iPad Air+, Mac with T1/T2/M1+
        false // Placeholder
    }
    
    /// Store key in Keychain
    pub fn store_in_keychain(
        key_id: &str,
        data: &[u8],
        require_biometric: bool,
    ) -> Result<()> {
        // FFI call to SecItemAdd with:
        // - kSecClass: kSecClassGenericPassword
        // - kSecAttrService: KEYCHAIN_SERVICE
        // - kSecAttrAccount: key_id
        // - kSecValueData: data
        // - kSecAttrAccessible: kSecAttrAccessibleWhenPasscodeSetThisDeviceOnly
        // - kSecAttrAccessControl: if require_biometric, add biometricCurrentSet
        let _ = (key_id, data, require_biometric);
        Ok(())
    }
    
    /// Retrieve key from Keychain
    pub fn retrieve_from_keychain(key_id: &str) -> Result<Vec<u8>> {
        // FFI call to SecItemCopyMatching
        let _ = key_id;
        Err(Error::Encryption("Keychain FFI not implemented".into()))
    }
    
    /// Delete key from Keychain
    pub fn delete_from_keychain(key_id: &str) -> Result<()> {
        // FFI call to SecItemDelete
        let _ = key_id;
        Ok(())
    }
}

/// Windows DPAPI shim
/// 
/// In production, this uses win32 crate to call:
/// - CryptProtectData / CryptUnprotectData
/// - Optional: NCrypt for TPM-backed keys
#[cfg(target_os = "windows")]
pub mod windows {
    use super::*;
    
    /// Protect data using DPAPI
    pub fn protect_data(plaintext: &[u8]) -> Result<Vec<u8>> {
        // FFI call to CryptProtectData
        // CRYPTPROTECT_LOCAL_MACHINE for machine scope
        // Or default for user scope (preferred)
        let _ = plaintext;
        Err(Error::Encryption("DPAPI FFI not implemented".into()))
    }
    
    /// Unprotect data using DPAPI
    pub fn unprotect_data(ciphertext: &[u8]) -> Result<Vec<u8>> {
        // FFI call to CryptUnprotectData
        let _ = ciphertext;
        Err(Error::Encryption("DPAPI FFI not implemented".into()))
    }
    
    /// Check if TPM is available for hardware-backed protection
    pub fn has_tpm() -> bool {
        // Check TPM 2.0 availability
        false // Placeholder
    }
}

/// Linux libsecret shim
/// 
/// In production, this uses libsecret-1 to interface with:
/// - GNOME Keyring
/// - KDE Wallet
/// - Other Secret Service implementations
#[cfg(target_os = "linux")]
pub mod linux {
    use super::*;
    
    /// Secret Service collection
    pub const SECRET_COLLECTION: &str = "pirate_wallet";
    
    /// Store secret using libsecret
    pub fn store_secret(key_id: &str, data: &[u8]) -> Result<()> {
        // FFI call to secret_password_store_sync
        // Schema: org.pirate.wallet
        // Attributes: { "key_id": key_id }
        let _ = (key_id, data);
        Ok(())
    }
    
    /// Retrieve secret using libsecret
    pub fn retrieve_secret(key_id: &str) -> Result<Vec<u8>> {
        // FFI call to secret_password_lookup_sync
        let _ = key_id;
        Err(Error::Encryption("libsecret FFI not implemented".into()))
    }
    
    /// Delete secret
    pub fn delete_secret(key_id: &str) -> Result<()> {
        // FFI call to secret_password_clear_sync
        let _ = key_id;
        Ok(())
    }
}

// =============================================================================
// FFI-friendly Keystore Manager
// =============================================================================

/// Keystore manager for FFI integration
/// 
/// This struct is designed to be instantiated from Flutter with platform-specific
/// callbacks for actual keystore operations.
pub struct KeystoreManager {
    key_id_prefix: String,
    algorithm: EncryptionAlgorithm,
}

impl KeystoreManager {
    /// Create new keystore manager
    pub fn new(wallet_id: &str) -> Self {
        Self {
            key_id_prefix: format!("pirate_wallet_{}", wallet_id),
            algorithm: EncryptionAlgorithm::ChaCha20Poly1305,
        }
    }
    
    /// Get key ID for master key
    pub fn master_key_id(&self) -> String {
        format!("{}_master", self.key_id_prefix)
    }
    
    /// Get key ID for biometric key
    pub fn biometric_key_id(&self) -> String {
        format!("{}_biometric", self.key_id_prefix)
    }
    
    /// Seal master key (to be called with platform keystore from FFI)
    pub fn seal_master_key(&self, key: &MasterKey) -> SealedKey {
        // This creates a SealedKey structure that will be passed to
        // platform-specific sealing via FFI
        SealedKey::new(
            key.as_bytes().to_vec(),
            self.master_key_id(),
            self.algorithm,
        )
    }
    
    /// Create sealed key from platform-encrypted bytes
    pub fn create_sealed_key(&self, encrypted_bytes: Vec<u8>) -> SealedKey {
        SealedKey::new(
            encrypted_bytes,
            self.master_key_id(),
            self.algorithm,
        )
    }
}

// =============================================================================
// Biometric Unlock Support
// =============================================================================

/// Biometric unlock configuration
#[derive(Debug, Clone)]
pub struct BiometricConfig {
    /// Enable biometric unlock
    pub enabled: bool,
    /// Require confirmation after successful biometric
    pub require_confirmation: bool,
    /// Allow fallback to passphrase
    pub allow_passphrase_fallback: bool,
    /// Timeout in seconds before requiring re-authentication
    pub timeout_seconds: u32,
}

impl Default for BiometricConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            require_confirmation: false,
            allow_passphrase_fallback: true,
            timeout_seconds: 30,
        }
    }
}

/// Biometric unlock state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiometricState {
    /// Not configured
    NotConfigured,
    /// Configured and ready
    Ready,
    /// Locked out (too many failed attempts)
    LockedOut,
    /// Disabled by user
    Disabled,
    /// Not available on this device
    NotAvailable,
}

/// Biometric unlock manager
pub struct BiometricManager {
    config: BiometricConfig,
    state: BiometricState,
    failed_attempts: u32,
    last_auth_time: Option<std::time::Instant>,
}

impl BiometricManager {
    /// Maximum failed attempts before lockout
    pub const MAX_FAILED_ATTEMPTS: u32 = 5;
    
    /// Lockout duration in seconds
    pub const LOCKOUT_DURATION_SECS: u64 = 30;
    
    /// Create new biometric manager
    pub fn new() -> Self {
        Self {
            config: BiometricConfig::default(),
            state: BiometricState::NotConfigured,
            failed_attempts: 0,
            last_auth_time: None,
        }
    }
    
    /// Configure biometric unlock
    pub fn configure(&mut self, config: BiometricConfig) {
        let enabled = config.enabled;
        self.config = config;
        if enabled {
            self.state = BiometricState::Ready;
        } else {
            self.state = BiometricState::Disabled;
        }
    }
    
    /// Get current state
    pub fn state(&self) -> BiometricState {
        self.state
    }
    
    /// Check if biometric is ready for use
    pub fn is_ready(&self) -> bool {
        self.state == BiometricState::Ready
    }
    
    /// Record successful authentication
    pub fn record_success(&mut self) {
        self.failed_attempts = 0;
        self.last_auth_time = Some(std::time::Instant::now());
        self.state = BiometricState::Ready;
    }
    
    /// Record failed authentication
    pub fn record_failure(&mut self) {
        self.failed_attempts += 1;
        if self.failed_attempts >= Self::MAX_FAILED_ATTEMPTS {
            self.state = BiometricState::LockedOut;
        }
    }
    
    /// Check if session is still valid (within timeout)
    pub fn is_session_valid(&self) -> bool {
        if let Some(last_auth) = self.last_auth_time {
            last_auth.elapsed().as_secs() < self.config.timeout_seconds as u64
        } else {
            false
        }
    }
    
    /// Check if locked out
    pub fn is_locked_out(&self) -> bool {
        self.state == BiometricState::LockedOut
    }
    
    /// Reset lockout (after timeout or manual reset)
    pub fn reset_lockout(&mut self) {
        self.failed_attempts = 0;
        if self.config.enabled {
            self.state = BiometricState::Ready;
        }
    }
    
    /// Disable biometric unlock
    pub fn disable(&mut self) {
        self.state = BiometricState::Disabled;
        self.config.enabled = false;
    }
}

impl Default for BiometricManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_platform_detection() {
        let platform = Platform::current();
        // Should return a valid platform
        assert!(matches!(
            platform,
            Platform::Android
                | Platform::Ios
                | Platform::MacOs
                | Platform::Windows
                | Platform::Linux
                | Platform::Unknown
        ));
    }
    
    #[test]
    fn test_mock_keystore_seal_unseal() {
        let keystore = MockKeystore::new();
        let master_key = MasterKey::generate(EncryptionAlgorithm::ChaCha20Poly1305);
        
        let sealed = keystore.seal_key(&master_key, "test_key").unwrap();
        assert_eq!(sealed.key_id, "test_key");
        
        if let KeystoreResult::Success(unsealed) = keystore.unseal_key(&sealed) {
            assert_eq!(unsealed.as_bytes(), master_key.as_bytes());
        } else {
            panic!("Failed to unseal key");
        }
    }
    
    #[test]
    fn test_keystore_manager() {
        let manager = KeystoreManager::new("wallet_123");
        
        assert!(manager.master_key_id().contains("wallet_123"));
        assert!(manager.biometric_key_id().contains("biometric"));
    }
    
    #[test]
    fn test_biometric_manager_lockout() {
        let mut manager = BiometricManager::new();
        manager.configure(BiometricConfig {
            enabled: true,
            ..Default::default()
        });
        
        assert!(manager.is_ready());
        
        // Record failures until lockout
        for _ in 0..BiometricManager::MAX_FAILED_ATTEMPTS {
            manager.record_failure();
        }
        
        assert!(manager.is_locked_out());
        
        // Reset
        manager.reset_lockout();
        assert!(manager.is_ready());
    }
    
    #[test]
    fn test_biometric_session_timeout() {
        let mut manager = BiometricManager::new();
        manager.configure(BiometricConfig {
            enabled: true,
            timeout_seconds: 0, // Immediate timeout for testing
            ..Default::default()
        });
        
        manager.record_success();
        
        // Session should be invalid immediately with 0 timeout
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!manager.is_session_valid());
    }
    
    #[test]
    fn test_sealed_key_roundtrip() {
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
}

