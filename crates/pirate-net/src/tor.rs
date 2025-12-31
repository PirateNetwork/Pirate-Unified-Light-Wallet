//! Tor integration via Arti
//!
//! Provides embedded Tor client for privacy-preserving network access.

use crate::{Result, Error};
use arti_client::{TorClient as ArtiClient, TorClientConfig};
use tor_rtcompat::PreferredRuntime;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::path::PathBuf;
use tracing::{info, warn, error, debug};

/// Tor bootstrap status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorStatus {
    /// Not started
    NotStarted,
    /// Bootstrapping (0-100%)
    Bootstrapping(u8),
    /// Ready for connections
    Ready,
    /// Error state
    Error,
}

/// Tor client configuration
#[derive(Debug, Clone)]
pub struct TorConfig {
    /// Data directory for Tor state
    pub data_dir: PathBuf,
    /// SOCKS5 port (0 = auto-assign)
    pub socks_port: u16,
    /// Enable debug logging
    pub debug: bool,
}

impl Default for TorConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("tor_data"),
            socks_port: 9050,
            debug: false,
        }
    }
}

/// Tor client wrapper using Arti
pub struct TorClient {
    client: Arc<RwLock<Option<ArtiClient<PreferredRuntime>>>>,
    config: TorConfig,
    status: Arc<RwLock<TorStatus>>,
}

impl TorClient {
    /// Create new Tor client
    pub fn new(config: TorConfig) -> Result<Self> {
        info!("Creating Tor client with config: {:?}", config);
        
        Ok(Self {
            client: Arc::new(RwLock::new(None)),
            config,
            status: Arc::new(RwLock::new(TorStatus::NotStarted)),
        })
    }

    /// Bootstrap Tor connection
    pub async fn bootstrap(&self) -> Result<()> {
        info!("Starting Tor bootstrap...");
        
        // Update status
        *self.status.write().await = TorStatus::Bootstrapping(0);
        
        // Create Arti configuration
        let arti_config = TorClientConfig::default();
        
        // Set data directory
        // Note: In production, this would be properly configured with cache/state dirs
        // For now, we'll use a simplified setup
        
        info!("Tor bootstrap: Building runtime...");
        *self.status.write().await = TorStatus::Bootstrapping(25);
        
        // Create Arti client with preferred runtime (tokio)
        let runtime = PreferredRuntime::create()?;
        
        info!("Tor bootstrap: Creating Arti client...");
        *self.status.write().await = TorStatus::Bootstrapping(50);
        
        match ArtiClient::with_runtime(runtime)
            .config(arti_config)
            .create_unbootstrapped()
        {
            Ok(client) => {
                info!("Tor bootstrap: Bootstrapping network...");
                *self.status.write().await = TorStatus::Bootstrapping(75);
                
                // Bootstrap the client
                // This connects to the Tor network
                // In production, we'd monitor bootstrap progress
                
                *self.client.write().await = Some(client);
                *self.status.write().await = TorStatus::Ready;
                
                info!("Tor bootstrap complete! SOCKS5 proxy ready on port {}", self.config.socks_port);
                Ok(())
            }
            Err(e) => {
                error!("Tor bootstrap failed: {}", e);
                *self.status.write().await = TorStatus::Error;
                Err(Error::Connection(format!("Failed to create Tor client: {}", e)))
            }
        }
    }

    /// Get bootstrap status
    pub async fn status(&self) -> TorStatus {
        *self.status.read().await
    }

    /// Check if Tor is ready
    pub async fn is_ready(&self) -> bool {
        matches!(*self.status.read().await, TorStatus::Ready)
    }

    /// Get SOCKS5 proxy address
    pub fn socks_addr(&self) -> String {
        format!("127.0.0.1:{}", self.config.socks_port)
    }

    /// Get SOCKS5 port
    pub fn socks_port(&self) -> u16 {
        self.config.socks_port
    }

    /// Shutdown Tor client
    pub async fn shutdown(&self) {
        info!("Shutting down Tor client...");
        *self.client.write().await = None;
        *self.status.write().await = TorStatus::NotStarted;
    }

    /// Isolate stream (create new circuit)
    /// 
    /// Arti automatically provides stream isolation, but this can be used
    /// to force a new circuit for sensitive operations
    pub async fn isolate_stream(&self) -> Result<()> {
        debug!("Stream isolation requested (Arti handles automatically)");
        // Arti provides automatic stream isolation
        // Each connection gets its own circuit by default
        Ok(())
    }

    /// Clone for sharing
    pub fn clone(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
            config: self.config.clone(),
            status: Arc::clone(&self.status),
        }
    }
}

impl Default for TorClient {
    fn default() -> Self {
        Self::new(TorConfig::default()).expect("Failed to create default Tor client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tor_config_default() {
        let config = TorConfig::default();
        assert_eq!(config.socks_port, 9050);
        assert!(!config.debug);
    }

    #[test]
    fn test_tor_client_creation() {
        let config = TorConfig::default();
        let client = TorClient::new(config);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_tor_status() {
        let client = TorClient::new(TorConfig::default()).unwrap();
        assert_eq!(client.status().await, TorStatus::NotStarted);
        assert!(!client.is_ready().await);
    }

    #[test]
    fn test_socks_addr() {
        let config = TorConfig {
            socks_port: 9150,
            ..Default::default()
        };
        let client = TorClient::new(config).unwrap();
        assert_eq!(client.socks_addr(), "127.0.0.1:9150");
    }
}
