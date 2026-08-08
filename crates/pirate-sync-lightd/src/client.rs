//! Lightwalletd gRPC client with Tor routing and TLS pinning
//!
//! Provides connection to lightwalletd servers with:
//! - Tor routing by default via pirate-net
//! - TLS with optional SPKI certificate pinning
//! - Retry logic with exponential backoff
//! - Compact block streaming

use crate::proto_types as proto;
use crate::{Error, Result};
use once_cell::sync::Lazy;
use percent_encoding::percent_decode_str;
use pirate_net::{
    DnsConfig as NetDnsConfig, I2pConfig as NetI2pConfig, Socks5Config as NetSocks5Config,
    TorBridgeConfig, TorBridgeTransport, TorConfig as NetTorConfig,
    TransportConfig as NetTransportConfig, TransportManager as NetTransportManager,
    TransportMode as NetTransportMode,
};
use rand::Rng;
use std::env;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tracing::{debug, error, info, warn};

use proto::compact_tx_streamer_client::CompactTxStreamerClient;
use proto::{
    BlockId, BlockRange, ChainSpec, Empty, GetSubtreeRootsArg, RawTransaction, ShieldedProtocol,
    SubtreeRoot, TxFilter,
};

/// Default lightwalletd endpoint (known-working mainnet)
pub const DEFAULT_LIGHTD_HOST: &str = "64.23.167.130";
/// Default lightwalletd port
pub const DEFAULT_LIGHTD_PORT: u16 = 9067;
/// Default TLS usage for the default endpoint
pub const DEFAULT_LIGHTD_USE_TLS: bool = false;
/// Default SPKI pin for the official lightwalletd endpoint.
pub const DEFAULT_LIGHTD_SPKI_PIN: &str = "";
/// Default endpoint URL
pub const DEFAULT_LIGHTD_URL: &str = "http://64.23.167.130:9067";

/// Retry configuration for network operations
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

/// Transport mode for network connections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportMode {
    /// Route through Tor (default, most private)
    #[default]
    Tor,
    /// Route through I2P (desktop only)
    I2p,
    /// Route through custom SOCKS5 proxy
    Socks5,
    /// Direct connection (NOT RECOMMENDED - exposes IP)
    Direct,
}

impl TransportMode {
    /// Check if this mode preserves privacy
    pub fn is_private(&self) -> bool {
        !matches!(self, Self::Direct)
    }
}

struct GlobalTransportState {
    manager: RwLock<Option<Arc<NetTransportManager>>>,
}

impl GlobalTransportState {
    async fn get_or_init(&self, config: NetTransportConfig) -> Result<Arc<NetTransportManager>> {
        let config = resolve_transport_config(config);
        let existing = {
            let guard = self.manager.read().await;
            guard.as_ref().map(Arc::clone)
        };
        if let Some(manager) = existing {
            manager.update_config(config).await.map_err(map_net_error)?;
            return Ok(manager);
        }

        let created = Arc::new(
            NetTransportManager::new(config.clone())
                .await
                .map_err(map_net_error)?,
        );
        let existing = {
            let mut guard = self.manager.write().await;
            if let Some(manager) = guard.as_ref() {
                Some(Arc::clone(manager))
            } else {
                *guard = Some(Arc::clone(&created));
                None
            }
        };

        if let Some(manager) = existing {
            manager.update_config(config).await.map_err(map_net_error)?;
            Ok(manager)
        } else {
            Ok(created)
        }
    }

    async fn get(&self) -> Option<Arc<NetTransportManager>> {
        let manager = {
            let guard = self.manager.read().await;
            guard.as_ref().map(Arc::clone)
        };
        manager
    }

    async fn shutdown(&self) {
        let manager = {
            let mut guard = self.manager.write().await;
            let manager = guard.as_ref().map(Arc::clone);
            *guard = None;
            manager
        };
        if let Some(manager) = manager {
            manager.shutdown().await;
        }
    }
}

static GLOBAL_TRANSPORT: Lazy<GlobalTransportState> = Lazy::new(|| GlobalTransportState {
    manager: RwLock::new(None),
});

static DESIRED_TRANSPORT_CONFIG: Lazy<StdRwLock<Option<NetTransportConfig>>> =
    Lazy::new(|| StdRwLock::new(None));

static TOR_CONFIG_OVERRIDE: Lazy<std::sync::RwLock<Option<NetTorConfig>>> =
    Lazy::new(|| std::sync::RwLock::new(None));

fn set_desired_transport_config(config: NetTransportConfig) {
    if let Ok(mut guard) = DESIRED_TRANSPORT_CONFIG.write() {
        *guard = Some(config);
    }
}

fn clear_desired_transport_config() {
    if let Ok(mut guard) = DESIRED_TRANSPORT_CONFIG.write() {
        *guard = None;
    }
}

fn desired_transport_config() -> Option<NetTransportConfig> {
    DESIRED_TRANSPORT_CONFIG
        .read()
        .ok()
        .and_then(|guard| (*guard).clone())
}

fn resolve_transport_config(requested: NetTransportConfig) -> NetTransportConfig {
    if let Some(desired) = desired_transport_config() {
        if requested.mode != desired.mode || requested.socks5 != desired.socks5 {
            debug!(
                "Overriding stale transport request mode={:?} with desired mode={:?}",
                requested.mode, desired.mode
            );
        }
        desired
    } else {
        requested
    }
}

/// Override the embedded Tor configuration for this process.
pub fn set_tor_config_override(config: NetTorConfig) {
    if let Ok(mut guard) = TOR_CONFIG_OVERRIDE.write() {
        *guard = Some(config);
    }
}

/// Clear any previously configured Tor override.
pub fn clear_tor_config_override() {
    if let Ok(mut guard) = TOR_CONFIG_OVERRIDE.write() {
        *guard = None;
    }
}

/// TLS configuration for gRPC connection
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Enable TLS (default: true)
    pub enabled: bool,
    /// Optional SPKI SHA256 pin (base64, 44 chars) for certificate pinning
    pub spki_pin: Option<String>,
    /// Server name for TLS verification (uses endpoint host if None)
    pub server_name: Option<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: DEFAULT_LIGHTD_USE_TLS,
            spki_pin: None,
            server_name: None,
        }
    }
}

/// One explicitly configured failover endpoint and its TLS identity.
#[derive(Debug, Clone)]
pub struct LightClientEndpoint {
    /// Full HTTP(S) endpoint URL.
    pub endpoint: String,
    /// TLS, server-name, and SPKI pin configuration for this endpoint.
    pub tls: TlsConfig,
}

impl LightClientEndpoint {
    /// Create an endpoint with TLS inferred from its URL.
    pub fn new(endpoint: impl Into<String>) -> Self {
        let endpoint = endpoint.into();
        let tls_enabled = LightClientConfig::infer_tls_enabled(&endpoint);
        Self {
            endpoint,
            tls: TlsConfig {
                enabled: tls_enabled,
                ..TlsConfig::default()
            },
        }
    }

    /// Attach an SPKI pin to this endpoint.
    pub fn with_spki_pin(mut self, pin: impl Into<String>) -> Self {
        self.tls.enabled = true;
        self.tls.spki_pin = Some(normalize_spki_pin(&pin.into()).to_string());
        self
    }
}

/// Result of a transport-preserving lightwalletd health probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointHealth {
    /// Endpoint URL that was probed.
    pub endpoint: String,
    /// Whether the endpoint passed connectivity and same-chain checks.
    pub healthy: bool,
    /// Latest reported block height when available.
    pub tip_height: Option<u64>,
    /// Diagnostic reason when the endpoint is unavailable or rejected.
    pub reason: Option<String>,
}

/// Client configuration
#[derive(Debug, Clone)]
pub struct LightClientConfig {
    /// Endpoint URL (e.g., "http://64.23.167.130:9067")
    pub endpoint: String,
    /// Transport mode (Tor, I2P, SOCKS5, or Direct)
    pub transport: TransportMode,
    /// SOCKS5 proxy URL (required if transport is Socks5)
    pub socks5_url: Option<String>,
    /// TLS configuration
    pub tls: TlsConfig,
    /// Retry configuration
    pub retry: RetryConfig,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Request timeout
    pub request_timeout: Duration,
    /// Legacy flag kept for compatibility (direct fallback is disabled).
    pub allow_direct_fallback: bool,
    /// Explicit same-network endpoints eligible for bounded failover.
    ///
    /// Each endpoint retains its own TLS server name and SPKI pin. The selected
    /// Tor/I2P/SOCKS5/direct transport is inherited from this configuration.
    pub failover_endpoints: Vec<LightClientEndpoint>,
}

impl Default for LightClientConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_LIGHTD_URL.to_string(),
            transport: TransportMode::Tor,
            socks5_url: None,
            tls: TlsConfig {
                enabled: DEFAULT_LIGHTD_USE_TLS,
                spki_pin: if DEFAULT_LIGHTD_USE_TLS {
                    match DEFAULT_LIGHTD_SPKI_PIN {
                        "" => None,
                        pin => Some(pin.to_string()),
                    }
                } else {
                    None
                },
                server_name: if DEFAULT_LIGHTD_USE_TLS {
                    Some(DEFAULT_LIGHTD_HOST.to_string())
                } else {
                    None
                },
            },
            retry: RetryConfig::default(),
            connect_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(180),
            allow_direct_fallback: false,
            failover_endpoints: Vec::new(),
        }
    }
}

fn compact_block_range_timeouts(
    transport: TransportMode,
    range_blocks: u64,
    default_request_timeout: Duration,
) -> (Duration, Duration, Duration) {
    let large_range = range_blocks > 256;
    let (first_msg_timeout, next_msg_timeout, per_block_ms) = match (transport, large_range) {
        (TransportMode::Direct, false) => (Duration::from_secs(30), Duration::from_secs(20), 150),
        (TransportMode::Direct, true) => (Duration::from_secs(60), Duration::from_secs(30), 250),
        (_, false) => (Duration::from_secs(60), Duration::from_secs(30), 300),
        (_, true) => (Duration::from_secs(120), Duration::from_secs(60), 750),
    };
    let open_timeout = first_msg_timeout.saturating_add(Duration::from_secs(10));
    let streaming_budget = Duration::from_secs(60).saturating_add(Duration::from_millis(
        range_blocks.saturating_mul(per_block_ms),
    ));
    let request_timeout = default_request_timeout
        .max(open_timeout)
        .max(streaming_budget);

    (first_msg_timeout, next_msg_timeout, request_timeout)
}

impl LightClientConfig {
    fn infer_tls_enabled(endpoint: &str) -> bool {
        let normalized = endpoint.trim_start();
        if normalized.starts_with("https://") {
            return true;
        }
        if normalized.starts_with("http://") {
            return false;
        }
        DEFAULT_LIGHTD_USE_TLS
    }

    /// Create config for direct connection (NOT RECOMMENDED)
    pub fn direct(endpoint: &str) -> Self {
        let tls_enabled = Self::infer_tls_enabled(endpoint);
        Self {
            endpoint: endpoint.to_string(),
            transport: TransportMode::Direct,
            tls: TlsConfig {
                enabled: tls_enabled,
                ..TlsConfig::default()
            },
            ..Default::default()
        }
    }

