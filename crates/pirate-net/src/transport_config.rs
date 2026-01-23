//! Transport configuration storage
//!
//! Provides encrypted storage for transport settings.

use crate::{CertificatePin, DnsProvider, Socks5Config, TransportMode};
use argon2::{Algorithm, Argon2, ParamsBuilder, Version};
use base64::engine::general_purpose::STANDARD as Base64Standard;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::ChaCha20Poly1305;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Persistent transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTransportConfig {
    /// Transport mode
    pub mode: StoredTransportMode,
    /// Tor settings
    pub tor: TorSettings,
    /// I2P settings (desktop only)
    #[serde(default)]
    pub i2p: I2pSettings,
    /// SOCKS5 settings
    pub socks5: Option<Socks5Settings>,
    /// DNS settings
    pub dns: DnsSettings,
    /// TLS pinning settings
    pub tls_pinning: TlsPinningSettings,
}

const TRANSPORT_CONFIG_VERSION: u8 = 1;
const ARGON2_M_COST: u32 = 65_536;
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 4;
const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedTransportConfig {
    version: u8,
    salt_b64: String,
    nonce_b64: String,
    ciphertext_b64: String,
}

impl Default for StoredTransportConfig {
    fn default() -> Self {
        Self {
            mode: StoredTransportMode::Tor,
            tor: TorSettings::default(),
            i2p: I2pSettings::default(),
            socks5: None,
            dns: DnsSettings::default(),
            tls_pinning: TlsPinningSettings::default(),
        }
    }
}

/// Transport mode (serializable)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StoredTransportMode {
    /// Tor (default)
    #[serde(rename = "tor")]
    Tor,
    /// I2P (desktop only)
    #[serde(rename = "i2p")]
    I2p,
    /// SOCKS5 proxy
    #[serde(rename = "socks5")]
    Socks5,
    /// Direct connection (NOT RECOMMENDED)
    #[serde(rename = "direct")]
    Direct,
}

impl From<TransportMode> for StoredTransportMode {
    fn from(mode: TransportMode) -> Self {
        match mode {
            TransportMode::Tor => Self::Tor,
            TransportMode::I2p => Self::I2p,
            TransportMode::Socks5 => Self::Socks5,
            TransportMode::Direct => Self::Direct,
        }
    }
}

impl From<StoredTransportMode> for TransportMode {
    fn from(mode: StoredTransportMode) -> Self {
        match mode {
            StoredTransportMode::Tor => Self::Tor,
            StoredTransportMode::I2p => Self::I2p,
            StoredTransportMode::Socks5 => Self::Socks5,
            StoredTransportMode::Direct => Self::Direct,
        }
    }
}

/// Tor settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorSettings {
    /// Enable Tor
    pub enabled: bool,
    /// SOCKS5 port
    pub socks_port: u16,
    /// Enable debug logging
    pub debug: bool,
}

impl Default for TorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            socks_port: 9050,
            debug: false,
        }
    }
}

/// I2P settings (desktop only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct I2pSettings {
    /// Enable I2P
    pub enabled: bool,
    /// Optional path to embedded i2pd binary
    pub binary_path: Option<String>,
    /// Optional persistent data dir
    pub data_dir: Option<String>,
    /// Router listens on this address
    pub address: String,
    /// SOCKS proxy port
    pub socks_port: u16,
    /// Use ephemeral identities (new data dir per launch)
    pub ephemeral: bool,
}

impl Default for I2pSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            binary_path: None,
            data_dir: None,
            address: "127.0.0.1".to_string(),
            socks_port: 4447,
            ephemeral: true,
        }
    }
}

/// SOCKS5 settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Socks5Settings {
    /// Host address
    pub host: String,
    /// Port
    pub port: u16,
    /// Username (optional, encrypted)
    pub username: Option<String>,
    /// Password (optional, encrypted)
    pub password: Option<String>,
}

