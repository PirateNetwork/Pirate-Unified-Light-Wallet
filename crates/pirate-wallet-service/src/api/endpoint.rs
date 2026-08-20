use super::*;
use pirate_sync_lightd::client::{
    LightClientConfig, LightClientEndpoint, RetryConfig, TlsConfig, TransportMode,
};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

const IP_TLS_SERVER_NAME: &str = "lightd1.piratechain.com";
const CUSTOM_ENDPOINT_LABEL: &str = "Custom";
const OFFICIAL_ENDPOINT_LABEL: &str = "Pirate Chain Mainnet";
const DEV_LIGHTD_HOST: &str = "64.23.167.130";
const DEV_LIGHTD_PORT: u16 = 9067;
const IRONWOOD_TESTNET_PORT: u16 = 8067;
const MAX_FAILOVER_ENDPOINTS: usize = 16;
const MAINNET_LIGHTD_HOSTS: &[&str] = &[
    "lightd1.pirate.black",
    "lightd.pirate.black",
    "lightd1.piratechain.com",
    "pirate.mathnodes.com",
    "arrr.qortal.link",
    "arrr2.qortal.link",
    "arrr3.qortal.link",
    "lightwalletd1.cryptoforge.cc",
    "lightwalletd2.cryptoforge.cc",
    "4kbfoltkqir44ab62l6dhkovugdrdevxzjtp6duv6gga3ixoe6kwkcqd.onion",
    "ibdhmxvqg3imgf67el6y2zxakuf37h3dyug4ujpa6qb7zvrz7sacmnqd.onion",
    "5vjlbxmzx4gjfuwcot2qtfjdnxodzpe4jsw3ckx7i4maltz7j5qa.b32.i2p",
    "47go5e2vfmm2o5qdl7zr7rzf57hxjt6z4453ugvgyfkl3bbobwmq.b32.i2p",
];
const IRONWOOD_TESTNET_LIGHTD_HOSTS: &[&str] = &[
    "testlightwalletd1.cryptoforge.cc",
    "testlightwalletd2.cryptoforge.cc",
    "6rwymqddf6dxaphftoy5n3wfgpgwut2upf2lnk6shimjkum2z6uq.b32.i2p",
    "g4vk6mdenflhm5j2c4kiujwkox7ygyftdfhwai6clgye4br2ujlq.b32.i2p",
    "lzciy5lpujcqz42vtbr523ceik6rkzlvwtknxfnpyxcskpmx3swkfryd.onion",
    "iwfhfhwyg6gfm3mqpe5clnwi5oh652hsd2aq4hiael7m7syl4nkyxiqd.onion",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EndpointRoute {
    Clearnet,
    Tor,
    I2p,
}

/// Default official Pirate Chain mainnet endpoint.
pub const DEFAULT_LIGHTD_HOST: &str = pirate_sync_lightd::client::DEFAULT_LIGHTD_HOST;
pub const DEFAULT_LIGHTD_PORT: u16 = pirate_sync_lightd::client::DEFAULT_LIGHTD_PORT;
pub const DEFAULT_LIGHTD_USE_TLS: bool = pirate_sync_lightd::client::DEFAULT_LIGHTD_USE_TLS;
pub const DEFAULT_LIGHTD_SPKI_PIN: &str = pirate_sync_lightd::client::DEFAULT_LIGHTD_SPKI_PIN;

lazy_static::lazy_static! {
    /// Persisted endpoint per wallet (in production, stored encrypted)
    static ref LIGHTD_ENDPOINTS: Arc<RwLock<HashMap<WalletId, LightdEndpoint>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

/// Lightwalletd endpoint configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LightdEndpoint {
    /// Server host
    pub host: String,
    /// Server port
    pub port: u16,
    /// Whether TLS is enabled
    pub use_tls: bool,
    /// Optional TLS certificate pin (SPKI hash, base64)
    pub tls_pin: Option<String>,
    /// User label
    pub label: Option<String>,
    /// Whether this endpoint uses an explicitly configured failover pool.
    #[serde(default)]
    pub automatic_failover: bool,
    /// Same-network endpoints eligible for validated failover and historical striping.
    #[serde(default)]
    pub failover_endpoints: Vec<String>,
    /// Whether the caller explicitly selected an endpoint mode.
    #[serde(default)]
    pub is_configured: bool,
}

impl Default for LightdEndpoint {
    fn default() -> Self {
        Self {
            host: DEFAULT_LIGHTD_HOST.to_string(),
            port: DEFAULT_LIGHTD_PORT,
            use_tls: DEFAULT_LIGHTD_USE_TLS,
            tls_pin: if DEFAULT_LIGHTD_USE_TLS {
                match DEFAULT_LIGHTD_SPKI_PIN {
                    "" => None,
                    pin => Some(pin.to_string()),
                }
            } else {
                None
            },
            label: Some(OFFICIAL_ENDPOINT_LABEL.to_string()),
            automatic_failover: false,
            failover_endpoints: Vec::new(),
            is_configured: false,
        }
    }
}

impl LightdEndpoint {
    /// Full URL for gRPC connection
    pub fn url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }

    /// Display string (host:port)
    pub fn display_string(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

fn normalize_endpoint_url(url: &str, default_use_tls: bool) -> Result<(String, u16, bool)> {
    let mut normalized = url.trim().to_string();
    let mut use_tls = default_use_tls;

    if normalized.starts_with("https://") {
        normalized = normalized[8..].to_string();
        use_tls = true;
    } else if normalized.starts_with("http://") {
        normalized = normalized[7..].to_string();
        use_tls = false;
    }

    if normalized.ends_with('/') {
        normalized.pop();
    }

    let parts: Vec<&str> = normalized.split(':').collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(anyhow!("Invalid endpoint URL format"));
    }

    let host = parts[0].to_string();
    if host.is_empty() {
        return Err(anyhow!("Empty host"));
    }

    let port = if parts.len() == 2 {
        parts[1]
            .parse::<u16>()
            .map_err(|_| anyhow!("Invalid port number"))?
    } else if use_tls {
        443
    } else {
        DEV_LIGHTD_PORT
    };

    Ok((host, port, use_tls))
}

pub(super) fn endpoint_from_url(
    url: &str,
    default_use_tls: bool,
    tls_pin: Option<String>,
    label: Option<String>,
) -> Result<LightdEndpoint> {
    let (host, port, use_tls) = if default_use_tls == DEFAULT_LIGHTD_USE_TLS {
        parse_endpoint_url(url)?
    } else {
        normalize_endpoint_url(url, default_use_tls)?
    };
    Ok(LightdEndpoint {
        host,
        port,
        use_tls,
        tls_pin,
        label,
        automatic_failover: false,
        failover_endpoints: Vec::new(),
        is_configured: false,
    })
}

/// Parse endpoint URL into components
pub(super) fn parse_endpoint_url(url: &str) -> Result<(String, u16, bool)> {
    normalize_endpoint_url(url, DEFAULT_LIGHTD_USE_TLS)
}

pub(super) fn cache_lightd_endpoint(wallet_id: WalletId, endpoint: LightdEndpoint) {
    LIGHTD_ENDPOINTS.write().insert(wallet_id, endpoint);
}

pub(super) fn remove_cached_lightd_endpoint(wallet_id: &WalletId) {
    LIGHTD_ENDPOINTS.write().remove(wallet_id);
}

pub(super) fn clear_cached_lightd_endpoints() {
    LIGHTD_ENDPOINTS.write().clear();
}

pub(super) fn load_registry_endpoints(db: &Database, wallets: &[WalletMeta]) -> Result<()> {
    let mut endpoints = LIGHTD_ENDPOINTS.write();
    endpoints.clear();

    for wallet in wallets {
        let endpoint_key = format!("lightd_endpoint_{}", wallet.id);
        let pin_key = format!("lightd_tls_pin_{}", wallet.id);
        let failover_key = format!("lightd_failover_endpoints_{}", wallet.id);
        let endpoint_url = get_registry_setting(db, &endpoint_key)?;
        let tls_pin = get_registry_setting(db, &pin_key)?;
        let stored_failover = get_registry_setting(db, &failover_key)?;

        if let Some(url) = endpoint_url {
            match endpoint_from_url(
                &url,
                DEFAULT_LIGHTD_USE_TLS,
                tls_pin,
                Some(CUSTOM_ENDPOINT_LABEL.to_string()),
            ) {
                Ok(mut endpoint) => {
                    endpoint.is_configured = true;
                    if let Some(raw) = stored_failover {
                        match serde_json::from_str::<Vec<String>>(&raw)
                            .map_err(anyhow::Error::from)
                            .and_then(|candidates| {
                                normalize_failover_endpoints(&endpoint, candidates)
                            }) {
                            Ok(candidates) => {
                                endpoint.automatic_failover = !candidates.is_empty();
                                endpoint.failover_endpoints = candidates;
                            }
                            Err(error) => tracing::warn!(
                                wallet_id = %wallet.id,
                                %error,
                                "Ignoring invalid persisted lightwalletd failover pool"
                            ),
                        }
                    }
                    endpoints.insert(wallet.id.clone(), endpoint);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse stored endpoint for wallet {}: {}",
                        wallet.id,
                        e
                    );
                }
            }
        }
    }

    Ok(())
}

