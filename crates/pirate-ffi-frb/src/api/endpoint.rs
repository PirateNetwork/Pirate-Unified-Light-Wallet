use super::*;
use pirate_sync_lightd::client::{LightClientConfig, RetryConfig, TlsConfig, TransportMode};
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;

pub const DEFAULT_LIGHTD_HOST: &str = service::DEFAULT_LIGHTD_HOST;
pub const DEFAULT_LIGHTD_PORT: u16 = service::DEFAULT_LIGHTD_PORT;
pub const DEFAULT_LIGHTD_USE_TLS: bool = service::DEFAULT_LIGHTD_USE_TLS;

const IP_TLS_SERVER_NAME: &str = "lightd1.piratechain.com";

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
}

impl Default for LightdEndpoint {
    fn default() -> Self {
        Self {
            host: DEFAULT_LIGHTD_HOST.to_string(),
            port: DEFAULT_LIGHTD_PORT,
            use_tls: DEFAULT_LIGHTD_USE_TLS,
            tls_pin: None,
            label: None,
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
    } else {
        DEFAULT_LIGHTD_PORT
    };

    Ok(LightdEndpoint {
        host,
        port,
        use_tls,
        tls_pin,
        label,
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
    }
}

pub(super) fn detect_network_from_endpoint(host: &str, port: u16) -> Option<NetworkType> {
    let host_lower = host.to_ascii_lowercase();
    if port == 8067 {
        return Some(NetworkType::Testnet);
    }
    if host_lower.contains("regtest") {
        return Some(NetworkType::Regtest);
    }
    if host_lower.contains("testnet") {
        return Some(NetworkType::Testnet);
    }
    if host_lower.contains("piratechain.com")
        || (host == DEFAULT_LIGHTD_HOST && port == DEFAULT_LIGHTD_PORT)
    {
        return Some(NetworkType::Mainnet);
    }
    None
}

pub(super) fn orchard_activation_override_height(endpoint: &LightdEndpoint) -> Option<u32> {
    if endpoint.host == DEFAULT_LIGHTD_HOST && endpoint.port == 8067 {
        Some(61)
    } else {
        None
    }
}

pub(super) fn address_prefix_network_type_for_endpoint(
    endpoint: &LightdEndpoint,
    default_network: NetworkType,
) -> NetworkType {
    if endpoint.host == DEFAULT_LIGHTD_HOST && endpoint.port == 8067 {
        NetworkType::Mainnet
    } else {
        default_network
    }
}

pub(super) fn tls_server_name(endpoint: &LightdEndpoint) -> Option<String> {
    if !endpoint.use_tls {
        return None;
    }
    if endpoint.host.parse::<IpAddr>().is_ok() {
        return Some(IP_TLS_SERVER_NAME.to_string());
    }
    Some(endpoint.host.clone())
}
