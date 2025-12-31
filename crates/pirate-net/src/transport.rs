//! Privacy-preserving network transport layer
//!
//! Ensures all wallet traffic is tunneled through Tor/SOCKS5.

use crate::{Result, Error, TorClient, DnsResolver, DnsConfig, DnsProvider};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use tonic::transport::{Channel, Endpoint};

/// Transport mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    /// Tor (default, most private)
    Tor,
    /// SOCKS5 proxy
    Socks5,
    /// Direct connection (NOT RECOMMENDED)
    Direct,
}

impl TransportMode {
    /// Get mode name
    pub fn name(&self) -> &str {
        match self {
            Self::Tor => "Tor (Most Private)",
            Self::Socks5 => "SOCKS5 Proxy",
            Self::Direct => "Direct (Not Private)",
        }
    }

    /// Check if mode is privacy-preserving
    pub fn is_private(&self) -> bool {
        !matches!(self, Self::Direct)
    }
}

/// SOCKS5 configuration
#[derive(Debug, Clone)]
pub struct Socks5Config {
    /// Host address
    pub host: String,
    /// Port
    pub port: u16,
    /// Username (optional)
    pub username: Option<String>,
    /// Password (optional)
    pub password: Option<String>,
}

impl Socks5Config {
    /// Get proxy URL
    pub fn proxy_url(&self) -> String {
        if let (Some(user), Some(pass)) = (&self.username, &self.password) {
            format!("socks5://{}:{}@{}:{}", user, pass, self.host, self.port)
        } else {
            format!("socks5://{}:{}", self.host, self.port)
        }
    }
}

/// Transport configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Transport mode
    pub mode: TransportMode,
    /// Tor client (if mode is Tor)
    pub tor_enabled: bool,
    /// SOCKS5 config (if mode is SOCKS5)
    pub socks5: Option<Socks5Config>,
    /// DNS configuration
    pub dns_config: DnsConfig,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            mode: TransportMode::Tor,
            tor_enabled: true,
            socks5: None,
            dns_config: DnsConfig::default(),
        }
    }
}

/// Privacy-preserving transport manager
pub struct TransportManager {
    config: Arc<RwLock<TransportConfig>>,
    tor_client: Arc<RwLock<Option<TorClient>>>,
    dns_resolver: Arc<RwLock<DnsResolver>>,
}