impl From<Socks5Config> for Socks5Settings {
    fn from(config: Socks5Config) -> Self {
        Self {
            host: config.host,
            port: config.port,
            username: config.username,
            password: config.password,
        }
    }
}

impl From<Socks5Settings> for Socks5Config {
    fn from(settings: Socks5Settings) -> Self {
        Self {
            host: settings.host,
            port: settings.port,
            username: settings.username,
            password: settings.password,
        }
    }
}

/// DNS settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsSettings {
    /// DNS provider
    pub provider: StoredDnsProvider,
    /// Custom DoH URL (if provider is Custom)
    pub custom_doh_url: Option<String>,
    /// Tunnel DNS through SOCKS proxy
    pub tunnel_dns: bool,
}

impl Default for DnsSettings {
    fn default() -> Self {
        Self {
            provider: StoredDnsProvider::CloudflareDoH,
            custom_doh_url: None,
            tunnel_dns: true,
        }
    }
}

/// DNS provider (serializable)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StoredDnsProvider {
    /// Cloudflare DoH
    #[serde(rename = "cloudflare_doh")]
    CloudflareDoH,
    /// Quad9 DoH
    #[serde(rename = "quad9_doh")]
    Quad9DoH,
    /// Google DoH
    #[serde(rename = "google_doh")]
    GoogleDoH,
    /// Custom DoH
    #[serde(rename = "custom_doh")]
    CustomDoH,
    /// System (not recommended)
    #[serde(rename = "system", alias = "dnscrypt")]
    System,
}

impl From<DnsProvider> for StoredDnsProvider {
    fn from(provider: DnsProvider) -> Self {
        match provider {
            DnsProvider::CloudflareDoH => Self::CloudflareDoH,
            DnsProvider::Quad9DoH => Self::Quad9DoH,
            DnsProvider::GoogleDoH => Self::GoogleDoH,
            DnsProvider::CustomDoH(_) => Self::CustomDoH,
            DnsProvider::System => Self::System,
        }
    }
}

/// TLS pinning settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsPinningSettings {
    /// Enable TLS pinning enforcement
    pub enforce: bool,
    /// Certificate pins
    pub pins: Vec<CertificatePin>,
}

impl Default for TlsPinningSettings {
    fn default() -> Self {
        Self {
            enforce: true,
            pins: vec![],
        }
    }
}

/// Configuration storage manager
pub struct TransportConfigStorage {
    config: StoredTransportConfig,
}

impl TransportConfigStorage {
    /// Create new config storage
    pub fn new() -> Self {
        Self {
            config: StoredTransportConfig::default(),
        }
    }

    /// Load configuration from encrypted storage
    pub fn load(&mut self, encrypted_json: &str, passphrase: &str) -> crate::Result<()> {
        if let Ok(encrypted) = serde_json::from_str::<EncryptedTransportConfig>(encrypted_json) {
            self.config = decrypt_config(encrypted, passphrase)?;
            return Ok(());
        }

        self.config = serde_json::from_str(encrypted_json)
            .map_err(|e| crate::Error::Network(format!("Failed to parse config: {}", e)))?;
        Ok(())
    }

    /// Save configuration to encrypted storage
    pub fn save(&self, passphrase: &str) -> crate::Result<String> {
        encrypt_config(&self.config, passphrase)
    }

    /// Get current configuration
    pub fn get(&self) -> &StoredTransportConfig {
        &self.config
    }

    /// Update configuration
    pub fn update(&mut self, config: StoredTransportConfig) {
        self.config = config;
    }
}

impl Default for TransportConfigStorage {
    fn default() -> Self {
        Self::new()
    }
}

