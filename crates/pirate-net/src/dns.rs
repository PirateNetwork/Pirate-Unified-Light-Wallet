//! DNS resolution via DoH or system resolver.
//!
//! Direct connections use the operating system resolver by default. Explicit
//! DNS-over-HTTPS providers encrypt transport to a third-party resolver.

use crate::debug_log::log_debug_event;
use crate::Result;
use std::net::IpAddr;
use tracing::{debug, info, warn};

/// DNS resolver provider
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsProvider {
    /// Cloudflare DoH (1.1.1.1)
    CloudflareDoH,
    /// Quad9 DoH (9.9.9.9)
    Quad9DoH,
    /// Google DoH (8.8.8.8)
    GoogleDoH,
    /// Custom DoH endpoint
    CustomDoH(String),
    /// Operating system resolver (including device, VPN, and network policy)
    System,
}

impl DnsProvider {
    /// Get DoH endpoint URL
    pub fn doh_url(&self) -> Option<String> {
        match self {
            Self::CloudflareDoH => Some("https://cloudflare-dns.com/dns-query".to_string()),
            Self::Quad9DoH => Some("https://dns.quad9.net/dns-query".to_string()),
            Self::GoogleDoH => Some("https://dns.google/dns-query".to_string()),
            Self::CustomDoH(url) => Some(url.clone()),
            _ => None,
        }
    }

    /// Get provider name for display
    pub fn name(&self) -> &str {
        match self {
            Self::CloudflareDoH => "Cloudflare (1.1.1.1)",
            Self::Quad9DoH => "Quad9 (9.9.9.9)",
            Self::GoogleDoH => "Google (8.8.8.8)",
            Self::CustomDoH(_) => "Custom DoH",
            Self::System => "System",
        }
    }

    /// Whether queries use an HTTPS connection to a designated resolver.
    ///
    /// This describes transport encryption, not anonymity: the resolver can
    /// still observe the requested hostname and connecting IP address.
    pub fn uses_encrypted_transport(&self) -> bool {
        !matches!(self, Self::System)
    }
}

/// DNS resolver configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsConfig {
    /// DNS provider
    pub provider: DnsProvider,
    /// Tunnel DNS through SOCKS proxy
    pub tunnel_dns: bool,
    /// SOCKS proxy URL (if tunneling)
    pub socks_proxy: Option<String>,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            provider: DnsProvider::System,
            tunnel_dns: true,
            socks_proxy: Some("socks5h://127.0.0.1:9050".to_string()),
        }
    }
}

/// DNS resolver
#[derive(Clone)]
pub struct DnsResolver {
    config: DnsConfig,
}

impl DnsResolver {
    /// Create new DNS resolver
    pub fn new(config: DnsConfig) -> Self {
        info!("Creating DNS resolver: {:?}", config.provider.name());

        Self { config }
    }

    /// Resolve hostname to IP addresses
    pub async fn resolve(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        self.clone().resolve_owned(hostname.to_string()).await
    }

    /// Resolve an owned hostname without retaining a borrowed resolver across
    /// awaits. This is used by detached endpoint health probes.
    pub async fn resolve_owned(self, hostname: String) -> Result<Vec<IpAddr>> {
        debug!(
            "Resolving hostname: {} via {}",
            hostname,
            self.config.provider.name()
        );

        if matches!(&self.config.provider, DnsProvider::System) {
            Self::resolve_system_owned(hostname).await
        } else if self.config.provider.doh_url().is_some() {
            Self::resolve_doh_owned(self.config, hostname).await
        } else {
            Self::resolve_system_owned(hostname).await
        }
    }