    /// Create config with SOCKS5 proxy
    pub fn with_socks5(endpoint: &str, socks5_url: &str) -> Self {
        let tls_enabled = Self::infer_tls_enabled(endpoint);
        Self {
            endpoint: endpoint.to_string(),
            transport: TransportMode::Socks5,
            socks5_url: Some(socks5_url.to_string()),
            tls: TlsConfig {
                enabled: tls_enabled,
                ..TlsConfig::default()
            },
            ..Default::default()
        }
    }

    /// Set SPKI pin for certificate verification
    pub fn with_spki_pin(mut self, pin: &str) -> Self {
        self.tls.spki_pin = Some(normalize_spki_pin(pin).to_string());
        self.tls.enabled = true;
        self
    }

    /// Add a same-network endpoint eligible for health-checked failover.
    pub fn with_failover_endpoint(mut self, endpoint: LightClientEndpoint) -> Self {
        self.failover_endpoints.push(endpoint);
        self
    }
}

fn map_net_error(err: pirate_net::Error) -> Error {
    Error::Network(err.to_string())
}

fn build_transport_config(config: &LightClientConfig) -> Result<NetTransportConfig> {
    build_transport_config_from_mode(config.transport, config.socks5_url.as_deref())
}

fn build_transport_config_from_mode(
    mode: TransportMode,
    socks5_url: Option<&str>,
) -> Result<NetTransportConfig> {
    let net_mode = match mode {
        TransportMode::Tor => NetTransportMode::Tor,
        TransportMode::I2p => NetTransportMode::I2p,
        TransportMode::Socks5 => NetTransportMode::Socks5,
        TransportMode::Direct => NetTransportMode::Direct,
    };

    let socks5 = if net_mode == NetTransportMode::Socks5 {
        let url = socks5_url.ok_or_else(|| {
            Error::Connection("SOCKS5 URL required for SOCKS5 transport".to_string())
        })?;
        Some(parse_socks5_url(url)?)
    } else {
        None
    };

    let mut tor = tor_config_from_env();
    tor.enabled = net_mode == NetTransportMode::Tor;

    let mut i2p = i2p_config_from_env();
    i2p.enabled = net_mode == NetTransportMode::I2p;

    let mut dns_config = NetDnsConfig::default();
    match net_mode {
        NetTransportMode::Socks5 => {
            if let Some(ref proxy) = socks5 {
                dns_config.tunnel_dns = true;
                dns_config.socks_proxy = Some(proxy.proxy_url());
            }
        }
        NetTransportMode::I2p => {
            dns_config.tunnel_dns = true;
            dns_config.socks_proxy = Some(format!("socks5h://{}:{}", i2p.address, i2p.socks_port));
        }
        NetTransportMode::Direct => {
            dns_config.tunnel_dns = false;
            dns_config.socks_proxy = None;
        }
        NetTransportMode::Tor => {
            dns_config.tunnel_dns = false;
            dns_config.socks_proxy = None;
        }
    }

    Ok(NetTransportConfig {
        mode: net_mode,
        tor,
        i2p,
        socks5,
        dns_config,
    })
}

fn parse_socks5_url(url: &str) -> Result<NetSocks5Config> {
    let trimmed = url.trim();
    let uri: http::Uri = trimmed
        .parse()
        .map_err(|e| Error::Connection(format!("Invalid SOCKS5 URL '{}': {}", trimmed, e)))?;
    if let Some(scheme) = uri.scheme_str() {
        let scheme = scheme.to_lowercase();
        if scheme != "socks5" && scheme != "socks5h" {
            return Err(Error::Connection(format!(
                "Unsupported SOCKS5 URL scheme '{}'",
                scheme
            )));
        }
    }
    let host = uri
        .host()
        .ok_or_else(|| Error::Connection("SOCKS5 URL missing host".to_string()))?
        .to_string();
    let port = uri.port_u16().unwrap_or(1080);

    let mut username = None;
    let mut password = None;
    if let Some(authority) = uri.authority() {
        if let Some((userinfo, _)) = authority.as_str().rsplit_once('@') {
            if let Some((user, pass)) = userinfo.split_once(':') {
                if !user.is_empty() {
                    username = Some(decode_socks5_userinfo_component(user)?);
                }
                if !pass.is_empty() {
                    password = Some(decode_socks5_userinfo_component(pass)?);
                }
            } else if !userinfo.is_empty() {
                username = Some(decode_socks5_userinfo_component(userinfo)?);
            }
        }
    }

    Ok(NetSocks5Config {
        host,
        port,
        username,
        password,
    })
}

fn decode_socks5_userinfo_component(value: &str) -> Result<String> {
    percent_decode_str(value)
        .decode_utf8()
        .map(|decoded| decoded.into_owned())
        .map_err(|e| Error::Connection(format!("Invalid SOCKS5 credentials encoding: {}", e)))
}

fn tor_config_from_env_raw() -> NetTorConfig {
    let mut config = NetTorConfig::default();

    if let Ok(value) = env::var("PIRATE_TOR_STATE_DIR") {
        if !value.trim().is_empty() {
            config.state_dir = PathBuf::from(value);
        }
    }
    if let Ok(value) = env::var("PIRATE_TOR_CACHE_DIR") {
        if !value.trim().is_empty() {
            config.cache_dir = PathBuf::from(value);
        }
    }
    if let Ok(value) = env::var("PIRATE_TOR_BOOTSTRAP_TIMEOUT_SECS") {
        if let Ok(secs) = value.trim().parse::<u64>() {
            config.bootstrap_timeout = Duration::from_secs(secs.max(1));
        }
    }
    if let Ok(value) = env::var("PIRATE_TOR_CONNECT_TIMEOUT_SECS") {
        if let Ok(secs) = value.trim().parse::<u64>() {
            config.connect_timeout = Duration::from_secs(secs.max(1));
        }
    }
    if let Ok(value) = env::var("PIRATE_TOR_DEBUG") {
        config.debug = parse_bool_env(&value);
    }
    if let Ok(value) = env::var("PIRATE_TOR_USE_BRIDGES") {
        config.use_bridges = parse_bool_env(&value);
    }
    if let Ok(value) = env::var("PIRATE_TOR_FALLBACK_BRIDGES") {
        config.fallback_to_bridges = parse_bool_env(&value);
    }

    let bridge_lines = env::var("PIRATE_TOR_BRIDGE_LINES")
        .ok()
        .as_deref()
        .map(split_list_env)
        .unwrap_or_default();

    if !bridge_lines.is_empty() {
        let transport = match env::var("PIRATE_TOR_BRIDGE_TRANSPORT")
            .unwrap_or_else(|_| "obfs4".to_string())
            .to_lowercase()
            .as_str()
        {
            "snowflake" => TorBridgeTransport::Snowflake,
            "obfs4" => TorBridgeTransport::Obfs4,
            custom => TorBridgeTransport::Custom(custom.to_string()),
        };

        let transport_path = env::var("PIRATE_TOR_BRIDGE_PATH").ok().and_then(|path| {
            if path.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(path))
            }
        });

        config.bridges = Some(TorBridgeConfig {
            transport,
            bridge_lines,
            transport_path,
        });
    }

    config
}

fn tor_config_from_env() -> NetTorConfig {
    if let Ok(guard) = TOR_CONFIG_OVERRIDE.read() {
        if let Some(config) = guard.clone() {
            return config;
        }
    }
    tor_config_from_env_raw()
}

/// Update bridge configuration for the embedded Tor client.
pub fn set_tor_bridge_settings(
    use_bridges: bool,
    fallback_to_bridges: bool,
    transport: String,
    bridge_lines: Vec<String>,
    transport_path: Option<String>,
) -> Result<()> {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        let mut config = tor_config_from_env_raw();
        config.use_bridges = false;
        config.fallback_to_bridges = false;
        config.bridges = None;
        set_tor_config_override(config);
        return Ok(());
    }

    let mut config = tor_config_from_env_raw();
    let normalized_transport = transport.trim().to_lowercase();

    let mut bridge_lines = normalize_bridge_lines_input(bridge_lines);
    if (use_bridges || fallback_to_bridges)
        && bridge_lines.is_empty()
        && normalized_transport == "snowflake"
    {
        bridge_lines = bundled_snowflake_bridges();
    }

    if use_bridges || fallback_to_bridges {
        if bridge_lines.is_empty() {
            config.use_bridges = false;
            config.fallback_to_bridges = false;
            config.bridges = None;
        } else {
            let transport = match normalized_transport.as_str() {
                "obfs4" => TorBridgeTransport::Obfs4,
                "snowflake" => TorBridgeTransport::Snowflake,
                "" => TorBridgeTransport::Snowflake,
                custom => TorBridgeTransport::Custom(custom.to_string()),
            };
            let path = transport_path.as_ref().and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(trimmed))
                }
            });

            config.use_bridges = use_bridges;
            config.fallback_to_bridges = fallback_to_bridges;
            config.bridges = Some(TorBridgeConfig {
                transport,
                bridge_lines,
                transport_path: path,
            });
        }
    } else {
        config.use_bridges = false;
        config.fallback_to_bridges = false;
        config.bridges = None;
    }

    set_tor_config_override(config);
    Ok(())
}

fn i2p_config_from_env() -> NetI2pConfig {
    let mut config = NetI2pConfig::default();

    if let Ok(value) = env::var("PIRATE_I2P_BINARY") {
        if !value.trim().is_empty() {
            config.binary_path = Some(PathBuf::from(value));
        }
    }
    if let Ok(value) = env::var("PIRATE_I2P_DATA_DIR") {
        if !value.trim().is_empty() {
            config.data_dir = Some(PathBuf::from(value));
        }
    }
    if let Ok(value) = env::var("PIRATE_I2P_ADDRESS") {
        if !value.trim().is_empty() {
            config.address = value;
        }
    }
    if let Ok(value) = env::var("PIRATE_I2P_SOCKS_PORT") {
        if let Ok(port) = value.trim().parse::<u16>() {
            config.socks_port = port;
        }
    }
    if let Ok(value) = env::var("PIRATE_I2P_EPHEMERAL") {
        config.ephemeral = parse_bool_env(&value);
    }
    if let Ok(value) = env::var("PIRATE_I2P_STARTUP_TIMEOUT_SECS") {
        if let Ok(secs) = value.trim().parse::<u64>() {
            config.startup_timeout = Duration::from_secs(secs.max(1));
        }
    }
    if let Ok(value) = env::var("PIRATE_I2P_EXTRA_ARGS") {
        let extra_args = split_list_env(&value);
        if !extra_args.is_empty() {
            config.extra_args = extra_args;
        }
    }

    config
}