pub(super) fn get_lightd_endpoint(wallet_id: WalletId) -> Result<String> {
    let endpoints = LIGHTD_ENDPOINTS.read();
    let endpoint = endpoints.get(&wallet_id).cloned().unwrap_or_default();
    Ok(endpoint.url())
}

pub(super) fn get_lightd_endpoint_config(wallet_id: WalletId) -> Result<LightdEndpoint> {
    let endpoints = LIGHTD_ENDPOINTS.read();
    Ok(endpoints.get(&wallet_id).cloned().unwrap_or_default())
}

/// Detect network type from endpoint URL
///
/// Detects network based on hostname and port:
/// - `lightd1.pirate.black:443` -> Mainnet (official endpoint)
/// - `64.23.167.130:9067` -> Mainnet (developer endpoint)
/// - `64.23.167.130:8067` -> Testnet
pub(super) fn detect_network_from_endpoint(host: &str, port: u16) -> Option<NetworkType> {
    let host_lower = host.to_ascii_lowercase();

    if IRONWOOD_TESTNET_LIGHTD_HOSTS.contains(&host_lower.as_str()) {
        return Some(NetworkType::Testnet);
    }

    if host == DEFAULT_LIGHTD_HOST && port == DEFAULT_LIGHTD_PORT {
        return Some(NetworkType::Mainnet);
    }

    if host == DEV_LIGHTD_HOST && port == DEV_LIGHTD_PORT {
        return Some(NetworkType::Mainnet);
    }

    if port == IRONWOOD_TESTNET_PORT {
        return Some(NetworkType::Testnet);
    }

    if host_lower.contains("regtest") {
        return Some(NetworkType::Regtest);
    }
    if host_lower.contains("testnet") {
        return Some(NetworkType::Testnet);
    }

    if MAINNET_LIGHTD_HOSTS.contains(&host_lower.as_str())
        || host_lower.contains("piratechain.com")
        || host_lower.contains("pirate.black")
    {
        return Some(NetworkType::Mainnet);
    }

    None
}