    /// Resolve via DNS-over-HTTPS
    async fn resolve_doh_owned(config: DnsConfig, hostname: String) -> Result<Vec<IpAddr>> {
        let doh_url = config
            .provider
            .doh_url()
            .expect("DoH provider checked before resolution")
            .to_string();

        debug!("DoH resolution: {} via {}", hostname, doh_url);

        // Build HTTP client
        let client = if config.tunnel_dns {
            let proxy = config.socks_proxy.ok_or_else(|| {
                crate::Error::Network(
                    "DNS tunneling was requested without a SOCKS proxy".to_string(),
                )
            })?;
            let proxy_url = if proxy.contains("://") {
                proxy
            } else {
                format!("socks5h://{}", proxy)
            };
            debug!("Tunneling DNS through SOCKS proxy: {}", proxy_url);
            reqwest::Client::builder()
                .proxy(
                    reqwest::Proxy::all(proxy_url)
                        .map_err(|e| crate::Error::Network(format!("Proxy error: {}", e)))?,
                )
                .build()
                .map_err(|e| crate::Error::Network(format!("HTTP client error: {}", e)))?
        } else {
            reqwest::Client::new()
        };

        let mut addrs = Vec::new();

        for record_type in ["A", "AAAA"] {
            let response = client
                .get(&doh_url)
                .query(&[("name", hostname.as_str()), ("type", record_type)])
                .header("Accept", "application/dns-json")
                .send()
                .await
                .map_err(|e| crate::Error::Network(format!("DoH query failed: {}", e)))?;

            if !response.status().is_success() {
                warn!(
                    "DoH query failed with status {} for {} ({})",
                    response.status(),
                    hostname,
                    record_type
                );
                continue;
            }

            let body = response.text().await.map_err(|e| {
                crate::Error::Network(format!("Failed to read DoH response: {}", e))
            })?;

            debug!("DoH response ({}) for {}: {}", record_type, hostname, body);

            addrs.extend(parse_doh_response(&body));
        }

        if addrs.is_empty() {
            return Err(crate::Error::Network(format!(
                "DoH returned no usable addresses for {}",
                hostname
            )));
        }

        Ok(addrs)
    }

    /// Resolve through the operating system's configured DNS policy.
    async fn resolve_system_owned(hostname: String) -> Result<Vec<IpAddr>> {
        use tokio::net::lookup_host;

        debug!("Resolving {} with the operating system resolver", hostname);
        log_debug_event(
            "dns.rs:DnsResolver::resolve_system",
            "dns_system_resolve",
            &format!("host={}", hostname),
        );

        let addrs: Vec<IpAddr> = lookup_host(format!("{}:443", hostname))
            .await
            .map_err(|e| crate::Error::Network(format!("DNS resolution failed: {}", e)))?
            .map(|addr| addr.ip())
            .collect();

        debug!("Resolved {} to {:?}", hostname, addrs);

        Ok(addrs)
    }

    /// Update configuration
    pub fn set_config(&mut self, config: DnsConfig) {
        info!("Updating DNS config: {:?}", config.provider.name());
        self.config = config;
    }

    /// Get current provider
    pub fn provider(&self) -> &DnsProvider {
        &self.config.provider
    }

    /// Whether app-managed DoH is configured to traverse a SOCKS proxy.
    pub fn is_tunneled(&self) -> bool {
        self.config.provider.uses_encrypted_transport() && self.config.tunnel_dns
    }
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self::new(DnsConfig::default())
    }
}

#[derive(serde::Deserialize)]
struct DohResponse {
    #[serde(rename = "Answer")]
    answer: Option<Vec<DohAnswer>>,
}

#[derive(serde::Deserialize)]
struct DohAnswer {
    #[serde(rename = "data")]
    data: String,
}

fn parse_doh_response(body: &str) -> Vec<IpAddr> {
    let parsed: std::result::Result<DohResponse, serde_json::Error> = serde_json::from_str(body);
    let Ok(response) = parsed else {
        return Vec::new();
    };

    response
        .answer
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| entry.data.parse::<IpAddr>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_provider_urls() {
        assert_eq!(
            DnsProvider::CloudflareDoH.doh_url().unwrap(),
            "https://cloudflare-dns.com/dns-query"
        );
        assert_eq!(
            DnsProvider::Quad9DoH.doh_url().unwrap(),
            "https://dns.quad9.net/dns-query"
        );
    }

    #[test]
    fn test_dns_transport_description() {
        assert!(DnsProvider::CloudflareDoH.uses_encrypted_transport());
        assert!(DnsProvider::Quad9DoH.uses_encrypted_transport());
        assert!(!DnsProvider::System.uses_encrypted_transport());
    }

    #[test]
    fn test_dns_config_default() {
        let config = DnsConfig::default();
        assert_eq!(config.provider, DnsProvider::System);
        assert!(config.tunnel_dns);
    }

    #[tokio::test]
    async fn test_dns_resolver_creation() {
        let resolver = DnsResolver::new(DnsConfig::default());
        assert_eq!(resolver.provider().name(), "System");
        assert!(!resolver.is_tunneled());
    }
}
