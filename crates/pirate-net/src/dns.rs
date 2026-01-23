//! DNS resolution via DoH or system resolver.
//!
//! Provides privacy-preserving DNS resolution to prevent leaks when possible.

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
    /// System resolver (NOT RECOMMENDED - may leak)
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
            Self::System => "System (Not Private)",
        }
    }

    /// Check if provider is privacy-preserving
    pub fn is_private(&self) -> bool {
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
            provider: DnsProvider::CloudflareDoH,
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

        if !config.provider.is_private() {
            warn!("DNS resolver is using system DNS - privacy may be compromised!");
        }

        Self { config }
    }

    /// Resolve hostname to IP addresses
    pub async fn resolve(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        debug!(
            "Resolving hostname: {} via {}",
            hostname,
            self.config.provider.name()
        );

        match &self.config.provider {
            DnsProvider::System => {
                warn!("Using system DNS for {}: Privacy not guaranteed!", hostname);
                self.resolve_system(hostname).await
            }
            provider if provider.doh_url().is_some() => self.resolve_doh(hostname).await,
            _ => self.resolve_system(hostname).await,
        }
    }

    /// Resolve via DNS-over-HTTPS
    async fn resolve_doh(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        let doh_url = self.config.provider.doh_url().unwrap();

        debug!("DoH resolution: {} via {}", hostname, doh_url);

        // Build HTTP client
        let client = if self.config.tunnel_dns {
            if let Some(ref proxy) = self.config.socks_proxy {
                let proxy_url = if proxy.contains("://") {
                    proxy.clone()
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
            }
        } else {
            reqwest::Client::new()
        };

        let mut addrs = Vec::new();

        for record_type in ["A", "AAAA"] {
            let query_url = format!("{}?name={}&type={}", doh_url, hostname, record_type);
            let response = client
                .get(&query_url)
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
            warn!(
                "DoH returned no IPs for {}, falling back to system resolver",
                hostname
            );
            return self.resolve_system(hostname).await;
        }

        Ok(addrs)
    }

    /// Resolve via system resolver (NOT PRIVATE)
    async fn resolve_system(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        use tokio::net::lookup_host;

        warn!(
            "Using system DNS for {} - this may leak information!",
            hostname
        );
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

    /// Check if DNS is tunneled
    pub fn is_tunneled(&self) -> bool {
        self.config.tunnel_dns
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
    fn test_dns_privacy() {
        assert!(DnsProvider::CloudflareDoH.is_private());
        assert!(DnsProvider::Quad9DoH.is_private());
        assert!(!DnsProvider::System.is_private());
    }

    #[test]
    fn test_dns_config_default() {
        let config = DnsConfig::default();
        assert!(config.provider.is_private());
        assert!(config.tunnel_dns);
    }

    #[tokio::test]
    async fn test_dns_resolver_creation() {
        let resolver = DnsResolver::new(DnsConfig::default());
        assert_eq!(resolver.provider().name(), "Cloudflare (1.1.1.1)");
        assert!(resolver.is_tunneled());
    }
}
