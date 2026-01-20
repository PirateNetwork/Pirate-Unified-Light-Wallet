//! Transport configuration storage
//!
//! Provides encrypted storage for transport settings.

use crate::{TransportMode, Socks5Config, DnsProvider, CertificatePin};
use serde::{Serialize, Deserialize};

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
    pub fn load(&mut self, encrypted_json: &str, _passphrase: &str) -> crate::Result<()> {
        // TODO: Decrypt with passphrase
        // For now, just parse JSON
        self.config = serde_json::from_str(encrypted_json)
            .map_err(|e| crate::Error::Network(format!("Failed to parse config: {}", e)))?;
        Ok(())
    }

    /// Save configuration to encrypted storage
    pub fn save(&self, _passphrase: &str) -> crate::Result<String> {
        // TODO: Encrypt with passphrase
        // For now, just serialize
        serde_json::to_string_pretty(&self.config)
            .map_err(|e| crate::Error::Network(format!("Failed to serialize config: {}", e)))
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
        
        let mut storage2 = TransportConfigStorage::new();
        storage2.load(&json, "test_pass").unwrap();
        
        assert_eq!(storage2.get().mode, StoredTransportMode::Tor);
    }
}