fn parse_bool_env(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn split_list_env(value: &str) -> Vec<String> {
    value
        .split([',', ';', '\n', '\r'])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_bridge_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#') && !line.starts_with("//"))
        .map(|line| line.to_string())
        .collect()
}

fn normalize_bridge_lines_input(lines: Vec<String>) -> Vec<String> {
    let mut normalized = parse_bridge_lines(&lines.join("\n"));
    normalized.retain(|line| {
        let lower = line.to_lowercase();
        lower != "bridge snowflake" && lower != "snowflake"
    });
    normalized
}

fn bundled_snowflake_bridges() -> Vec<String> {
    let raw = include_str!("../assets/tor/snowflake_bridges.txt");
    parse_bridge_lines(raw)
}

fn jitter_duration(duration: Duration) -> Duration {
    let millis = duration.as_millis() as u64;
    if millis == 0 {
        return duration;
    }
    let jitter = rand::thread_rng().gen_range(0.8..1.2);
    let jittered = (millis as f64 * jitter) as u64;
    Duration::from_millis(jittered.max(1))
}

fn is_transport_not_ready_error(err: &Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("service was not ready")
        || msg.contains("transport error")
        || msg.contains("not connected")
}

/// Bootstrap transport early (Tor/I2P/SOCKS5) without touching wallet state.
pub async fn bootstrap_transport(mode: TransportMode, socks5_url: Option<String>) -> Result<()> {
    let config = build_transport_config_from_mode(mode, socks5_url.as_deref())?;
    set_desired_transport_config(config.clone());
    let manager = GLOBAL_TRANSPORT.get_or_init(config).await?;
    manager.ensure_ready().await.map_err(map_net_error)?;
    Ok(())
}

/// Get current Tor status if transport manager is initialized.
pub async fn tor_status() -> Option<pirate_net::TorStatus> {
    let manager = GLOBAL_TRANSPORT.get().await?;
    manager.tor_status().await
}

/// Rotate Tor exit circuits by isolating future streams.
pub async fn rotate_tor_exit() -> Result<()> {
    let manager = GLOBAL_TRANSPORT
        .get()
        .await
        .ok_or_else(|| Error::Connection("Transport manager not initialized".to_string()))?;
    manager.rotate_tor_exit().await.map_err(map_net_error)?;
    Ok(())
}

/// Fetch the TLS SPKI pin from a lightwalletd endpoint using the configured transport.
pub async fn fetch_spki_pin(
    host: &str,
    port: u16,
    server_name: Option<String>,
    mode: TransportMode,
    socks5_url: Option<String>,
) -> Result<String> {
    let config = build_transport_config_from_mode(mode, socks5_url.as_deref())?;
    let manager = GLOBAL_TRANSPORT.get_or_init(config).await?;
    let server_name = server_name.unwrap_or_else(|| host.to_string());
    manager
        .fetch_spki_pin(host, port, &server_name)
        .await
        .map_err(map_net_error)
}

/// Fetch arbitrary HTTP(S) bytes using the configured transport.
pub async fn fetch_http_bytes(
    url: String,
    headers: Vec<(String, String)>,
    mode: TransportMode,
    socks5_url: Option<String>,
) -> Result<Vec<u8>> {
    let config = build_transport_config_from_mode(mode, socks5_url.as_deref())?;
    let manager = GLOBAL_TRANSPORT.get_or_init(config).await?;
    manager
        .fetch_url_bytes(&url, &headers)
        .await
        .map_err(map_net_error)
}

/// Get current I2P status if transport manager is initialized.
pub async fn i2p_status() -> Option<pirate_net::I2pStatus> {
    let manager = GLOBAL_TRANSPORT.get().await?;
    manager.i2p_status().await
}

/// Shutdown any active transport manager.
pub async fn shutdown_transport() {
    clear_desired_transport_config();
    GLOBAL_TRANSPORT.shutdown().await;
}

/// Compact block data received from lightwalletd
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactBlock {
    /// Proto version
    #[serde(default)]
    pub proto_version: u32,
    /// Block height
    pub height: u64,
    /// Block hash (32 bytes)
    pub hash: Vec<u8>,
    /// Previous block hash (32 bytes)
    #[serde(default)]
    pub prev_hash: Vec<u8>,
    /// Block timestamp (Unix epoch)
    pub time: u32,
    /// Block header bytes
    #[serde(default)]
    pub header: Vec<u8>,
    /// Compact transactions in this block
    pub transactions: Vec<CompactTx>,
}

impl From<proto::CompactBlock> for CompactBlock {
    fn from(pb: proto::CompactBlock) -> Self {
        Self {
            proto_version: pb.proto_version,
            height: pb.height,
            hash: pb.hash,
            prev_hash: pb.prev_hash,
            time: pb.time,
            header: pb.header,
            transactions: pb.vtx.into_iter().map(CompactTx::from).collect(),
        }
    }
}

impl From<CompactBlock> for proto::CompactBlock {
    fn from(block: CompactBlock) -> Self {
        Self {
            proto_version: if block.proto_version == 0 {
                1
            } else {
                block.proto_version
            },
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            time: block.time,
            header: block.header,
            vtx: block
                .transactions
                .into_iter()
                .map(proto::CompactTx::from)
                .collect(),
        }
    }
}

/// Compact transaction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactTx {
    /// Transaction index within block
    #[serde(default)]
    pub index: Option<u64>,
    /// Transaction hash (32 bytes)
    pub hash: Vec<u8>,
    /// Transaction fee (arrrtoshis)
    #[serde(default)]
    pub fee: Option<u32>,
    /// Sapling spends (nullifiers)
    #[serde(default)]
    pub spends: Vec<CompactSaplingSpend>,
    /// Sapling outputs
    pub outputs: Vec<CompactSaplingOutput>,
    /// Ironwood actions
    pub actions: Vec<CompactIronwoodAction>,
}

impl From<proto::CompactTx> for CompactTx {
    fn from(pb: proto::CompactTx) -> Self {
        Self {
            index: Some(pb.index),
            hash: pb.hash,
            fee: Some(pb.fee),
            spends: pb
                .spends
                .into_iter()
                .map(CompactSaplingSpend::from)
                .collect(),
            outputs: pb
                .outputs
                .into_iter()
                .map(CompactSaplingOutput::from)
                .collect(),
            actions: pb
                .actions
                .into_iter()
                .map(CompactIronwoodAction::from)
                .collect(),
        }
    }
}

impl From<CompactTx> for proto::CompactTx {
    fn from(tx: CompactTx) -> Self {
        Self {
            index: tx.index.unwrap_or(0),
            hash: tx.hash,
            fee: tx.fee.unwrap_or(0),
            spends: tx
                .spends
                .into_iter()
                .map(proto::CompactSaplingSpend::from)
                .collect(),
            outputs: tx
                .outputs
                .into_iter()
                .map(proto::CompactSaplingOutput::from)
                .collect(),
            actions: tx
                .actions
                .into_iter()
                .map(proto::CompactIronwoodAction::from)
                .collect(),
        }
    }
}

/// Compact Sapling spend (nullifier only)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactSaplingSpend {
    /// Nullifier (32 bytes)
    pub nf: Vec<u8>,
}

impl From<proto::CompactSaplingSpend> for CompactSaplingSpend {
    fn from(pb: proto::CompactSaplingSpend) -> Self {
        Self { nf: pb.nf }
    }
}

impl From<CompactSaplingSpend> for proto::CompactSaplingSpend {
    fn from(spend: CompactSaplingSpend) -> Self {
        Self { nf: spend.nf }
    }
}

/// Compact Sapling output (for trial decryption)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactSaplingOutput {
    /// Note commitment (32 bytes)
    pub cmu: Vec<u8>,
    /// Ephemeral public key (32 bytes)
    pub ephemeral_key: Vec<u8>,
    /// Encrypted ciphertext (first 52 bytes only)
    pub ciphertext: Vec<u8>,
}

impl From<proto::CompactSaplingOutput> for CompactSaplingOutput {
    fn from(pb: proto::CompactSaplingOutput) -> Self {
        Self {
            cmu: pb.cmu,
            ephemeral_key: pb.ephemeral_key,
            ciphertext: pb.ciphertext,
        }
    }
}

impl From<CompactSaplingOutput> for proto::CompactSaplingOutput {
    fn from(output: CompactSaplingOutput) -> Self {
        Self {
            cmu: output.cmu,
            ephemeral_key: output.ephemeral_key,
            ciphertext: output.ciphertext,
        }
    }
}

/// Compact Ironwood action (for trial decryption)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactIronwoodAction {
    /// Nullifier (32 bytes)
    pub nullifier: Vec<u8>,
    /// Note commitment (32 bytes)
    pub cmx: Vec<u8>,
    /// Ephemeral public key (32 bytes)
    pub ephemeral_key: Vec<u8>,
    /// Encrypted ciphertext (for note encryption)
    pub enc_ciphertext: Vec<u8>,
    /// Outgoing ciphertext (for OVK recovery)
    pub out_ciphertext: Vec<u8>,
}

impl From<proto::CompactIronwoodAction> for CompactIronwoodAction {
    fn from(pb: proto::CompactIronwoodAction) -> Self {
        Self {
            nullifier: pb.nullifier,
            cmx: pb.cmx,
            ephemeral_key: pb.ephemeral_key,
            enc_ciphertext: pb.ciphertext, // Proto field is "ciphertext", we call it enc_ciphertext internally
            out_ciphertext: Vec::new(),    // Not in server's compact format, only in full format
        }
    }
}

impl From<CompactIronwoodAction> for proto::CompactIronwoodAction {
    fn from(action: CompactIronwoodAction) -> Self {
        Self {
            nullifier: action.nullifier,
            cmx: action.cmx,
            ephemeral_key: action.ephemeral_key,
            ciphertext: action.enc_ciphertext, // Proto field is "ciphertext", we call it enc_ciphertext internally
        }
    }
}

fn estimate_compact_block_bytes(block: &CompactBlock) -> u64 {
    let mut total = 0u64;
    for tx in &block.transactions {
        // Rough tx overhead (hash/index/etc.)
        total += 100;
        for output in &tx.outputs {
            let ct_len = output.ciphertext.len().max(52) as u64;
            total += 32 + 32 + ct_len;
        }
        for action in &tx.actions {
            let enc_len = action.enc_ciphertext.len().max(52) as u64;
            let out_len = action.out_ciphertext.len().max(52) as u64;
            total += 32 + 32 + 32 + enc_len + out_len;
        }
    }
    total
}

/// Transaction broadcast result
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    /// Transaction ID (hex string)
    pub txid: String,
    /// Error code (0 = success)
    pub error_code: i32,
    /// Error message (empty on success)
    pub error_message: String,
}

/// Lightwalletd server info
#[derive(Debug, Clone)]
pub struct LightdInfo {
    /// Server version
    pub version: String,
    /// Vendor name
    pub vendor: String,
    /// Chain name (e.g., "ARRR")
    pub chain_name: String,
    /// Consensus branch id reported by the server (hex)
    pub consensus_branch_id: String,
    /// Current block height
    pub block_height: u64,
    /// Estimated network height
    pub estimated_height: u64,
    /// Sapling activation height
    pub sapling_activation_height: u64,
}

impl From<proto::LightdInfo> for LightdInfo {
    fn from(pb: proto::LightdInfo) -> Self {
        Self {
            version: pb.version,
            vendor: pb.vendor,
            chain_name: pb.chain_name,
            consensus_branch_id: pb.consensus_branch_id,
            block_height: pb.block_height,
            estimated_height: pb.estimated_height,
            sapling_activation_height: pb.sapling_activation_height,
        }
    }
}