fn encrypt_config(config: &StoredTransportConfig, passphrase: &str) -> crate::Result<String> {
    let plaintext = serde_json::to_vec(config)
        .map_err(|e| crate::Error::Network(format!("Failed to serialize config: {}", e)))?;

    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = derive_key(passphrase, &salt)?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

    let cipher = ChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(key.as_ref()));
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| crate::Error::Network(format!("Failed to encrypt config: {}", e)))?;

    let envelope = EncryptedTransportConfig {
        version: TRANSPORT_CONFIG_VERSION,
        salt_b64: Base64Standard.encode(salt),
        nonce_b64: Base64Standard.encode(nonce_bytes),
        ciphertext_b64: Base64Standard.encode(ciphertext),
    };

    serde_json::to_string_pretty(&envelope)
        .map_err(|e| crate::Error::Network(format!("Failed to serialize encrypted config: {}", e)))
}

fn decrypt_config(
    encrypted: EncryptedTransportConfig,
    passphrase: &str,
) -> crate::Result<StoredTransportConfig> {
    if encrypted.version != TRANSPORT_CONFIG_VERSION {
        return Err(crate::Error::Network(format!(
            "Unsupported transport config version: {}",
            encrypted.version
        )));
    }

    let salt = decode_b64("salt", &encrypted.salt_b64)?;
    let nonce_bytes = decode_b64("nonce", &encrypted.nonce_b64)?;
    let ciphertext = decode_b64("ciphertext", &encrypted.ciphertext_b64)?;

    if salt.len() != SALT_LEN {
        return Err(crate::Error::Network(format!(
            "Invalid salt length: {}",
            salt.len()
        )));
    }

    if nonce_bytes.len() != NONCE_LEN {
        return Err(crate::Error::Network(format!(
            "Invalid nonce length: {}",
            nonce_bytes.len()
        )));
    }

    let key = derive_key(passphrase, &salt)?;
    let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);
    let cipher = ChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(key.as_ref()));
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|e| crate::Error::Network(format!("Failed to decrypt config: {}", e)))?;

    serde_json::from_slice(&plaintext)
        .map_err(|e| crate::Error::Network(format!("Failed to parse config: {}", e)))
}

fn derive_key(passphrase: &str, salt: &[u8]) -> crate::Result<Zeroizing<[u8; 32]>> {
    if salt.len() < 16 {
        return Err(crate::Error::Network("Salt too short".to_string()));
    }

    let params = ParamsBuilder::new()
        .m_cost(ARGON2_M_COST)
        .t_cost(ARGON2_T_COST)
        .p_cost(ARGON2_P_COST)
        .output_len(32)
        .build()
        .map_err(|e| crate::Error::Network(format!("Argon2 params error: {}", e)))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
        .map_err(|e| crate::Error::Network(format!("Key derivation failed: {}", e)))?;

    Ok(key)
}

fn decode_b64(label: &str, value: &str) -> crate::Result<Vec<u8>> {
    Base64Standard
        .decode(value.as_bytes())
        .map_err(|e| crate::Error::Network(format!("Invalid {} base64: {}", label, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_config_serialization() {
        let config = StoredTransportConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: StoredTransportConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.mode, StoredTransportMode::Tor);
    }

    #[test]
    fn test_config_storage() {
        let storage = TransportConfigStorage::new();
        let json = storage.save("test_pass").unwrap();
        assert!(json.contains("\"ciphertext_b64\""));

        let mut storage2 = TransportConfigStorage::new();
        storage2.load(&json, "test_pass").unwrap();

        assert_eq!(storage2.get().mode, StoredTransportMode::Tor);
    }

    #[test]
    fn test_config_storage_wrong_passphrase() {
        let storage = TransportConfigStorage::new();
        let json = storage.save("test_pass").unwrap();

        let mut storage2 = TransportConfigStorage::new();
        assert!(storage2.load(&json, "wrong_pass").is_err());
    }

    #[test]
    fn test_config_storage_plaintext_fallback() {
        let config = StoredTransportConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();

        let mut storage = TransportConfigStorage::new();
        storage.load(&json, "test_pass").unwrap();

        assert_eq!(storage.get().mode, StoredTransportMode::Tor);
    }
}