pub(super) fn address_prefix_network_type_for_endpoint(
    endpoint: &LightdEndpoint,
    default_network: NetworkType,
) -> NetworkType {
    if is_ironwood_testnet_endpoint(endpoint) {
        return NetworkType::Mainnet;
    }
    default_network
}

fn is_ironwood_testnet_endpoint(endpoint: &LightdEndpoint) -> bool {
    let host = endpoint.host.to_ascii_lowercase();
    (endpoint.host == DEV_LIGHTD_HOST && endpoint.port == IRONWOOD_TESTNET_PORT)
        || IRONWOOD_TESTNET_LIGHTD_HOSTS.contains(&host.as_str())
}

pub(super) fn normalize_failover_endpoints(
    primary: &LightdEndpoint,
    candidates: Vec<String>,
) -> Result<Vec<String>> {
    if candidates.len() > MAX_FAILOVER_ENDPOINTS {
        return Err(anyhow!(
            "A lightwalletd pool may contain at most {MAX_FAILOVER_ENDPOINTS} alternate endpoints"
        ));
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    if primary
        .tls_pin
        .as_ref()
        .is_some_and(|pin| !pin.trim().is_empty())
    {
        return Err(anyhow!(
            "Automatic failover cannot be combined with a pinned primary endpoint"
        ));
    }

    let primary_network = detect_network_from_endpoint(&primary.host, primary.port)
        .ok_or_else(|| anyhow!("Automatic failover requires a recognized Pirate network"))?;
    let primary_route = endpoint_route(&primary.host);
    let primary_identity = primary.url().to_ascii_lowercase();
    let mut seen = HashSet::from([primary_identity]);
    let mut normalized = Vec::with_capacity(candidates.len());

    for candidate_url in candidates {
        let candidate = endpoint_from_url(&candidate_url, DEFAULT_LIGHTD_USE_TLS, None, None)?;
        let candidate_network = detect_network_from_endpoint(&candidate.host, candidate.port)
            .ok_or_else(|| {
                anyhow!(
                    "Automatic failover endpoint {} has an unknown network",
                    candidate.url()
                )
            })?;
        if candidate_network != primary_network {
            return Err(anyhow!(
                "Automatic failover endpoint {} is on a different network",
                candidate.url()
            ));
        }
        if endpoint_route(&candidate.host) != primary_route {
            return Err(anyhow!(
                "Automatic failover endpoint {} uses a different network route",
                candidate.url()
            ));
        }
        if candidate.use_tls != primary.use_tls {
            return Err(anyhow!(
                "Automatic failover endpoint {} changes the connection security mode",
                candidate.url()
            ));
        }
        let url = candidate.url();
        if seen.insert(url.to_ascii_lowercase()) {
            normalized.push(url);
        }
    }

    Ok(normalized)
}

fn endpoint_route(host: &str) -> EndpointRoute {
    let host = host.to_ascii_lowercase();
    if host.ends_with(".onion") {
        EndpointRoute::Tor
    } else if host.ends_with(".b32.i2p") || host.ends_with(".i2p") {
        EndpointRoute::I2p
    } else {
        EndpointRoute::Clearnet
    }
}

pub(super) fn tls_server_name(endpoint: &LightdEndpoint) -> Option<String> {
    if !endpoint.use_tls {
        return None;
    }
    if endpoint.host == DEV_LIGHTD_HOST && endpoint.host.parse::<IpAddr>().is_ok() {
        return Some(IP_TLS_SERVER_NAME.to_string());
    }
    Some(endpoint.host.clone())
}

pub(super) fn build_light_client_config(
    endpoint: &LightdEndpoint,
    transport: TransportMode,
    socks5_url: Option<String>,
    allow_direct_fallback: bool,
    retry: RetryConfig,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> LightClientConfig {
    let mut config = LightClientConfig {
        endpoint: endpoint.url(),
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: endpoint.use_tls,
            spki_pin: endpoint.tls_pin.clone(),
            server_name: tls_server_name(endpoint),
        },
        retry,
        connect_timeout,
        request_timeout,
        allow_direct_fallback,
        failover_endpoints: Vec::new(),
    };
    if endpoint.automatic_failover && endpoint.tls_pin.is_none() {
        for candidate in &endpoint.failover_endpoints {
            config = config.with_failover_endpoint(LightClientEndpoint::new(candidate));
        }
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tls_endpoint(host: &str) -> LightdEndpoint {
        LightdEndpoint {
            host: host.to_string(),
            port: 443,
            use_tls: true,
            tls_pin: None,
            label: None,
            ..LightdEndpoint::default()
        }
    }

    #[test]
    fn tls_hostname_uses_endpoint_host() {
        let endpoint = tls_endpoint("pirate.mathnodes.com");
        assert_eq!(
            tls_server_name(&endpoint).as_deref(),
            Some("pirate.mathnodes.com")
        );
    }

    #[test]
    fn custom_tls_ip_does_not_use_official_server_name() {
        let endpoint = tls_endpoint("192.0.2.10");
        assert_eq!(tls_server_name(&endpoint).as_deref(), Some("192.0.2.10"));
    }

    #[test]
    fn developer_tls_ip_keeps_its_certificate_server_name() {
        let endpoint = tls_endpoint(DEV_LIGHTD_HOST);
        assert_eq!(
            tls_server_name(&endpoint).as_deref(),
            Some(IP_TLS_SERVER_NAME)
        );
    }

    #[test]
    fn plaintext_endpoint_has_no_tls_server_name() {
        let mut endpoint = tls_endpoint("pirate.mathnodes.com");
        endpoint.use_tls = false;
        assert_eq!(tls_server_name(&endpoint), None);
    }

    #[test]
    fn curated_routes_are_detected_as_mainnet() {
        for host in MAINNET_LIGHTD_HOSTS {
            assert_eq!(
                detect_network_from_endpoint(host, DEFAULT_LIGHTD_PORT),
                Some(NetworkType::Mainnet),
                "{host} should use mainnet key derivation"
            );
        }
        assert_eq!(
            detect_network_from_endpoint(DEV_LIGHTD_HOST, DEV_LIGHTD_PORT),
            Some(NetworkType::Mainnet)
        );
    }

    #[test]
    fn default_endpoint_uses_the_official_tls_server() {
        let endpoint = LightdEndpoint::default();
        assert_eq!(endpoint.url(), "https://lightd1.pirate.black:443");
        assert_eq!(endpoint.label.as_deref(), Some("Pirate Chain Mainnet"));
        assert!(!endpoint.automatic_failover);
        assert!(!endpoint.is_configured);
    }

    #[test]
    fn curated_endpoints_remain_single_source_without_opt_in() {
        for host in [
            DEFAULT_LIGHTD_HOST,
            "lightd.pirate.black",
            "arrr2.qortal.link",
            "lightwalletd2.cryptoforge.cc",
        ] {
            let endpoint = tls_endpoint(host);
            let config = build_light_client_config(
                &endpoint,
                TransportMode::Direct,
                None,
                false,
                RetryConfig::default(),
                Duration::from_secs(30),
                Duration::from_secs(180),
            );
            assert!(config.failover_endpoints.is_empty(), "{host}");
        }
    }

    #[test]
    fn explicit_pool_adds_only_normalized_same_network_alternates() {
        let mut endpoint = LightdEndpoint::default();
        endpoint.is_configured = true;
        endpoint.failover_endpoints = normalize_failover_endpoints(
            &endpoint,
            vec![
                "https://lightwalletd1.cryptoforge.cc:443".to_string(),
                "https://lightwalletd1.cryptoforge.cc:443/".to_string(),
                endpoint.url(),
            ],
        )
        .expect("valid mainnet pool");
        endpoint.automatic_failover = true;

        let config = build_light_client_config(
            &endpoint,
            TransportMode::Direct,
            None,
            false,
            RetryConfig::default(),
            Duration::from_secs(30),
            Duration::from_secs(180),
        );
        assert_eq!(config.failover_endpoints.len(), 1);
        assert_eq!(
            config.failover_endpoints[0].endpoint,
            "https://lightwalletd1.cryptoforge.cc:443"
        );
    }

    #[test]
    fn pool_rejects_pins_and_cross_network_candidates() {
        let pinned = LightdEndpoint {
            tls_pin: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()),
            ..LightdEndpoint::default()
        };
        assert!(normalize_failover_endpoints(
            &pinned,
            vec!["https://lightwalletd1.cryptoforge.cc:443".to_string()]
        )
        .is_err());

        assert!(normalize_failover_endpoints(
            &LightdEndpoint::default(),
            vec!["https://testlightwalletd1.cryptoforge.cc:443".to_string()]
        )
        .is_err());
    }

    #[test]
    fn empty_pool_keeps_custom_endpoints_compatible() {
        let custom = endpoint_from_url(
            "https://wallet.example:443",
            true,
            None,
            Some("Custom".to_string()),
        )
        .expect("custom endpoint");
        assert_eq!(
            normalize_failover_endpoints(&custom, Vec::new()).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn pool_rejects_route_and_security_mode_changes() {
        assert!(normalize_failover_endpoints(
            &LightdEndpoint::default(),
            vec![format!(
                "http://{}:{}",
                MAINNET_LIGHTD_HOSTS[8], DEV_LIGHTD_PORT
            )]
        )
        .is_err());

        assert!(normalize_failover_endpoints(
            &LightdEndpoint::default(),
            vec![format!(
                "http://{}:{}",
                DEFAULT_LIGHTD_HOST, DEFAULT_LIGHTD_PORT
            )]
        )
        .is_err());
    }

    #[test]
    fn pinned_default_endpoint_remains_single_source() {
        let endpoint = LightdEndpoint {
            tls_pin: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string()),
            ..LightdEndpoint::default()
        };
        let config = build_light_client_config(
            &endpoint,
            TransportMode::Direct,
            None,
            false,
            RetryConfig::default(),
            Duration::from_secs(30),
            Duration::from_secs(180),
        );
        assert!(config.failover_endpoints.is_empty());
    }

    #[test]
    fn ironwood_testnet_keeps_mainnet_address_prefixes() {
        let endpoint = LightdEndpoint {
            host: DEV_LIGHTD_HOST.to_string(),
            port: IRONWOOD_TESTNET_PORT,
            use_tls: false,
            tls_pin: None,
            label: None,
            ..LightdEndpoint::default()
        };
        assert_eq!(
            address_prefix_network_type_for_endpoint(&endpoint, NetworkType::Testnet),
            NetworkType::Mainnet
        );

        for host in IRONWOOD_TESTNET_LIGHTD_HOSTS {
            let endpoint = tls_endpoint(host);
            assert_eq!(
                detect_network_from_endpoint(host, endpoint.port),
                Some(NetworkType::Testnet)
            );
            assert_eq!(
                address_prefix_network_type_for_endpoint(&endpoint, NetworkType::Testnet),
                NetworkType::Mainnet
            );
        }
    }
}