/// Tree state for Sapling and Ironwood note commitment trees
#[derive(Debug, Clone)]
pub struct TreeState {
    /// Network name ("main" or "test")
    pub network: String,
    /// Block height for this tree state
    pub height: u64,
    /// Block hash (hex string)
    pub hash: String,
    /// Unix epoch time when the block was mined
    pub time: u32,
    /// Sapling tree state (hex-encoded string)
    pub sapling_tree: String,
    /// Sapling frontier (hex-encoded string)
    pub sapling_frontier: String,
    /// Ironwood tree state (hex-encoded string, empty if Ironwood is not activated)
    pub ironwood_tree: String,
}

#[derive(Default)]
struct EndpointPoolState {
    probed: bool,
    active_index: usize,
    healthy_indices: Vec<usize>,
    failures: HashMap<usize, u32>,
    tips: HashMap<usize, u64>,
    channels: HashMap<usize, Channel>,
}

/// Lightwalletd gRPC client.
///
/// Provides tip queries, bounded compact-block streams, endpoint failover, and
/// transaction broadcast through the configured privacy transport.
pub struct LightClient {
    config: LightClientConfig,
    channel: Arc<Mutex<Option<Channel>>>,
    endpoint_pool: Arc<RwLock<EndpointPoolState>>,
    subtree_root_capabilities: Arc<StdRwLock<HashMap<(String, i32), SubtreeRootCapability>>>,
}

#[derive(Clone, Copy)]
enum SubtreeRootCapability {
    Available,
    RetryAfter(Instant),
}

const SUBTREE_ROOT_TRANSIENT_RETRY: Duration = Duration::from_secs(60);
const SUBTREE_ROOT_TIMEOUT_RETRY: Duration = Duration::from_secs(10 * 60);
const SUBTREE_ROOT_UNSUPPORTED_RETRY: Duration = Duration::from_secs(24 * 60 * 60);

/// Full transaction payload returned by lightwalletd.
#[derive(Debug, Clone)]
pub struct RawTransactionData {
    /// Raw serialized transaction bytes.
    pub data: Vec<u8>,
    /// Block height reported by lightwalletd, when available.
    pub height: Option<u64>,
}

impl LightClient {
    fn is_non_retryable_status(code: tonic::Code) -> bool {
        matches!(
            code,
            tonic::Code::InvalidArgument
                | tonic::Code::Unimplemented
                | tonic::Code::FailedPrecondition
                | tonic::Code::PermissionDenied
        )
    }

    fn is_non_retryable_error(error: &Error) -> bool {
        match error {
            Error::Status(status) => Self::is_non_retryable_status(status.code()),
            Error::Sync(msg) | Error::Network(msg) | Error::Connection(msg) => {
                msg.starts_with("NON_RETRYABLE:")
            }
            _ => false,
        }
    }

