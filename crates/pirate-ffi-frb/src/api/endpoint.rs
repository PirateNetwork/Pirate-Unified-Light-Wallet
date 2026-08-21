use super::*;
use pirate_sync_lightd::client::{LightClientConfig, RetryConfig, TlsConfig, TransportMode};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

pub const DEFAULT_LIGHTD_HOST: &str = service::DEFAULT_LIGHTD_HOST;
pub const DEFAULT_LIGHTD_PORT: u16 = service::DEFAULT_LIGHTD_PORT;
pub const DEFAULT_LIGHTD_USE_TLS: bool = service::DEFAULT_LIGHTD_USE_TLS;

const IP_TLS_SERVER_NAME: &str = "lightd1.piratechain.com";
const DEV_LIGHTD_HOST: &str = "64.23.167.130";
const DEV_LIGHTD_PORT: u16 = 9067;
const IRONWOOD_TESTNET_PORT: u16 = 8067;
const MAINNET_LIGHTD_HOSTS: &[&str] = &[
    "lightd1.pirate.black",
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

lazy_static::lazy_static! {
    static ref LIGHTD_ENDPOINTS: Arc<RwLock<HashMap<WalletId, LightdEndpoint>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LightdEndpoint {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub tls_pin: Option<String>,
    pub label: Option<String>,
    pub automatic_failover: bool,
    pub failover_endpoints: Vec<String>,
    pub is_configured: bool,
}

impl Default for LightdEndpoint {
    fn default() -> Self {
        Self {
            host: DEFAULT_LIGHTD_HOST.to_string(),
            port: DEFAULT_LIGHTD_PORT,
            use_tls: DEFAULT_LIGHTD_USE_TLS,
            tls_pin: None,
            label: None,
            automatic_failover: false,
            failover_endpoints: Vec::new(),
            is_configured: false,
        }
    }
}

impl LightdEndpoint {
    pub fn url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }

    pub fn display_string(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

pub(super) fn cache_lightd_endpoint(wallet_id: WalletId, endpoint: LightdEndpoint) {
    LIGHTD_ENDPOINTS.write().insert(wallet_id, endpoint);
}

pub(super) fn remove_cached_lightd_endpoint(wallet_id: &WalletId) {
    LIGHTD_ENDPOINTS.write().remove(wallet_id);
}

pub(super) fn load_registry_endpoints(db: &Database, wallets: &[WalletMeta]) -> Result<()> {
    let mut endpoints = LIGHTD_ENDPOINTS.write();
    endpoints.clear();

    for wallet in wallets {
        let endpoint_key = format!("lightd_endpoint_{}", wallet.id);
        let pin_key = format!("lightd_tls_pin_{}", wallet.id);
        let endpoint_url = get_registry_setting(db, &endpoint_key)?;
        let tls_pin = get_registry_setting(db, &pin_key)?;

        if let Some(url) = endpoint_url {
            let endpoint = endpoint_from_url(&url, DEFAULT_LIGHTD_USE_TLS, tls_pin, None)?;
            endpoints.insert(wallet.id.clone(), endpoint);
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

pub(super) fn endpoint_from_url(
    url: &str,
    default_use_tls: bool,
    tls_pin: Option<String>,
    label: Option<String>,
) -> Result<LightdEndpoint> {
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
    let port = if parts.len() == 2 {
        parts[1]
            .parse::<u16>()
            .map_err(|_| anyhow!("Invalid port number"))?
    } else if use_tls {
        443
    } else {
        DEV_LIGHTD_PORT
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

pub(super) fn build_light_client_config(
    endpoint: &LightdEndpoint,
    transport: TransportMode,
    socks5_url: Option<String>,
    allow_direct_fallback: bool,
    retry: RetryConfig,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> LightClientConfig {
    LightClientConfig {
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
    }
}

pub(super) fn detect_network_from_endpoint(host: &str, port: u16) -> Option<NetworkType> {
    let host_lower = host.to_ascii_lowercase();
    if IRONWOOD_TESTNET_LIGHTD_HOSTS.contains(&host_lower.as_str()) {
        return Some(NetworkType::Testnet);
    }
    if port == 8067 {
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
        || (host == DEFAULT_LIGHTD_HOST && port == DEFAULT_LIGHTD_PORT)
        || (host == DEV_LIGHTD_HOST && port == DEV_LIGHTD_PORT)
    {
        return Some(NetworkType::Mainnet);
    }
    None
}

pub(super) fn address_prefix_network_type_for_endpoint(
    endpoint: &LightdEndpoint,
    default_network: NetworkType,
) -> NetworkType {
    let host = endpoint.host.to_ascii_lowercase();
    if (endpoint.host == DEV_LIGHTD_HOST && endpoint.port == IRONWOOD_TESTNET_PORT)
        || IRONWOOD_TESTNET_LIGHTD_HOSTS.contains(&host.as_str())
    {
        NetworkType::Mainnet
    } else {
        default_network
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn default_endpoint_remains_single_source() {
        let endpoint = LightdEndpoint::default();
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
    fn custom_tls_ip_uses_its_own_server_name() {
        let endpoint = LightdEndpoint {
            host: "192.0.2.10".to_string(),
            port: 443,
            use_tls: true,
            tls_pin: None,
            label: None,
            ..LightdEndpoint::default()
        };
        assert_eq!(tls_server_name(&endpoint).as_deref(), Some("192.0.2.10"));
    }
}
