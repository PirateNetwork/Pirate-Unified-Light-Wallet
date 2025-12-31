//! End-to-end transport tests
//!
//! Tests proving traffic enforcement and privacy guarantees.

use pirate_net::{TransportManager, TransportConfig, TransportMode, Socks5Config, DnsConfig, DnsProvider, TorConfig};

#[tokio::test]
async fn test_tor_mode_requires_tor_client() {
    // Configure Tor mode but don't initialize Tor client
    let config = TransportConfig {
        mode: TransportMode::Tor,
        tor_enabled: false, // Tor disabled!
        socks5: None,
        dns_config: DnsConfig::default(),
    };

    let manager = TransportManager::new(config).await.unwrap();

    // Attempt to create HTTP client should fail
    let result = manager.create_http_client().await;
    
    assert!(result.is_err(), "Should fail when Tor is required but not initialized");
    assert!(result.unwrap_err().to_string().contains("Tor client not initialized"));
}

#[tokio::test]
async fn test_socks5_mode_requires_config() {
    // Configure SOCKS5 mode but don't provide config
    let config = TransportConfig {
        mode: TransportMode::Socks5,
        tor_enabled: false,
        socks5: None, // No SOCKS5 config!
        dns_config: DnsConfig::default(),
    };

    let manager = TransportManager::new(config).await.unwrap();

    // Should fail without SOCKS5 config
    let result = manager.create_http_client().await;
    
    assert!(result.is_err(), "Should fail when SOCKS5 config not provided");
    assert!(result.unwrap_err().to_string().contains("SOCKS5 config not provided"));
}

#[tokio::test]
async fn test_socks5_with_valid_config() {
    let socks5 = Socks5Config {
        host: "localhost".to_string(),
        port: 1080,
        username: None,
        password: None,
    };

    let config = TransportConfig {
        mode: TransportMode::Socks5,
        tor_enabled: false,
        socks5: Some(socks5),
        dns_config: DnsConfig::default(),
    };

    let manager = TransportManager::new(config).await.unwrap();

    // Should succeed with valid config
    let client = manager.create_http_client().await;
    assert!(client.is_ok(), "Should succeed with valid SOCKS5 config");
}

#[tokio::test]
async fn test_direct_mode_creates_client() {
    let config = TransportConfig {
        mode: TransportMode::Direct,
        tor_enabled: false,
        socks5: None,
        dns_config: DnsConfig::default(),
    };

    let manager = TransportManager::new(config).await.unwrap();

    // Direct mode should always succeed (but not private!)
    let client = manager.create_http_client().await;
    assert!(client.is_ok(), "Direct mode should create client without proxy");
}

#[tokio::test]
async fn test_privacy_status() {
    // Tor mode is private
    let tor_config = TransportConfig {
        mode: TransportMode::Tor,
        ..Default::default()
    };
    let tor_manager = TransportManager::new(tor_config).await.unwrap();
    assert!(tor_manager.is_private().await, "Tor should be private");

    // SOCKS5 mode is private
    let socks5_config = TransportConfig {
        mode: TransportMode::Socks5,
        socks5: Some(Socks5Config {
            host: "localhost".to_string(),
            port: 1080,
            username: None,
            password: None,
        }),
        ..Default::default()
    };
    let socks5_manager = TransportManager::new(socks5_config).await.unwrap();
    assert!(socks5_manager.is_private().await, "SOCKS5 should be private");

    // Direct mode is NOT private
    let direct_config = TransportConfig {
        mode: TransportMode::Direct,
        ..Default::default()
    };
    let direct_manager = TransportManager::new(direct_config).await.unwrap();
    assert!(!direct_manager.is_private().await, "Direct should NOT be private");
}

#[tokio::test]
async fn test_dns_tunneling() {
    let config = DnsConfig {
        provider: DnsProvider::CloudflareDoH,
        tunnel_dns: true,
        socks_proxy: Some("127.0.0.1:9050".to_string()),
    };

    let resolver = pirate_net::DnsResolver::new(config);
    
    // Verify DNS is configured for tunneling
    assert!(resolver.is_tunneled(), "DNS should be configured for tunneling");
}

#[tokio::test]
async fn test_dns_privacy_status() {
    // CloudflareDoH is private
    assert!(DnsProvider::CloudflareDoH.is_private());
    assert!(DnsProvider::Quad9DoH.is_private());
    assert!(DnsProvider::GoogleDoH.is_private());
    
    // System DNS is NOT private
    assert!(!DnsProvider::System.is_private());
}

#[test]
fn test_transport_mode_names() {
    assert_eq!(TransportMode::Tor.name(), "Tor (Most Private)");
    assert_eq!(TransportMode::Socks5.name(), "SOCKS5 Proxy");
    assert_eq!(TransportMode::Direct.name(), "Direct (Not Private)");
}

#[test]
fn test_dns_provider_names() {
    assert_eq!(DnsProvider::CloudflareDoH.name(), "Cloudflare (1.1.1.1)");
    assert_eq!(DnsProvider::Quad9DoH.name(), "Quad9 (9.9.9.9)");
    assert_eq!(DnsProvider::System.name(), "System (Not Private)");
}