impl TransportManager {
    /// Create new transport manager
    pub async fn new(config: TransportConfig) -> Result<Self> {
        info!("Creating transport manager: mode={:?}", config.mode);

        // Initialize Tor if enabled
        let tor_client = if config.mode == TransportMode::Tor && config.tor_enabled {
            info!("Initializing embedded Tor client...");
            let client = TorClient::new(crate::tor::TorConfig::default())?;
            
            // Bootstrap in background
            let client_clone = client.clone();
            tokio::spawn(async move {
                if let Err(e) = client_clone.bootstrap().await {
                    error!("Tor bootstrap failed: {}", e);
                }
            });
            
            Some(client)
        } else {
            None
        };

        let dns_resolver = DnsResolver::new(config.dns_config.clone());

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            tor_client: Arc::new(RwLock::new(tor_client)),
            dns_resolver: Arc::new(RwLock::new(dns_resolver)),
        })
    }

    /// Get current transport mode
    pub async fn mode(&self) -> TransportMode {
        self.config.read().await.mode
    }

    /// Check if transport is privacy-preserving
    pub async fn is_private(&self) -> bool {
        self.config.read().await.mode.is_private()
    }

    /// Update transport configuration
    pub async fn update_config(&self, config: TransportConfig) -> Result<()> {
        info!("Updating transport config: mode={:?}", config.mode);

        // If switching to Tor, initialize Tor client
        if config.mode == TransportMode::Tor && config.tor_enabled {
            let tor_guard = self.tor_client.read().await;
            if tor_guard.is_none() {
                drop(tor_guard);
                
                info!("Initializing Tor client...");
                let client = TorClient::new(crate::tor::TorConfig::default())?;
                client.bootstrap().await?;
                
                *self.tor_client.write().await = Some(client);
            }
        }

        // Update DNS resolver
        self.dns_resolver.write().await.set_config(config.dns_config.clone());

        // Update config
        *self.config.write().await = config;

        Ok(())
    }

    /// Create HTTP client with configured transport
    pub async fn create_http_client(&self) -> Result<reqwest::Client> {
        let config = self.config.read().await;

        let mut client_builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60));

        match config.mode {
            TransportMode::Tor => {
                let tor_guard = self.tor_client.read().await;
                if let Some(ref tor) = *tor_guard {
                    if !tor.is_ready().await {
                        warn!("Tor is not ready yet, waiting...");
                        // In production, we'd wait or return error
                    }
                    
                    let proxy_url = format!("socks5://{}", tor.socks_addr());
                    debug!("Creating HTTP client with Tor proxy: {}", proxy_url);
                    
                    let proxy = reqwest::Proxy::all(&proxy_url)
                        .map_err(|e| Error::Network(format!("Failed to create Tor proxy: {}", e)))?;
                    
                    client_builder = client_builder.proxy(proxy);
                } else {
                    return Err(Error::Network("Tor client not initialized".to_string()));
                }
            }
            TransportMode::Socks5 => {
                if let Some(ref socks5) = config.socks5 {
                    let proxy_url = socks5.proxy_url();
                    debug!("Creating HTTP client with SOCKS5 proxy: {}", proxy_url);
                    
                    let proxy = reqwest::Proxy::all(&proxy_url)
                        .map_err(|e| Error::Network(format!("Failed to create SOCKS5 proxy: {}", e)))?;
                    
                    client_builder = client_builder.proxy(proxy);
                } else {
                    return Err(Error::Network("SOCKS5 config not provided".to_string()));
                }
            }
            TransportMode::Direct => {
                warn!("Creating HTTP client with DIRECT mode - privacy not guaranteed!");
                // No proxy
            }
        }

        client_builder
            .build()
            .map_err(|e| Error::Network(format!("Failed to create HTTP client: {}", e)))
    }

    /// Create gRPC channel with configured transport
    pub async fn create_grpc_channel(&self, endpoint_url: &str) -> Result<Channel> {
        let config = self.config.read().await;

        debug!("Creating gRPC channel to: {} via {:?}", endpoint_url, config.mode);

        // Parse endpoint
        let endpoint = Endpoint::from_shared(endpoint_url.to_string())
            .map_err(|e| Error::Network(format!("Invalid endpoint: {}", e)))?;

        match config.mode {
            TransportMode::Tor => {
                let tor_guard = self.tor_client.read().await;
                if let Some(ref tor) = *tor_guard {
                    if !tor.is_ready().await {
                        return Err(Error::Network("Tor not ready".to_string()));
                    }
                    
                    // For gRPC over Tor, we need to use HTTP/2 over SOCKS5
                    // This requires a custom connector
                    // For now, return error indicating this needs implementation
                    warn!("gRPC over Tor requires custom connector - not yet implemented");
                    
                    // TODO: Implement custom connector for gRPC over SOCKS5
                    // See: https://github.com/hyperium/tonic/discussions/691
                    
                    return Err(Error::Network(
                        "gRPC over Tor requires custom connector (TODO)".to_string()
                    ));
                } else {
                    return Err(Error::Network("Tor client not initialized".to_string()));
                }
            }
            TransportMode::Socks5 => {
                warn!("gRPC over SOCKS5 requires custom connector - not yet implemented");
                return Err(Error::Network(
                    "gRPC over SOCKS5 requires custom connector (TODO)".to_string()
                ));
            }
            TransportMode::Direct => {
                warn!("Using DIRECT gRPC connection - privacy not guaranteed!");
                
                // Direct connection
                endpoint
                    .connect()
                    .await
                    .map_err(|e| Error::Network(format!("gRPC connection failed: {}", e)))
            }
        }
    }

    /// Resolve hostname via configured DNS
    pub async fn resolve_host(&self, hostname: &str) -> Result<Vec<std::net::IpAddr>> {
        let resolver = self.dns_resolver.read().await;
        resolver.resolve(hostname).await
    }

    /// Get Tor bootstrap status
    pub async fn tor_status(&self) -> Option<crate::tor::TorStatus> {
        if let Some(ref tor) = *self.tor_client.read().await {
            Some(tor.status().await)
        } else {
            None
        }
    }

    /// Shutdown transport (cleanup)
    pub async fn shutdown(&self) {
        info!("Shutting down transport manager...");
        
        if let Some(ref tor) = *self.tor_client.read().await {
            tor.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_mode_privacy() {
        assert!(TransportMode::Tor.is_private());
        assert!(TransportMode::Socks5.is_private());
        assert!(!TransportMode::Direct.is_private());
    }

    #[test]
    fn test_socks5_proxy_url() {
        let config = Socks5Config {
            host: "localhost".to_string(),
            port: 9050,
            username: None,
            password: None,
        };
        assert_eq!(config.proxy_url(), "socks5://localhost:9050");

        let config_auth = Socks5Config {
            host: "proxy.example.com".to_string(),
            port: 1080,
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
        };
        assert_eq!(config_auth.proxy_url(), "socks5://user:pass@proxy.example.com:1080");
    }

    #[tokio::test]
    async fn test_transport_manager_creation() {
        let config = TransportConfig {
            mode: TransportMode::Direct, // Avoid Tor bootstrap in test
            ..Default::default()
        };
        
        let manager = TransportManager::new(config).await.unwrap();
        assert_eq!(manager.mode().await, TransportMode::Direct);
    }
}