    /// Create new client with default configuration
    ///
    /// Default: uses DEFAULT_LIGHTD_URL via Tor (TLS disabled unless enabled in config)
    pub fn new(endpoint: String) -> Self {
        Self {
            config: LightClientConfig {
                endpoint,
                ..Default::default()
            },
            channel: Arc::new(Mutex::new(None)),
            endpoint_pool: Arc::new(RwLock::new(EndpointPoolState::default())),
            subtree_root_capabilities: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Create client with custom configuration
    pub fn with_config(config: LightClientConfig) -> Self {
        Self {
            config,
            channel: Arc::new(Mutex::new(None)),
            endpoint_pool: Arc::new(RwLock::new(EndpointPoolState::default())),
            subtree_root_capabilities: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Create client with retry configuration
    pub fn with_retry_config(endpoint: String, retry_config: RetryConfig) -> Self {
        Self {
            config: LightClientConfig {
                endpoint,
                retry: retry_config,
                ..Default::default()
            },
            channel: Arc::new(Mutex::new(None)),
            endpoint_pool: Arc::new(RwLock::new(EndpointPoolState::default())),
            subtree_root_capabilities: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Get current endpoint URL
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    /// Get current transport mode.
    pub fn transport_mode(&self) -> TransportMode {
        self.config.transport
    }

    /// Whether this client has explicitly configured failover endpoints.
    pub fn has_failover_endpoints(&self) -> bool {
        !self.config.failover_endpoints.is_empty()
    }

    fn endpoint_candidate(&self, index: usize) -> Option<LightClientEndpoint> {
        if index == 0 {
            return Some(LightClientEndpoint {
                endpoint: self.config.endpoint.clone(),
                tls: self.config.tls.clone(),
            });
        }
        self.config.failover_endpoints.get(index - 1).cloned()
    }

    fn candidate_client(&self, index: usize) -> Option<Self> {
        let candidate = self.endpoint_candidate(index)?;
        let mut config = self.config.clone();
        config.endpoint = candidate.endpoint;
        config.tls = candidate.tls;
        config.failover_endpoints.clear();
        config.retry.max_attempts = 1;
        Some(Self {
            config,
            channel: Arc::new(Mutex::new(None)),
            endpoint_pool: Arc::clone(&self.endpoint_pool),
            subtree_root_capabilities: Arc::clone(&self.subtree_root_capabilities),
        })
    }

    async fn connected_candidate_client(&self, index: usize) -> Option<Self> {
        let candidate = self.endpoint_candidate(index)?;
        let channel = if index == 0 {
            self.channel.lock().await.clone()
        } else {
            self.endpoint_pool
                .read()
                .await
                .channels
                .get(&index)
                .cloned()
        }?;
        let mut config = self.config.clone();
        config.endpoint = candidate.endpoint;
        config.tls = candidate.tls;
        config.failover_endpoints.clear();
        config.retry.max_attempts = 1;
        Some(Self {
            config,
            channel: Arc::new(Mutex::new(Some(channel))),
            endpoint_pool: Arc::clone(&self.endpoint_pool),
            subtree_root_capabilities: Arc::clone(&self.subtree_root_capabilities),
        })
    }

    fn endpoint_count(&self) -> usize {
        1usize.saturating_add(self.config.failover_endpoints.len())
    }

    /// Probe configured endpoints through the selected transport and retain only
    /// candidates that match the primary endpoint at a common chain anchor.
    pub async fn probe_endpoints(&self) -> Vec<EndpointHealth> {
        let endpoint_count = self.endpoint_count();
        let probe_timeout = match self.config.transport {
            TransportMode::Direct => Duration::from_secs(12),
            TransportMode::Tor | TransportMode::I2p | TransportMode::Socks5 => {
                Duration::from_secs(35)
            }
        };
        let mut probes: Vec<Option<(LightdInfo, u64, Channel)>> =
            Vec::with_capacity(endpoint_count);
        let mut health = Vec::with_capacity(endpoint_count);

        for index in 0..endpoint_count {
            let Some(candidate) = self.candidate_client(index) else {
                continue;
            };
            let endpoint = candidate.endpoint().to_string();
            let result = tokio::time::timeout(probe_timeout, async {
                candidate.connect().await?;
                let info = candidate.get_lightd_info().await?;
                let tip = candidate.get_latest_block().await?;
                Ok::<_, Error>((info, tip))
            })
            .await;

            match result {
                Ok(Ok((info, tip))) => {
                    let channel = candidate.channel.lock().await.clone();
                    probes.push(channel.map(|channel| (info, tip, channel)));
                    health.push(EndpointHealth {
                        endpoint,
                        healthy: true,
                        tip_height: Some(tip),
                        reason: None,
                    });
                }
                Ok(Err(error)) => {
                    probes.push(None);
                    health.push(EndpointHealth {
                        endpoint,
                        healthy: false,
                        tip_height: None,
                        reason: Some(error.to_string()),
                    });
                }
                Err(_) => {
                    probes.push(None);
                    health.push(EndpointHealth {
                        endpoint,
                        healthy: false,
                        tip_height: None,
                        reason: Some(format!("health probe timed out after {:?}", probe_timeout)),
                    });
                }
            }
        }

        let Some((primary_info, primary_tip, primary_channel)) =
            probes.first().and_then(Option::as_ref)
        else {
            for entry in health.iter_mut().skip(1) {
                entry.healthy = false;
                entry.reason = Some(
                    "primary endpoint unavailable; same-chain failover anchor could not be established"
                        .to_string(),
                );
            }
            let mut state = self.endpoint_pool.write().await;
            state.probed = true;
            state.active_index = 0;
            state.healthy_indices.clear();
            state.tips.clear();
            state.channels.clear();
            return health;
        };
        *self.channel.lock().await = Some(primary_channel.clone());

        let common_anchor = probes
            .iter()
            .filter_map(|probe| probe.as_ref().map(|(_, tip, _)| *tip))
            .min()
            .unwrap_or(*primary_tip)
            .saturating_sub(10);
        let common_anchor_u32 = u32::try_from(common_anchor).unwrap_or(u32::MAX);
        let primary_anchor = match self.candidate_client(0) {
            Some(mut primary) => {
                primary.channel = Arc::new(Mutex::new(Some(primary_channel.clone())));
                tokio::time::timeout(probe_timeout, primary.get_block(common_anchor_u32))
                    .await
                    .ok()
                    .and_then(std::result::Result::ok)
                    .map(|block| block.hash)
            }
            None => None,
        };

        for index in 1..endpoint_count {
            let Some((info, tip, channel)) = probes.get(index).and_then(Option::as_ref) else {
                continue;
            };
            let metadata_matches = info
                .chain_name
                .eq_ignore_ascii_case(&primary_info.chain_name)
                && info.sapling_activation_height == primary_info.sapling_activation_height
                && (*tip != *primary_tip
                    || info.consensus_branch_id == primary_info.consensus_branch_id);
            if !metadata_matches {
                if let Some(entry) = health.get_mut(index) {
                    entry.healthy = false;
                    entry.reason = Some("server chain metadata differs from primary".to_string());
                }
                continue;
            }

            let alternate_anchor = match self.candidate_client(index) {
                Some(mut candidate) => {
                    candidate.channel = Arc::new(Mutex::new(Some(channel.clone())));
                    tokio::time::timeout(probe_timeout, candidate.get_block(common_anchor_u32))
                        .await
                        .ok()
                        .and_then(std::result::Result::ok)
                        .map(|block| block.hash)
                }
                None => None,
            };
            if primary_anchor.is_none() || alternate_anchor != primary_anchor {
                if let Some(entry) = health.get_mut(index) {
                    entry.healthy = false;
                    entry.reason = Some(format!(
                        "server block hash differs from primary at height {}",
                        common_anchor
                    ));
                }
            }
        }

        let healthy_indices = health
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.healthy.then_some(index))
            .collect::<Vec<_>>();
        let mut state = self.endpoint_pool.write().await;
        state.probed = true;
        state.active_index = healthy_indices.first().copied().unwrap_or(0);
        state.healthy_indices = healthy_indices;
        state.failures.clear();
        state.tips = probes
            .iter()
            .enumerate()
            .filter_map(|(index, probe)| probe.as_ref().map(|(_, tip, _)| (index, *tip)))
            .collect();
        state.channels = probes
            .into_iter()
            .enumerate()
            .filter_map(|(index, probe)| {
                health
                    .get(index)
                    .is_some_and(|entry| entry.healthy)
                    .then(|| probe.map(|(_, _, channel)| (index, channel)))
                    .flatten()
            })
            .collect();
        health
    }

    async fn candidate_order(&self, minimum_tip: u64) -> Vec<usize> {
        let state = self.endpoint_pool.read().await;
        let mut candidates = if state.probed && !state.healthy_indices.is_empty() {
            state.healthy_indices.clone()
        } else {
            vec![0]
        };
        if let Some(active_position) = candidates
            .iter()
            .position(|index| *index == state.active_index)
        {
            candidates.rotate_left(active_position);
        }
        candidates.retain(|index| state.tips.get(index).is_none_or(|tip| *tip >= minimum_tip));
        candidates
    }

    async fn record_candidate_success(&self, index: usize) {
        let mut state = self.endpoint_pool.write().await;
        state.active_index = index;
        state.failures.remove(&index);
    }

    async fn record_candidate_failure(&self, index: usize) {
        let mut state = self.endpoint_pool.write().await;
        let failures = state.failures.entry(index).or_insert(0);
        *failures = failures.saturating_add(1);
    }

    /// Check if client is connected
    pub fn is_connected(&self) -> bool {
        // Channel exists (actual connectivity tested on RPC call)
        self.channel
            .try_lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Connect to lightwalletd server with retry
    pub async fn connect(&self) -> Result<()> {
        let mut attempt = 0;
        let mut backoff = self.config.retry.initial_backoff;

        loop {
            match self.try_connect().await {
                Ok(channel) => {
                    info!("Connected to lightwalletd at {}", self.config.endpoint);
                    *self.channel.lock().await = Some(channel);
                    return Ok(());
                }
                Err(e) => {
                    attempt += 1;
                    if attempt >= self.config.retry.max_attempts {
                        error!("Failed to connect after {} attempts: {}", attempt, e);
                        return Err(e);
                    }

                    warn!(
                        "Connection attempt {} failed, retrying in {:?}: {}",
                        attempt, backoff, e
                    );

                    tokio::time::sleep(jitter_duration(backoff)).await;

                    backoff = std::cmp::min(
                        Duration::from_millis(
                            (backoff.as_millis() as f64 * self.config.retry.backoff_multiplier)
                                as u64,
                        ),
                        self.config.retry.max_backoff,
                    );
                }
            }
        }
    }

    /// Disconnect from server
    pub async fn disconnect(&self) {
        *self.channel.lock().await = None;
        info!("Disconnected from lightwalletd");
    }

    async fn try_connect(&self) -> Result<Channel> {
        let endpoint_url = &self.config.endpoint;
        debug!(
            "Connecting to {} via {:?}",
            endpoint_url, self.config.transport
        );

        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"client.rs:448","message":"try_connect entry","data":{{"endpoint":"{}","tls_enabled":{},"transport":"{:?}","server_name":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"A"}}"#,
                id,
                ts,
                endpoint_url,
                self.config.tls.enabled,
                self.config.transport,
                self.config.tls.server_name
            );
        });
        // #endregion

        // Build endpoint with timeouts
        // Tonic requires URL in format: https://host:port or http://host:port
        let mut endpoint = match Endpoint::from_shared(endpoint_url.to_string()) {
            Ok(ep) => ep,
            Err(e) => {
                error!("Failed to parse endpoint URL '{}': {}", endpoint_url, e);
                return Err(Error::Connection(format!(
                    "Invalid endpoint URL format '{}': {}. Expected format: https://host:port",
                    endpoint_url, e
                )));
            }
        };

        endpoint = endpoint
            .connect_timeout(self.config.connect_timeout)
            .timeout(self.config.request_timeout);

        // Keepalive to avoid hung streams after network transitions (mobile background/resume,
        // Tor circuit changes, etc.). We avoid keepalives while idle to reduce background chatter.
        let is_mobile = cfg!(target_os = "android") || cfg!(target_os = "ios");
        let tcp_keepalive = Some(Duration::from_secs(if is_mobile { 60 } else { 30 }));
        let h2_keepalive_interval = Duration::from_secs(if is_mobile { 60 } else { 30 });
        let h2_keepalive_timeout = Duration::from_secs(15);

        endpoint = endpoint
            .tcp_keepalive(tcp_keepalive)
            .http2_keep_alive_interval(h2_keepalive_interval)
            .keep_alive_timeout(h2_keepalive_timeout)
            .keep_alive_while_idle(false);

        // Configure TLS if enabled
        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"client.rs:467","message":"TLS check","data":{{"tls_enabled":{},"endpoint":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                id, ts, self.config.tls.enabled, endpoint_url
            );
        });
        // #endregion
        if self.config.tls.enabled {
            let mut tls_config = ClientTlsConfig::new();

            // Set server name for SNI (required for TLS)
            if let Some(ref server_name) = self.config.tls.server_name {
                debug!("Using explicit server name for TLS: {}", server_name);
                tls_config = tls_config.domain_name(server_name.clone());
            } else {
                // Extract hostname from endpoint for SNI
                if let Some(host) = extract_host(endpoint_url) {
                    debug!("Extracted hostname for TLS SNI: {}", host);
                    tls_config = tls_config.domain_name(host);
                } else {
                    warn!(
                        "Could not extract hostname from endpoint '{}' for TLS SNI",
                        endpoint_url
                    );
                    // Try to continue without explicit domain name (tonic might handle it)
                }
            }

            // Note: SPKI pinning verification happens after connection
            // tonic doesn't support custom certificate verifiers directly
            // We verify the SPKI pin via a post-connect check (see verify_spki_pin)
            if self.config.tls.spki_pin.is_some() {
                debug!("SPKI pin configured, will verify after connection");
            }

            endpoint = endpoint.tls_config(tls_config).map_err(|e| {
                error!(
                    "Failed to configure TLS for endpoint '{}': {}",
                    endpoint_url, e
                );
                Error::Connection(format!("TLS configuration failed: {}", e))
            })?;
        }

        if self.config.transport == TransportMode::Direct {
            warn!("Using DIRECT connection - IP address exposed to server!");
        }

        let transport_config = build_transport_config(&self.config)?;
        let manager = GLOBAL_TRANSPORT.get_or_init(transport_config).await?;
        if self.config.tls.enabled {
            if let Some(expected_pin) = self.config.tls.spki_pin.as_deref() {
                let host = extract_host(endpoint_url).ok_or_else(|| {
                    Error::Connection(format!(
                        "Could not extract host from endpoint URL '{}'",
                        endpoint_url
                    ))
                })?;
                let port = extract_port(endpoint_url).unwrap_or(DEFAULT_LIGHTD_PORT);
                let server_name = self
                    .config
                    .tls
                    .server_name
                    .clone()
                    .unwrap_or_else(|| host.clone());
                let actual_pin = manager
                    .fetch_spki_pin(&host, port, &server_name)
                    .await
                    .map_err(map_net_error)?;
                if normalize_spki_pin(expected_pin) != normalize_spki_pin(&actual_pin) {
                    return Err(Error::Connection(format!(
                        "TLS SPKI pin mismatch for {}",
                        endpoint_url
                    )));
                }
            }
        }
        let result = manager.create_grpc_channel(endpoint).await;

        match result {
            Ok(channel) => Ok(channel),
            Err(e) => {
                error!("Connection failed to {}: {}", endpoint_url, e);
                let error_msg = e.to_string();

                if matches!(self.config.transport, TransportMode::Direct) {
                    let cleaned = error_msg.to_lowercase();
                    if cleaned.contains("certificate")
                        || cleaned.contains("tls")
                        || cleaned.contains("ssl")
                        || cleaned.contains("invalidcertificate")
                        || cleaned.contains("notvalidforname")
                    {
                        return Err(Error::Connection(format!(
                            "TLS/SSL certificate validation failed for {}: {}. This often happens when connecting via IP address because the server's certificate is issued for a hostname (e.g., lightd1.piratechain.com). Try using the hostname instead of the IP address, or ensure the certificate includes the IP in its SAN field.",
                            endpoint_url, error_msg
                        )));
                    }
                    if cleaned.contains("timeout") || cleaned.contains("timed out") {
                        return Err(Error::Connection(format!(
                            "Connection timeout to {}: {}. The server may be unreachable or firewall may be blocking.",
                            endpoint_url, error_msg
                        )));
                    }
                    if cleaned.contains("refused") || cleaned.contains("connection refused") {
                        return Err(Error::Connection(format!(
                            "Connection refused by {}: {}. The server may be down or not accepting connections.",
                            endpoint_url, error_msg
                        )));
                    }
                    if cleaned.contains("dns")
                        || cleaned.contains("name resolution")
                        || cleaned.contains("failed to lookup")
                    {
                        return Err(Error::Connection(format!(
                            "DNS resolution failed for {}: {}. The hostname may not exist or DNS may be misconfigured. Try using the IP address directly.",
                            endpoint_url, error_msg
                        )));
                    }
                }

                Err(Error::Connection(format!(
                    "Transport connection failed: {}",
                    error_msg
                )))
            }
        }
    }

    async fn get_client(&self) -> Result<CompactTxStreamerClient<Channel>> {
        let guard = self.channel.lock().await;
        let channel = guard
            .as_ref()
            .ok_or_else(|| Error::Connection("Not connected".to_string()))?
            .clone();
        Ok(CompactTxStreamerClient::new(channel))
    }

    async fn get_latest_block_internal(&self) -> Result<u64> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let request = tonic::Request::new(ChainSpec {
                network: String::new(), // Empty for default network
            });

            let response = client.get_latest_block(request).await?;
            let block_id = response.into_inner();

            debug!(
                "Latest block: height={}, hash={}",
                block_id.height,
                hex::encode(&block_id.hash)
            );

            Ok(block_id.height)
        })
        .await
    }

    /// Get the latest block height from the server
    ///
    /// Returns the current blockchain tip height.
    pub async fn get_latest_block(&self) -> Result<u64> {
        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"client.rs:564","message":"get_latest_block entry","data":{{"endpoint":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                id, ts, self.config.endpoint
            );
        });
        // #endregion

        let mut result = self.get_latest_block_internal().await;

        if let Err(err) = &result {
            if is_transport_not_ready_error(err) {
                warn!(
                    "Latest-block call hit transient transport readiness issue, reconnecting and retrying once: {:?}",
                    err
                );
                self.disconnect().await;
                if let Err(conn_err) = self.connect().await {
                    warn!("Reconnect before latest-block retry failed: {:?}", conn_err);
                } else {
                    result = self.get_latest_block_internal().await;
                }
            }
        }

        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"client.rs:580","message":"get_latest_block result","data":{{"success":{},"height":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                id,
                ts,
                result.is_ok(),
                result.as_ref().ok().copied().unwrap_or(0),
                result.as_ref().err()
            );
        });
        // #endregion
        result
    }

    /// Get compact blocks in the specified range
    ///
    /// Streams blocks from `range.start` to `range.end` (exclusive).
    /// Returns Vec for simplicity; use `stream_blocks` for large ranges.
    pub async fn get_compact_block_range(&self, range: Range<u32>) -> Result<Vec<CompactBlock>> {
        self.get_compact_block_range_with_wallet(range, None).await
    }

    /// Get compact blocks in the specified range with optional wallet context for logging.
    pub async fn get_compact_block_range_with_wallet(
        &self,
        range: Range<u32>,
        wallet_id: Option<&str>,
    ) -> Result<Vec<CompactBlock>> {
        if range.is_empty() {
            return Ok(Vec::new());
        }

        let range = range.clone();
        let wallet_id_owned = wallet_id.map(str::to_string);

        self.with_retry(move || {
            let range = range.clone();
            let wallet_id_owned = wallet_id_owned.clone();
            async move {
            let mut client = self.get_client().await?;
            let start_instant = Instant::now();
            let range_blocks = range.end.saturating_sub(range.start).max(1);
            let (first_msg_timeout, next_msg_timeout, request_timeout) =
                compact_block_range_timeouts(
                    self.config.transport,
                    range_blocks as u64,
                    self.config.request_timeout,
                );

            let mut request = tonic::Request::new(BlockRange {
                start: Some(BlockId {
                    height: range.start as u64,
                    hash: Vec::new(),
                }),
                end: Some(BlockId {
                    height: (range.end - 1) as u64, // end is inclusive in proto
                    hash: Vec::new(),
                }),
            });
            let open_timeout = first_msg_timeout.saturating_add(Duration::from_secs(10));
            request.set_timeout(request_timeout);

            debug!("Requesting blocks {}..{}", range.start, range.end);
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_{}","timestamp":{},"location":"client.rs:block_range_request","message":"block range request","data":{{"wallet_id":"{}","start":{},"end":{},"range_blocks":{},"first_timeout_secs":{},"next_timeout_secs":{},"open_timeout_secs":{},"request_timeout_secs":{},"endpoint":"{}","transport":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                    id,
                    ts,
                    wallet_id_owned.as_deref().unwrap_or("unknown"),
                    range.start,
                    range.end.saturating_sub(1),
                    range_blocks,
                    first_msg_timeout.as_secs(),
                    next_msg_timeout.as_secs(),
                    open_timeout.as_secs(),
                    request_timeout.as_secs(),
                    self.config.endpoint,
                    self.config.transport
                );
            });

            let stream_response = tokio::time::timeout(open_timeout, client.get_block_range(request))
                .await
                .map_err(|_| {
                    Error::Network(format!(
                        "Timed out opening compact block stream ({}..{}; timeout {:?})",
                        range.start,
                        range.end,
                        open_timeout
                    ))
                })??;
            let mut stream = stream_response.into_inner();
            let mut blocks = Vec::with_capacity((range.end - range.start) as usize);
            let mut first_block_ms: Option<u128> = None;
            let mut estimated_bytes = 0u64;

            loop {
                let idle_timeout = if blocks.is_empty() {
                    first_msg_timeout
                } else {
                    next_msg_timeout
                };

                let msg = tokio::time::timeout(idle_timeout, stream.message())
                    .await
                    .map_err(|_| {
                        Error::Network(format!(
                            "Timed out waiting for compact block stream ({}..{}, received {} blocks; idle {:?})",
                            range.start,
                            range.end,
                            blocks.len(),
                            idle_timeout
                        ))
                    })??;

                let Some(block) = msg else {
                    break;
                };

                if first_block_ms.is_none() {
                    first_block_ms = Some(start_instant.elapsed().as_millis());
                }
                let compact = CompactBlock::from(block);
                estimated_bytes = estimated_bytes
                    .saturating_add(estimate_compact_block_bytes(&compact));
                blocks.push(compact);
            }

            let total_ms = start_instant.elapsed().as_millis();
            let ttfb_ms = first_block_ms.unwrap_or(total_ms);
            let kbps = if total_ms > 0 {
                (estimated_bytes as f64 / 1024.0) / (total_ms as f64 / 1000.0)
            } else {
                0.0
            };

            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_{}","timestamp":{},"location":"client.rs:block_range_stats","message":"block range stats","data":{{"wallet_id":"{}","start":{},"end":{},"blocks":{},"ttfb_ms":{},"total_ms":{},"est_bytes":{},"est_kbps":{:.2},"endpoint":"{}","transport":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                    id,
                    ts,
                    wallet_id_owned.as_deref().unwrap_or("unknown"),
                    range.start,
                    range.end.saturating_sub(1),
                    blocks.len(),
                    ttfb_ms,
                    total_ms,
                    estimated_bytes,
                    kbps,
                    self.config.endpoint,
                    self.config.transport
                );
            });

            debug!("Received {} blocks", blocks.len());
            Ok(blocks)
            }
        })
        .await
    }

    /// Stream compact blocks in batches
    ///
    /// For large ranges, fetches blocks in batches of `batch_size`.
    pub async fn get_block_range_batched(
        &self,
        start: u64,
        end: u64,
        batch_size: u64,
    ) -> Result<Vec<CompactBlock>> {
        let mut all_blocks = Vec::new();
        let mut current = start;

        while current <= end {
            let batch_end = std::cmp::min(current + batch_size, end + 1);
            let blocks = self
                .get_compact_block_range(current as u32..batch_end as u32)
                .await?;

            debug!(
                "Fetched batch {}-{} ({} blocks)",
                current,
                batch_end - 1,
                blocks.len()
            );

            all_blocks.extend(blocks);
            current = batch_end;
        }

        Ok(all_blocks)
    }

    /// Stream blocks in a range (legacy API, uses u64 for compatibility)
    ///
    /// This is a compatibility wrapper around `get_compact_block_range`.
    pub async fn stream_blocks(&self, start: u64, end: u64) -> Result<Vec<CompactBlock>> {
        // Convert to inclusive range with u32
        self.get_compact_block_range(start as u32..(end + 1) as u32)
            .await
    }

    /// Broadcast a raw transaction to the network
    ///
    /// Returns the transaction ID on success.
    pub async fn broadcast(&self, raw_tx: Vec<u8>) -> Result<String> {
        info!("Broadcasting transaction ({} bytes)", raw_tx.len());

        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let request = tonic::Request::new(RawTransaction {
                data: raw_tx.clone(),
                height: 0, // Server will determine
            });

            let response = client.send_transaction(request).await?;
            let send_response = response.into_inner();

            if send_response.error_code != 0 {
                let error_message = send_response.error_message.to_ascii_lowercase();
                let broadcast_msg = format!(
                    "Broadcast failed: {} (code {})",
                    send_response.error_message, send_response.error_code
                );
                error!(
                    "Transaction broadcast failed: code={}, message={}",
                    send_response.error_code, send_response.error_message
                );
                // Node policy/consensus rejection is deterministic and should not be retried.
                if error_message.contains("bad-txns") || error_message.contains("unknown-anchor") {
                    return Err(Error::Sync(format!("NON_RETRYABLE: {}", broadcast_msg)));
                }
                return Err(Error::Network(broadcast_msg));
            }

            // Compute txid from raw transaction
            let txid = compute_txid(&raw_tx);
            info!("Transaction broadcast successful: {}", txid);

            Ok(txid)
        })
        .await
    }

    /// Get full transaction by hash (for memo decryption)
    ///
    /// Fetches the complete transaction data including full 580-byte ciphertexts
    /// needed for memo decryption. This is called after trial decryption finds
    /// a matching note in compact blocks.
    ///
    /// # Arguments
    /// * `tx_hash` - Transaction hash (32 bytes)
    ///
    /// # Returns
    /// Raw transaction bytes containing full shielded outputs
    pub async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Vec<u8>> {
        Ok(self.get_raw_transaction(tx_hash).await?.data)
    }

    /// Fetch the complete transaction data plus lightwalletd metadata.
    ///
    /// The height is needed by callers that decrypt Sapling outputs outside
    /// normal sync, where height-sensitive plaintext rules still apply.
    pub async fn get_raw_transaction(&self, tx_hash: &[u8; 32]) -> Result<RawTransactionData> {
        debug!(
            "Fetching full transaction for memo decryption: {}",
            hex::encode(tx_hash)
        );

        self.get_raw_transaction_by_filter(TxFilter {
            block: None, // Not used when hash is specified
            index: 0,    // Not used when hash is specified
            hash: tx_hash.to_vec(),
        })
        .await
    }

    /// Get full transaction by hash with block/index fallback.
    pub async fn get_transaction_with_fallback(
        &self,
        tx_hash: &[u8; 32],
        block_height: Option<u64>,
        tx_index: Option<u64>,
    ) -> Result<Vec<u8>> {
        match self.get_raw_transaction(tx_hash).await {
            Ok(raw) => Ok(raw.data),
            Err(err) => {
                if let (Some(height), Some(index)) = (block_height, tx_index) {
                    warn!(
                        "Hash lookup failed for tx {}, trying block/index fallback: height={}, index={}, err={}",
                        hex::encode(tx_hash),
                        height,
                        index,
                        err
                    );
                    return self
                        .get_raw_transaction_by_filter(TxFilter {
                            block: Some(BlockId {
                                height,
                                hash: Vec::new(),
                            }),
                            index,
                            hash: Vec::new(),
                        })
                        .await
                        .map(|raw| raw.data);
                }
                Err(err)
            }
        }
    }

    async fn get_raw_transaction_by_filter(&self, filter: TxFilter) -> Result<RawTransactionData> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;
            let request = tonic::Request::new(filter.clone());

            let response = client.get_transaction(request).await?;
            let raw_tx = response.into_inner();

            debug!("Received full transaction ({} bytes)", raw_tx.data.len());
            Ok(RawTransactionData {
                data: raw_tx.data,
                height: (raw_tx.height > 0).then_some(raw_tx.height),
            })
        })
        .await
    }

    /// Get lightwalletd server information
    pub async fn get_lightd_info(&self) -> Result<LightdInfo> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let request = tonic::Request::new(Empty {});
            let response = client.get_lightd_info(request).await?;

            Ok(LightdInfo::from(response.into_inner()))
        })
        .await
    }

    async fn get_tree_state_by_block_id(&self, block_id: BlockId) -> Result<TreeState> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let mut request = tonic::Request::new(block_id.clone());
            request.set_timeout(self.config.request_timeout);

            let response = client.get_tree_state(request).await?;
            let tree_state = response.into_inner();

            debug!(
                "Tree state at height {}: network={}, hash={}, saplingTree={}, ironwoodTree={}",
                tree_state.height,
                tree_state.network,
                tree_state.hash,
                tree_state.sapling_tree,
                tree_state.ironwood_tree
            );

            Ok(TreeState {
                network: tree_state.network,
                height: tree_state.height,
                hash: tree_state.hash,
                time: tree_state.time,
                sapling_tree: tree_state.sapling_tree,
                sapling_frontier: tree_state.sapling_frontier,
                ironwood_tree: tree_state.ironwood_tree,
            })
        })
        .await
    }

    /// Get tree state (Sapling and Ironwood anchors) at a specific block height
    ///
    /// If `height` is 0, returns the latest tree state.
    /// Returns TreeState with saplingTree and ironwoodTree (hex-encoded strings).
    ///
    /// # Arguments
    /// * `height` - Block height (0 for latest)
    ///
    /// # Returns
    /// TreeState containing network, height, hash, time, saplingTree, saplingFrontier, and ironwoodTree
    pub async fn get_tree_state(&self, height: u64) -> Result<TreeState> {
        self.get_tree_state_by_block_id(BlockId {
            height,
            hash: Vec::new(),
        })
        .await
    }

    /// Get tree state by block hash.
    pub async fn get_tree_state_by_hash(&self, hash: Vec<u8>) -> Result<TreeState> {
        self.get_tree_state_by_block_id(BlockId { height: 0, hash })
            .await
    }

    /// Get tree state with bridge tree support (improved long-range sync performance)
    ///
    /// Uses updated z_gettreestate RPC with bridge trees format.
    /// The block can be specified by either height or hash.
    /// Returns TreeState with saplingTree and ironwoodTree in bridge tree format.
    ///
    /// # Arguments
    /// * `height` - Block height (0 for latest)
    ///
    /// # Returns
    /// TreeState containing network, height, hash, time, saplingTree, saplingFrontier, and ironwoodTree
    /// in bridge tree format for improved long-range sync performance
    async fn get_bridge_tree_state_by_block_id(&self, block_id: BlockId) -> Result<TreeState> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let mut request = tonic::Request::new(block_id.clone());
            request.set_timeout(self.config.request_timeout);

            let response = client.get_bridge_tree_state(request).await?;
            let tree_state = response.into_inner();

            debug!(
                "Bridge tree state at height {}: network={}, hash={}, saplingTree={}, ironwoodTree={}",
                tree_state.height,
                tree_state.network,
                tree_state.hash,
                tree_state.sapling_tree,
                tree_state.ironwood_tree
            );

            Ok(TreeState {
                network: tree_state.network,
                height: tree_state.height,
                hash: tree_state.hash,
                time: tree_state.time,
                sapling_tree: tree_state.sapling_tree,
                sapling_frontier: tree_state.sapling_frontier,
                ironwood_tree: tree_state.ironwood_tree,
            })
        }).await
    }

    /// Get bridge tree state at a specific block height.
    pub async fn get_bridge_tree_state(&self, height: u64) -> Result<TreeState> {
        self.get_bridge_tree_state_by_block_id(BlockId {
            height,
            hash: Vec::new(),
        })
        .await
    }

    /// Get bridge tree state by block hash.
    pub async fn get_bridge_tree_state_by_hash(&self, hash: Vec<u8>) -> Result<TreeState> {
        self.get_bridge_tree_state_by_block_id(BlockId { height: 0, hash })
            .await
    }

    /// Get optimal block group end height for sync batching
    ///
    /// Groups blocks into ~4MB chunks for efficient sync.
    /// Returns the last block in a group starting from the given height.
    /// This helps optimize sync by using server-provided optimal batch sizes.
    ///
    /// # Arguments
    /// * `start_height` - Starting block height for the group
    ///
    /// # Returns
    /// BlockId containing the end height of the optimal block group
    pub async fn get_lite_wallet_block_group(&self, start_height: u64) -> Result<u64> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let request = tonic::Request::new(BlockId {
                height: start_height,
                hash: Vec::new(),
            });

            let response = client.get_lite_wallet_block_group(request).await?;
            let block_id = response.into_inner();

            debug!(
                "Block group for start height {}: end height={}",
                start_height, block_id.height
            );

            Ok(block_id.height)
        })
        .await
    }

    /// Fetch historical subtree roots for a shielded pool.
    pub async fn get_subtree_roots(
        &self,
        start_index: u32,
        shielded_protocol: ShieldedProtocol,
        max_entries: u32,
    ) -> Result<Vec<SubtreeRoot>> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;
            let mut request = tonic::Request::new(GetSubtreeRootsArg {
                start_index,
                shielded_protocol: shielded_protocol as i32,
                max_entries,
            });
            request.set_timeout(self.config.request_timeout);

            let mut stream = client.get_subtree_roots(request).await?.into_inner();
            let mut roots = Vec::new();
            let mut previous_height = None;
            while let Some(root) = stream.message().await? {
                let expected_index = u64::from(start_index) + roots.len() as u64;
                validate_received_subtree_root(
                    &root,
                    expected_index,
                    previous_height,
                    max_entries,
                    roots.len(),
                )?;
                previous_height = Some(root.completing_block_height);
                roots.push(root);
            }
            Ok(roots)
        })
        .await
    }

    /// Get a single block by height
    pub async fn get_block(&self, height: u32) -> Result<CompactBlock> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let request = tonic::Request::new(BlockId {
                height: height as u64,
                hash: Vec::new(),
            });

            let response = client.get_block(request).await?;
            Ok(CompactBlock::from(response.into_inner()))
        })
        .await
    }

    /// Execute operation with retry logic
    async fn with_retry<F, Fut, T>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T>> + Send,
    {
        let mut attempt = 0;
        let mut backoff = self.config.retry.initial_backoff;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    // Cancellation should return immediately (no retries/backoff).
                    if matches!(e, Error::Cancelled) {
                        return Err(e);
                    }

                    // Certain gRPC status codes are deterministic and should not be retried.
                    if Self::is_non_retryable_error(&e) {
                        return Err(e);
                    }

                    attempt += 1;
                    if attempt >= self.config.retry.max_attempts {
                        return Err(e);
                    }

                    warn!(
                        "Operation failed (attempt {}), retrying in {:?}: {:?}",
                        attempt, backoff, e
                    );

                    tokio::time::sleep(jitter_duration(backoff)).await;

                    backoff = std::cmp::min(
                        Duration::from_millis(
                            (backoff.as_millis() as f64 * self.config.retry.backoff_multiplier)
                                as u64,
                        ),
                        self.config.retry.max_backoff,
                    );
                }
            }
        }
    }
}

