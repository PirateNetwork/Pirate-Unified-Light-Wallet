//! Privacy-preserving network tunnel integration for background sync
//!
//! Ensures all background sync operations respect the configured network tunnel (Tor/SOCKS5).

#![allow(missing_docs)]

use crate::{Error, Result};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Network tunnel configuration
#[derive(Debug, Clone)]
pub enum TunnelConfig {
    /// Tor via Arti
    Tor { socks_port: u16 },
    /// SOCKS5 proxy
    Socks5 {
        host: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    },
    /// Direct connection (not recommended for privacy)
    Direct,
}

impl Default for TunnelConfig {
    fn default() -> Self {
        TunnelConfig::Tor { socks_port: 9050 }
    }
}

/// Network tunnel manager
pub struct TunnelManager {
    config: Arc<RwLock<TunnelConfig>>,
}

impl TunnelManager {
    /// Create new tunnel manager
    pub fn new(config: TunnelConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Get current tunnel configuration
    pub async fn get_config(&self) -> TunnelConfig {
        self.config.read().await.clone()
    }

    /// Update tunnel configuration
    pub async fn set_config(&self, config: TunnelConfig) {
        *self.config.write().await = config;
    }

    /// Verify tunnel is active and working
    pub async fn verify_tunnel(&self) -> Result<()> {
        let config = self.get_config().await;

        match config {
            TunnelConfig::Tor { socks_port } => self.verify_tor(socks_port).await,
            TunnelConfig::Socks5 { host, port, .. } => self.verify_socks5(&host, port).await,
            TunnelConfig::Direct => {
                tracing::warn!("Direct connection mode - privacy not guaranteed");
                Ok(())
            }
        }
    }

    /// Verify Tor is accessible
    async fn verify_tor(&self, port: u16) -> Result<()> {
        // Try to connect to Tor SOCKS port
        match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
            Ok(_) => {
                tracing::info!("Tor tunnel verified on port {}", port);
                Ok(())
            }
            Err(e) => {
                tracing::error!("Tor tunnel verification failed: {}", e);
                Err(Error::Network(format!(
                    "Tor not accessible on port {}: {}",
                    port, e
                )))
            }
        }
    }

    /// Verify SOCKS5 proxy is accessible
    async fn verify_socks5(&self, host: &str, port: u16) -> Result<()> {
        match tokio::net::TcpStream::connect((host, port)).await {
            Ok(_) => {
                tracing::info!("SOCKS5 tunnel verified at {}:{}", host, port);
                Ok(())
            }
            Err(e) => {
                tracing::error!("SOCKS5 tunnel verification failed: {}", e);
                Err(Error::Network(format!(
                    "SOCKS5 proxy not accessible at {}:{}: {}",
                    host, port, e
                )))
            }
        }
    }

    /// Get HTTP client configured for current tunnel
    pub async fn create_http_client(&self) -> Result<reqwest::Client> {
        let config = self.get_config().await;

        let client_builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(60));

        let client = match config {
            TunnelConfig::Tor { socks_port } => {
                let proxy = reqwest::Proxy::all(format!("socks5://127.0.0.1:{}", socks_port))
                    .map_err(|e| Error::Network(format!("Failed to create Tor proxy: {}", e)))?;

                client_builder
                    .proxy(proxy)
                    .build()
                    .map_err(|e| Error::Network(format!("Failed to create HTTP client: {}", e)))?
            }
            TunnelConfig::Socks5 {
                host,
                port,
                username,
                password,
            } => {
                let proxy_url = if let (Some(user), Some(pass)) = (username, password) {
                    format!("socks5://{}:{}@{}:{}", user, pass, host, port)
                } else {
                    format!("socks5://{}:{}", host, port)
                };

                let proxy = reqwest::Proxy::all(proxy_url)
                    .map_err(|e| Error::Network(format!("Failed to create SOCKS5 proxy: {}", e)))?;

                client_builder
                    .proxy(proxy)
                    .build()
                    .map_err(|e| Error::Network(format!("Failed to create HTTP client: {}", e)))?
            }
            TunnelConfig::Direct => client_builder
                .build()
                .map_err(|e| Error::Network(format!("Failed to create HTTP client: {}", e)))?,
        };

        Ok(client)
    }

    /// Check if tunnel is privacy-preserving
    pub async fn is_privacy_preserving(&self) -> bool {
        let config = self.get_config().await;
        !matches!(config, TunnelConfig::Direct)
    }
}

/// Background sync tunnel guard
///
/// Ensures background sync operations always use the configured tunnel
pub struct BackgroundSyncTunnelGuard {
    tunnel_manager: Arc<TunnelManager>,
}

impl BackgroundSyncTunnelGuard {
    /// Create new tunnel guard
    pub fn new(tunnel_manager: Arc<TunnelManager>) -> Self {
        Self { tunnel_manager }
    }

    /// Execute operation with tunnel verification
    pub async fn with_tunnel<F, T>(&self, operation: F) -> Result<T>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        // Verify tunnel before executing
        self.tunnel_manager.verify_tunnel().await?;

        // Execute operation
        operation.await
    }

    /// Log tunnel status for observability
    pub async fn log_tunnel_status(&self) {
        let config = self.tunnel_manager.get_config().await;
        let is_privacy = self.tunnel_manager.is_privacy_preserving().await;

        tracing::info!(
            "Background sync tunnel status: {:?}, privacy_preserving={}",
            config,
            is_privacy
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_config_default() {
        let config = TunnelConfig::default();
        match config {
            TunnelConfig::Tor { socks_port } => {
                assert_eq!(socks_port, 9050);
            }
            _ => panic!("Default should be Tor"),
        }
    }

    #[tokio::test]
    async fn test_tunnel_manager() {
        let manager = TunnelManager::new(TunnelConfig::Direct);

        // Verify is callable
        let _ = manager.verify_tunnel().await;

        // Update config
        manager
            .set_config(TunnelConfig::Tor { socks_port: 9050 })
            .await;

        let config = manager.get_config().await;
        match config {
            TunnelConfig::Tor { socks_port } => {
                assert_eq!(socks_port, 9050);
            }
            _ => panic!("Config should be Tor"),
        }
    }

    #[tokio::test]
    async fn test_privacy_check() {
        let manager = TunnelManager::new(TunnelConfig::Direct);
        assert!(!manager.is_privacy_preserving().await);

        manager
            .set_config(TunnelConfig::Tor { socks_port: 9050 })
            .await;
        assert!(manager.is_privacy_preserving().await);
    }
}