impl Clone for LightClient {
    fn clone(&self) -> Self {
        // Clone shares the existing channel to avoid reconnect races.
        Self {
            config: self.config.clone(),
            channel: Arc::clone(&self.channel),
            endpoint_pool: Arc::clone(&self.endpoint_pool),
            subtree_root_capabilities: Arc::clone(&self.subtree_root_capabilities),
        }
    }
}

/// Extract hostname from URL
fn extract_host(url: &str) -> Option<String> {
    // Simple extraction: strip protocol and port
    let without_proto = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    without_proto.split(':').next().map(|s| s.to_string())
}

fn extract_port(url: &str) -> Option<u16> {
    let without_proto = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (_, port_str) = without_proto.rsplit_once(':')?;
    port_str.parse::<u16>().ok()
}

fn normalize_spki_pin(pin: &str) -> &str {
    pin.trim().strip_prefix("sha256/").unwrap_or(pin.trim())
}

/// Compute transaction ID from raw transaction bytes
fn compute_txid(raw_tx: &[u8]) -> String {
    // Chain txid is double SHA256 of the tx, reversed
    use sha2::{Digest, Sha256};

    let hash1 = Sha256::digest(raw_tx);
    let hash2 = Sha256::digest(hash1);

    // Reverse bytes for display
    let mut txid_bytes: [u8; 32] = hash2.into();
    txid_bytes.reverse();

    hex::encode(txid_bytes)
}

fn validate_received_subtree_root(
    root: &SubtreeRoot,
    expected_index: u64,
    previous_height: Option<u64>,
    max_entries: u32,
    received_count: usize,
) -> Result<()> {
    if max_entries != 0 && received_count >= max_entries as usize {
        return Err(Error::Network(format!(
            "Lightwalletd returned more than the requested {} subtree roots",
            max_entries
        )));
    }
    if root.root_hash.len() != 32 {
        return Err(Error::Network(format!(
            "Subtree root at expected index {} is {} bytes, expected 32",
            expected_index,
            root.root_hash.len()
        )));
    }
    if root.completing_block_hash.len() != 32 {
        return Err(Error::Network(format!(
            "Completing block hash at expected subtree index {} is {} bytes, expected 32",
            expected_index,
            root.completing_block_hash.len()
        )));
    }
    if let Some(previous_height) = previous_height {
        if root.completing_block_height <= previous_height {
            return Err(Error::Network(format!(
                "Subtree completion height {} at expected index {} is not greater than previous height {}",
                root.completing_block_height, expected_index, previous_height
            )));
        }
    }
    Ok(())
}

// ============================================================================
// Legacy types for compatibility
// ============================================================================

/// Legacy compact block type (for backward compatibility)
pub type CompactBlockData = CompactBlock;

/// Legacy compact output type (alias for backward compatibility)
pub type CompactOutput = CompactSaplingOutput;

/// Transaction status
#[derive(Debug, Clone)]
pub struct TransactionStatus {
    /// Transaction ID
    pub txid: String,
    /// Block height (None if in mempool)
    pub height: Option<u64>,
    /// Number of confirmations
    pub confirmations: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_subtree_root(height: u64) -> SubtreeRoot {
        SubtreeRoot {
            root_hash: vec![1; 32],
            completing_block_hash: vec![2; 32],
            completing_block_height: height,
        }
    }

    #[test]
    fn validates_received_subtree_roots() {
        validate_received_subtree_root(&valid_subtree_root(100), 5, None, 0, 0)
            .expect("valid first subtree root");
        validate_received_subtree_root(&valid_subtree_root(200), 6, Some(100), 2, 1)
            .expect("valid second subtree root");
    }

    #[test]
    fn rejects_malformed_received_subtree_roots() {
        let mut short_root = valid_subtree_root(100);
        short_root.root_hash.pop();
        let mut short_block_hash = valid_subtree_root(100);
        short_block_hash.completing_block_hash.pop();

        let cases = [
            (short_root, None, 0, 0, "is 31 bytes, expected 32"),
            (short_block_hash, None, 0, 0, "is 31 bytes, expected 32"),
            (
                valid_subtree_root(100),
                Some(100),
                0,
                1,
                "is not greater than previous height",
            ),
            (
                valid_subtree_root(99),
                Some(100),
                0,
                1,
                "is not greater than previous height",
            ),
            (
                valid_subtree_root(200),
                Some(100),
                1,
                1,
                "more than the requested 1 subtree roots",
            ),
        ];

        for (root, previous_height, max_entries, received_count, expected_error) in cases {
            let err = validate_received_subtree_root(
                &root,
                5 + received_count as u64,
                previous_height,
                max_entries,
                received_count,
            )
            .expect_err("malformed subtree root was accepted");
            assert!(
                err.to_string().contains(expected_error),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn test_default_config() {
        let config = LightClientConfig::default();
        assert_eq!(config.endpoint, DEFAULT_LIGHTD_URL);
        assert_eq!(config.tls.enabled, DEFAULT_LIGHTD_USE_TLS);
        assert_eq!(config.tls.spki_pin, None);
        assert_eq!(config.transport, TransportMode::Tor);
    }

    #[test]
    fn test_direct_config() {
        let config = LightClientConfig::direct("https://custom:9067");
        assert_eq!(config.endpoint, "https://custom:9067");
        assert_eq!(config.transport, TransportMode::Direct);
    }

    #[test]
    fn test_compact_block_range_timeouts_scale_for_slow_networks() {
        let default_timeout = Duration::from_secs(120);
        let (direct_first, direct_next, direct_request) =
            compact_block_range_timeouts(TransportMode::Direct, 2_000, default_timeout);
        assert_eq!(direct_first, Duration::from_secs(60));
        assert_eq!(direct_next, Duration::from_secs(30));
        assert!(direct_request > default_timeout);

        let (tor_first, tor_next, tor_request) =
            compact_block_range_timeouts(TransportMode::Tor, 2_000, default_timeout);
        assert_eq!(tor_first, Duration::from_secs(120));
        assert_eq!(tor_next, Duration::from_secs(60));
        assert!(tor_request > direct_request);
    }

    #[test]
    fn test_socks5_config() {
        let config =
            LightClientConfig::with_socks5("https://lightd:9067", "socks5://127.0.0.1:9050");
        assert_eq!(config.transport, TransportMode::Socks5);
        assert_eq!(
            config.socks5_url,
            Some("socks5://127.0.0.1:9050".to_string())
        );
    }

    #[test]
    fn test_parse_socks5_url_decodes_credentials() {
        let parsed =
            parse_socks5_url("socks5://user%40name:pa%3Ass@proxy.example.com:1080").unwrap();
        assert_eq!(parsed.host, "proxy.example.com");
        assert_eq!(parsed.port, 1080);
        assert_eq!(parsed.username.as_deref(), Some("user@name"));
        assert_eq!(parsed.password.as_deref(), Some("pa:ss"));
    }

    #[test]
    fn test_parse_socks5_url_rejects_bad_scheme() {
        let err =
            parse_socks5_url("http://proxy.example.com:1080").expect_err("expected invalid scheme");
        assert!(
            format!("{}", err).contains("Unsupported SOCKS5 URL scheme"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn test_spki_pin_config() {
        let config = LightClientConfig::default()
            .with_spki_pin("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
        assert_eq!(
            config.tls.spki_pin,
            Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string())
        );
    }

    #[test]
    fn failover_inherits_transport_and_keeps_its_own_spki_pin() {
        let primary_pin = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let alternate_pin = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=";
        let config = LightClientConfig::with_socks5(
            "https://primary.example:443",
            "socks5://127.0.0.1:9050",
        )
        .with_spki_pin(primary_pin)
        .with_failover_endpoint(
            LightClientEndpoint::new("https://alternate.example:443").with_spki_pin(alternate_pin),
        );
        let client = LightClient::with_config(config);
        let alternate = client.candidate_client(1).expect("alternate client");

        assert_eq!(alternate.config.transport, TransportMode::Socks5);
        assert_eq!(
            alternate.config.socks5_url.as_deref(),
            Some("socks5://127.0.0.1:9050")
        );
        assert_eq!(
            alternate.config.tls.spki_pin.as_deref(),
            Some(alternate_pin)
        );
        assert!(alternate.config.failover_endpoints.is_empty());
    }

    #[test]
    fn failover_candidates_retain_private_transport_selection() {
        for transport in [
            TransportMode::Tor,
            TransportMode::I2p,
            TransportMode::Socks5,
        ] {
            let config = LightClientConfig {
                transport,
                socks5_url: (transport == TransportMode::Socks5)
                    .then(|| "socks5://127.0.0.1:9050".to_string()),
                allow_direct_fallback: false,
                ..LightClientConfig::default()
            }
            .with_failover_endpoint(LightClientEndpoint::new("https://alternate.example:443"));
            let client = LightClient::with_config(config);
            let alternate = client.candidate_client(1).expect("alternate client");

            assert_eq!(alternate.config.transport, transport);
            assert_eq!(alternate.config.socks5_url, client.config.socks5_url);
            assert!(!alternate.config.allow_direct_fallback);
        }
    }

    #[test]
    fn test_client_creation() {
        let client = LightClient::new(DEFAULT_LIGHTD_URL.to_string());
        assert!(!client.is_connected());
        assert_eq!(client.endpoint(), DEFAULT_LIGHTD_URL);
    }

    #[test]
    fn test_retry_config() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_secs(1),
            backoff_multiplier: 2.0,
        };

        let client = LightClient::with_retry_config(DEFAULT_LIGHTD_URL.to_string(), config);
        assert_eq!(client.config.retry.max_attempts, 3);
    }

    #[test]
    fn test_extract_host() {
        assert_eq!(
            extract_host("https://lightd1.piratechain.com:9067"),
            Some("lightd1.piratechain.com".to_string())
        );
        assert_eq!(
            extract_host("http://localhost:9067"),
            Some("localhost".to_string())
        );
        assert_eq!(
            extract_host("example.com:9067"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_extract_port() {
        assert_eq!(extract_port("https://lightd1.pirate.black:443"), Some(443));
        assert_eq!(extract_port("http://localhost:9067"), Some(9067));
        assert_eq!(extract_port("example.com:1234"), Some(1234));
    }

    #[test]
    fn test_normalize_spki_pin() {
        assert_eq!(
            normalize_spki_pin("sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );
        assert_eq!(
            normalize_spki_pin("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
        );
    }

    #[test]
    fn test_compute_txid() {
        // Test with a simple payload
        let raw_tx = vec![1, 2, 3, 4, 5];
        let txid = compute_txid(&raw_tx);
        assert_eq!(txid.len(), 64); // 32 bytes hex
    }

    #[test]
    fn test_transport_mode_privacy() {
        assert!(TransportMode::Tor.is_private());
        assert!(TransportMode::I2p.is_private());
        assert!(TransportMode::Socks5.is_private());
        assert!(!TransportMode::Direct.is_private());
    }

    #[tokio::test]
    async fn test_get_block_range_empty() {
        let client = LightClient::new(DEFAULT_LIGHTD_URL.to_string());
        // Empty range should return empty vec without connecting
        let blocks = client.get_compact_block_range(100..100).await.unwrap();
        assert!(blocks.is_empty());
    }
}

// ============================================================================
// Feature-gated integration tests
// ============================================================================

#[cfg(all(test, feature = "live_lightd"))]
mod integration_tests {
    use super::*;

    /// Test against live lightwalletd endpoint
    /// Run with: cargo test --features live_lightd -- --ignored
    #[tokio::test]
    #[ignore = "Requires live network connection"]
    async fn test_live_get_latest_block() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);

        client.connect().await.expect("Failed to connect");

        let height = client
            .get_latest_block()
            .await
            .expect("Failed to get latest block");

        // Pirate Chain mainnet should be well past block 1M
        assert!(height > 1_000_000, "Block height {} seems too low", height);

        println!("Latest block height: {}", height);
    }

    /// Test streaming compact blocks from live server
    #[tokio::test]
    #[ignore = "Requires live network connection"]
    async fn test_live_get_block_range() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);

        client.connect().await.expect("Failed to connect");

        // Get latest block first
        let latest = client
            .get_latest_block()
            .await
            .expect("Failed to get latest block");

        // Request last 10 blocks
        let start = latest.saturating_sub(10) as u32;
        let end = latest as u32;

        let blocks = client
            .get_compact_block_range(start..end)
            .await
            .expect("Failed to get block range");

        assert!(!blocks.is_empty(), "Should receive at least one block");
        assert_eq!(
            blocks.len(),
            (end - start) as usize,
            "Should receive requested blocks"
        );

        // Verify blocks are in order
        for (i, block) in blocks.iter().enumerate() {
            assert_eq!(block.height, (start as u64) + i as u64);
        }

        println!("Received {} blocks from {}..{}", blocks.len(), start, end);
    }

    /// Test getting server info
    #[tokio::test]
    #[ignore = "Requires live network connection"]
    async fn test_live_get_lightd_info() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        let client = LightClient::with_config(config);

        client.connect().await.expect("Failed to connect");

        let info = client
            .get_lightd_info()
            .await
            .expect("Failed to get server info");

        println!("Server: {} v{}", info.vendor, info.version);
        println!("Chain: {}", info.chain_name);
        println!("Block height: {}", info.block_height);
        println!("Sapling activation: {}", info.sapling_activation_height);

        assert!(!info.version.is_empty());
        assert!(info.block_height > 0);
    }
}

// ============================================================================
// Mock server tests
// ============================================================================

#[cfg(test)]
mod mock_tests {
    use super::*;

    /// Mock compact block for testing
    fn mock_compact_block(height: u64) -> CompactBlock {
        CompactBlock {
            proto_version: 1,
            height,
            hash: vec![0u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1234567890,
            header: vec![0u8; 32],
            transactions: vec![],
        }
    }

    /// Test pagination logic with mock data
    #[tokio::test]
    async fn test_block_range_pagination() {
        // Simulate fetching blocks in batches
        let batch_size = 10u64;
        let start = 1000u64;
        let end = 1035u64;

        let mut all_blocks = Vec::new();
        let mut current = start;

        while current <= end {
            let batch_end = std::cmp::min(current + batch_size, end + 1);

            // Simulate fetching a batch
            let batch: Vec<CompactBlock> = (current..batch_end).map(mock_compact_block).collect();

            all_blocks.extend(batch);
            current = batch_end;
        }

        // Verify we got all blocks
        assert_eq!(all_blocks.len(), (end - start + 1) as usize);

        // Verify ordering
        for (i, block) in all_blocks.iter().enumerate() {
            assert_eq!(block.height, start + i as u64);
        }
    }

    /// Test that batching handles edge cases
    #[tokio::test]
    async fn test_batch_edge_cases() {
        // Batch size exactly divides range
        let blocks: Vec<CompactBlock> = (0..20).map(mock_compact_block).collect();
        assert_eq!(blocks.len(), 20);

        // Single block range
        let single: Vec<CompactBlock> = (100..101).map(mock_compact_block).collect();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].height, 100);

        // Empty range
        let empty: Vec<CompactBlock> = (100..100).map(mock_compact_block).collect();
        assert!(empty.is_empty());
    }

    /// Test compact block conversion from proto
    #[test]
    fn test_compact_block_conversion() {
        let proto_block = proto::CompactBlock {
            proto_version: 1,
            height: 12345,
            hash: vec![1, 2, 3, 4],
            prev_hash: vec![9, 9, 9, 9],
            time: 1700000000,
            header: vec![7, 7, 7, 7],
            vtx: vec![proto::CompactTx {
                index: 0,
                hash: vec![5, 6, 7, 8],
                fee: 1000,
                spends: vec![proto::CompactSaplingSpend { nf: vec![0u8; 32] }],
                outputs: vec![proto::CompactSaplingOutput {
                    cmu: vec![0u8; 32],
                    ephemeral_key: vec![0u8; 32],
                    ciphertext: vec![0u8; 52],
                }],
                actions: vec![],
            }],
        };

        let block = CompactBlock::from(proto_block);

        assert_eq!(block.proto_version, 1);
        assert_eq!(block.height, 12345);
        assert_eq!(block.hash, vec![1, 2, 3, 4]);
        assert_eq!(block.prev_hash, vec![9, 9, 9, 9]);
        assert_eq!(block.time, 1700000000);
        assert_eq!(block.header, vec![7, 7, 7, 7]);
        assert_eq!(block.transactions.len(), 1);
        assert_eq!(block.transactions[0].outputs.len(), 1);
        assert_eq!(block.transactions[0].spends.len(), 1);
    }
}
