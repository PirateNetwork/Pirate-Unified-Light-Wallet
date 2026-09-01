//! Lightwalletd gRPC client with Tor routing and TLS pinning
//!
//! Provides connection to lightwalletd servers with:
//! - Tor routing by default via pirate-net
//! - TLS with optional SPKI certificate pinning
//! - Retry logic with exponential backoff
//! - Compact block streaming

use crate::ordered_stream::{OrderedBlockAssembler, OrderedBlockChunk};
use crate::proto_types as proto;
use crate::{Error, Result};
use futures_util::stream::{FuturesUnordered, StreamExt};
use once_cell::sync::Lazy;
use percent_encoding::percent_decode_str;
use pirate_net::{
    DnsConfig as NetDnsConfig, I2pConfig as NetI2pConfig, Socks5Config as NetSocks5Config,
    TorBridgeConfig, TorBridgeTransport, TorConfig as NetTorConfig,
    TransportConfig as NetTransportConfig, TransportManager as NetTransportManager,
    TransportMode as NetTransportMode,
};
use prost::Message;
use rand::Rng;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Notify, OwnedSemaphorePermit, RwLock, Semaphore};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tracing::{debug, error, info, warn};

use proto::compact_tx_streamer_client::CompactTxStreamerClient;
use proto::{
    BlockId, BlockRange, ChainSpec, Empty, GetSubtreeRootsArg, RawTransaction, ShieldedProtocol,
    SubtreeRoot, TxFilter,
};

/// Default official Pirate Chain mainnet endpoint.
pub const DEFAULT_LIGHTD_HOST: &str = "lightd1.pirate.black";
/// Default lightwalletd port
pub const DEFAULT_LIGHTD_PORT: u16 = 443;
/// Default TLS usage for the default endpoint
pub const DEFAULT_LIGHTD_USE_TLS: bool = true;
/// Default SPKI pin for the official lightwalletd endpoint.
pub const DEFAULT_LIGHTD_SPKI_PIN: &str = "";
/// Default endpoint URL
pub const DEFAULT_LIGHTD_URL: &str = "https://lightd1.pirate.black:443";

/// Curated Pirate Chain mainnet servers eligible for automatic historical sync.
///
/// Every candidate is still probed through the selected transport and must
/// match the canonical chain metadata and block hash before it receives work.
pub const MAINNET_AUTO_LIGHTD_URLS: &[&str] = &[
    DEFAULT_LIGHTD_URL,
    "https://lightwalletd1.cryptoforge.cc:443",
    "https://lightwalletd2.cryptoforge.cc:443",
    "https://pirate.mathnodes.com:443",
    "https://arrr.qortal.link:443",
    "https://arrr2.qortal.link:443",
    "https://arrr3.qortal.link:443",
];

const HISTORICAL_STRIPE_BLOCKS: u64 = 256;
const HISTORICAL_STRIPE_MIN_BLOCKS: u64 = HISTORICAL_STRIPE_BLOCKS * 2;
const HISTORICAL_STRIPE_TIP_MARGIN: u64 = 100;
const HISTORICAL_STRIPE_MAX_TIP_LAG: u64 = 24;
const HISTORICAL_STRIPE_MAX_SOURCES: usize = 3;
const HISTORICAL_STRIPE_HANDOFF_BYTES: u64 = 4 * 1024 * 1024;
const HISTORICAL_STRIPE_SOURCE_FAILURES: u32 = 2;
const ENDPOINT_POOL_TIP_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const COMPACT_CACHE_MAX_NODE_LAG: u64 = 24;

fn write_endpoint_pool_debug_event(id: &str, message: &str, data: &str) {
    pirate_core::debug_log::with_locked_file(|file| {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"{}","timestamp":{},"location":"client.rs:endpoint_pool","message":"{}","data":{},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
            id, timestamp, message, data
        );
    });
}

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
    manager: Arc<RwLock<Option<Arc<NetTransportManager>>>>,
    initialization: Arc<Mutex<()>>,
}

impl GlobalTransportState {
    async fn get_or_init(
        self: Arc<Self>,
        requested: NetTransportConfig,
    ) -> Result<Arc<NetTransportManager>> {
        // Never satisfy an old request by silently routing it through the new
        // transport. The endpoint and transport are one privacy decision: a
        // stale I2P request must be cancelled, not sent directly (or via Tor).
        if desired_transport_config()
            .as_ref()
            .is_some_and(|desired| desired != &requested)
        {
            return Err(Error::Cancelled);
        }
        let config = requested.clone();
        let existing = {
            let guard = Arc::clone(&self.manager).read_owned().await;
            guard.as_ref().map(Arc::clone)
        };
        if let Some(manager) = existing {
            Arc::clone(&manager)
                .update_config(config)
                .await
                .map_err(map_net_error)?;
            if desired_transport_config()
                .as_ref()
                .is_some_and(|desired| desired != &requested)
            {
                return Err(Error::Cancelled);
            }
            return Ok(manager);
        }

        // Constructing a manager can start native transports. Serialize the
        // empty-state path so concurrent bootstrap and connection requests
        // cannot launch separate embedded routers before either is published.
        let _initialization_guard = Arc::clone(&self.initialization).lock_owned().await;
        if desired_transport_config()
            .as_ref()
            .is_some_and(|desired| desired != &requested)
        {
            return Err(Error::Cancelled);
        }
        let config = requested.clone();
        if let Some(manager) = {
            let guard = Arc::clone(&self.manager).read_owned().await;
            guard.as_ref().map(Arc::clone)
        } {
            Arc::clone(&manager)
                .update_config(config)
                .await
                .map_err(map_net_error)?;
            if desired_transport_config()
                .as_ref()
                .is_some_and(|desired| desired != &requested)
            {
                return Err(Error::Cancelled);
            }
            return Ok(manager);
        }

        let created = Arc::new(
            NetTransportManager::new(config)
                .await
                .map_err(map_net_error)?,
        );
        *Arc::clone(&self.manager).write_owned().await = Some(Arc::clone(&created));
        if desired_transport_config()
            .as_ref()
            .is_some_and(|desired| desired != &requested)
        {
            return Err(Error::Cancelled);
        }
        Ok(created)
    }

    async fn get(self: Arc<Self>) -> Option<Arc<NetTransportManager>> {
        let manager = {
            let guard = Arc::clone(&self.manager).read_owned().await;
            guard.as_ref().map(Arc::clone)
        };
        manager
    }

    async fn get_matching(
        self: Arc<Self>,
        requested: NetTransportConfig,
    ) -> Option<Arc<NetTransportManager>> {
        if desired_transport_config()
            .as_ref()
            .is_some_and(|desired| *desired != requested)
        {
            return None;
        }
        let config = resolve_transport_config(requested);
        self.get()
            .await
            .filter(|manager| manager.matches_config(&config))
    }

    async fn shutdown(self: Arc<Self>) {
        let _initialization_guard = Arc::clone(&self.initialization).lock_owned().await;
        let manager = {
            let mut guard = Arc::clone(&self.manager).write_owned().await;
            let manager = guard.as_ref().map(Arc::clone);
            *guard = None;
            manager
        };
        if let Some(manager) = manager {
            manager.shutdown().await;
        }
    }
}

static GLOBAL_TRANSPORT: Lazy<Arc<GlobalTransportState>> = Lazy::new(|| {
    Arc::new(GlobalTransportState {
        manager: Arc::new(RwLock::new(None)),
        initialization: Arc::new(Mutex::new(())),
    })
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
    /// Total readiness and canonical-chain probe latency in milliseconds.
    pub latency_ms: Option<u64>,
    /// Diagnostic reason when the endpoint is unavailable or rejected.
    pub reason: Option<String>,
}

/// Client configuration
#[derive(Debug, Clone)]
pub struct LightClientConfig {
    /// Endpoint URL (e.g., "https://lightd1.pirate.black:443")
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

    /// Enable the curated Pirate Chain mainnet endpoint pool.
    ///
    /// The primary endpoint remains authoritative when available. Alternate
    /// endpoints inherit the selected transport, retain independent TLS host
    /// validation, and are rejected unless their metadata and canonical anchor
    /// match. A custom SPKI-pinned primary is intentionally left single-source.
    pub fn with_pirate_mainnet_auto_pool(mut self) -> Self {
        if self.transport == TransportMode::I2p
            || self.tls.spki_pin.is_some()
            || !self.failover_endpoints.is_empty()
            || !is_pirate_mainnet_auto_endpoint(&self.endpoint)
        {
            return self;
        }

        let primary = normalize_endpoint_identity(&self.endpoint);
        for endpoint in MAINNET_AUTO_LIGHTD_URLS {
            if normalize_endpoint_identity(endpoint) != primary {
                self.failover_endpoints
                    .push(LightClientEndpoint::new(*endpoint));
            }
        }
        self
    }
}

fn normalize_endpoint_identity(endpoint: &str) -> String {
    endpoint.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Whether an endpoint belongs to the curated Pirate Chain mainnet pool.
pub fn is_pirate_mainnet_auto_endpoint(endpoint: &str) -> bool {
    let endpoint = normalize_endpoint_identity(endpoint);
    MAINNET_AUTO_LIGHTD_URLS
        .iter()
        .any(|candidate| normalize_endpoint_identity(candidate) == endpoint)
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
    let manager = GLOBAL_TRANSPORT.clone().get_or_init(config).await?;
    manager.ensure_ready().await.map_err(map_net_error)?;
    Ok(())
}

/// Get current Tor status if transport manager is initialized.
pub async fn tor_status() -> Option<pirate_net::TorStatus> {
    let manager = GLOBAL_TRANSPORT.clone().get().await?;
    manager.tor_status().await
}

/// Rotate Tor exit circuits by isolating future streams.
pub async fn rotate_tor_exit() -> Result<()> {
    let manager = GLOBAL_TRANSPORT
        .clone()
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
    let manager = GLOBAL_TRANSPORT.clone().get_or_init(config).await?;
    let server_name = server_name.unwrap_or_else(|| host.to_string());
    manager
        .fetch_spki_pin(host.to_string(), port, server_name)
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
    let manager = GLOBAL_TRANSPORT.clone().get_or_init(config).await?;
    manager
        .fetch_url_bytes(&url, &headers)
        .await
        .map_err(map_net_error)
}

/// Get current I2P status if transport manager is initialized.
pub async fn i2p_status() -> Option<pirate_net::I2pStatus> {
    let manager = GLOBAL_TRANSPORT.clone().get().await?;
    manager.i2p_status().await
}

/// Shutdown any active transport manager.
pub async fn shutdown_transport() {
    clear_desired_transport_config();
    GLOBAL_TRANSPORT.clone().shutdown().await;
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

impl CompactBlock {
    pub(crate) fn shielded_work_items(
        &self,
        sapling_work_factor: u64,
        ironwood_work_factor: u64,
    ) -> u64 {
        self.transactions.iter().fold(0u64, |total, tx| {
            total
                .saturating_add(
                    (tx.outputs.len() as u64).saturating_mul(sapling_work_factor.max(1)),
                )
                .saturating_add(
                    (tx.actions.len() as u64).saturating_mul(ironwood_work_factor.max(1)),
                )
        })
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

/// A contiguous compact-block stream chunk bounded by protobuf wire bytes.
#[derive(Debug)]
pub struct CompactBlockChunk {
    /// Ordered compact blocks in this chunk.
    pub blocks: Vec<CompactBlock>,
    /// Exact encoded wire bytes for each corresponding block.
    pub encoded_block_bytes: Vec<u64>,
    /// Exact sum of protobuf `encoded_len()` values received from lightwalletd.
    pub encoded_bytes: u64,
    /// Endpoint that supplied this chunk.
    pub endpoint: String,
}

impl CompactBlockChunk {
    /// First block height in the chunk.
    pub fn start_height(&self) -> Option<u64> {
        self.blocks.first().map(|block| block.height)
    }

    /// Last block height in the chunk.
    pub fn end_height(&self) -> Option<u64> {
        self.blocks.last().map(|block| block.height)
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

async fn send_ordered_chunk(
    sender: &mpsc::Sender<Result<CompactBlockChunk>>,
    chunk: OrderedBlockChunk,
    endpoint: String,
) -> Result<()> {
    debug_assert_eq!(chunk.blocks.len(), chunk.encoded_block_bytes.len());
    sender
        .send(Ok(CompactBlockChunk {
            blocks: chunk.blocks,
            encoded_block_bytes: chunk.encoded_block_bytes,
            encoded_bytes: chunk.encoded_bytes,
            endpoint,
        }))
        .await
        .map_err(|_| Error::Cancelled)
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
    probe_latencies: HashMap<usize, Duration>,
    channels: HashMap<usize, Channel>,
    last_tip_refresh: Option<Instant>,
}

struct EndpointProbe {
    info: LightdInfo,
    tip: u64,
    channel: Channel,
    elapsed: Duration,
}

struct EndpointPoolProbeGuard {
    inflight: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Drop for EndpointPoolProbeGuard {
    fn drop(&mut self) {
        self.inflight.store(false, Ordering::Release);
        self.notify.notify_waiters();
    }
}

#[derive(Clone, Debug)]
struct HistoricalStripePlan {
    candidate_indices: Vec<usize>,
    end_exclusive: u64,
}

#[derive(Clone, Copy, Debug)]
struct StripeRange {
    start: u64,
    end_exclusive: u64,
    attempt: u32,
}

enum StripeEvent {
    Chunk {
        worker_index: usize,
        range: StripeRange,
        chunk: CompactBlockChunk,
        _permit: OwnedSemaphorePermit,
    },
    Complete {
        worker_index: usize,
    },
    Failed {
        worker_index: usize,
        range: StripeRange,
        resume_height: u64,
        error: Error,
    },
}

struct BufferedStripeChunk {
    chunk: CompactBlockChunk,
    _permit: OwnedSemaphorePermit,
}

#[derive(Default)]
struct StripeWorkerGuard {
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl StripeWorkerGuard {
    fn push(&mut self, handle: tokio::task::JoinHandle<()>) {
        self.handles.push(handle);
    }
}

impl Drop for StripeWorkerGuard {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

fn historical_source_buffer_bytes(max_buffer_bytes: u64, source_count: usize) -> u64 {
    let source_count = source_count.max(1) as u64;
    (max_buffer_bytes / source_count).clamp(1, HISTORICAL_STRIPE_HANDOFF_BYTES)
}

fn should_leave_historical_striping(
    range: StripeRange,
    resume_height: u64,
    worker_failures: u32,
    max_attempts: u32,
) -> bool {
    resume_height < range.end_exclusive
        && (range.attempt >= max_attempts.max(1)
            || worker_failures >= HISTORICAL_STRIPE_SOURCE_FAILURES)
}

fn should_probe_historical_pool(transport: TransportMode, start: u64, end_exclusive: u64) -> bool {
    transport != TransportMode::I2p
        && end_exclusive.saturating_sub(start) >= HISTORICAL_STRIPE_MIN_BLOCKS
}

fn preferred_active_endpoint(
    healthy_indices: &[usize],
    tips: &HashMap<usize, u64>,
    probe_latencies: &HashMap<usize, Duration>,
) -> Option<usize> {
    let highest_tip = healthy_indices
        .iter()
        .filter_map(|index| tips.get(index).copied())
        .max()?;
    if healthy_indices.contains(&0)
        && tips
            .get(&0)
            .is_some_and(|tip| tip.saturating_add(HISTORICAL_STRIPE_MAX_TIP_LAG) >= highest_tip)
    {
        return Some(0);
    }

    healthy_indices.iter().copied().max_by(|left, right| {
        tips.get(left)
            .copied()
            .unwrap_or_default()
            .cmp(&tips.get(right).copied().unwrap_or_default())
            .then_with(|| {
                probe_latencies
                    .get(right)
                    .copied()
                    .unwrap_or(Duration::MAX)
                    .cmp(&probe_latencies.get(left).copied().unwrap_or(Duration::MAX))
            })
    })
}

fn highest_tip_endpoint(
    healthy_indices: &[usize],
    tips: &HashMap<usize, u64>,
    probe_latencies: &HashMap<usize, Duration>,
) -> Option<(usize, u64)> {
    healthy_indices
        .iter()
        .filter_map(|index| tips.get(index).copied().map(|tip| (*index, tip)))
        .max_by(|(left_index, left_tip), (right_index, right_tip)| {
            left_tip.cmp(right_tip).then_with(|| {
                probe_latencies
                    .get(right_index)
                    .copied()
                    .unwrap_or(Duration::MAX)
                    .cmp(
                        &probe_latencies
                            .get(left_index)
                            .copied()
                            .unwrap_or(Duration::MAX),
                    )
            })
        })
}

fn eligible_candidate_order(state: &EndpointPoolState, minimum_tip: u64) -> Vec<usize> {
    let has_validated_pool = state.probed && !state.healthy_indices.is_empty();
    let mut candidates = if has_validated_pool {
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
    if has_validated_pool {
        candidates.retain(|index| state.tips.get(index).is_some_and(|tip| *tip >= minimum_tip));
    }
    candidates
}

fn validate_compact_cache_tip(
    info: &proto::LightdInfo,
    advertised_tip: &BlockId,
    compact_tip: &proto::CompactBlock,
) -> Result<()> {
    if advertised_tip.height == 0 {
        return Err(Error::Connection(
            "lightwalletd compact cache reported an empty tip".to_string(),
        ));
    }
    if advertised_tip.hash.len() != 32 {
        return Err(Error::Connection(format!(
            "lightwalletd compact-cache tip hash is {} bytes, expected 32",
            advertised_tip.hash.len()
        )));
    }

    let reported_network_height = info.block_height.max(info.estimated_height);
    if reported_network_height > 0
        && advertised_tip
            .height
            .saturating_add(COMPACT_CACHE_MAX_NODE_LAG)
            < reported_network_height
    {
        return Err(Error::Connection(format!(
            "lightwalletd compact cache is not ready: tip {} trails reported network height {}",
            advertised_tip.height, reported_network_height
        )));
    }
    if compact_tip.height != advertised_tip.height {
        return Err(Error::Connection(format!(
            "lightwalletd returned compact block {} for advertised tip {}",
            compact_tip.height, advertised_tip.height
        )));
    }
    if compact_tip.hash.len() != 32 {
        return Err(Error::Connection(format!(
            "lightwalletd compact block hash is {} bytes, expected 32",
            compact_tip.hash.len()
        )));
    }
    if compact_tip.hash != advertised_tip.hash {
        return Err(Error::Connection(
            "lightwalletd compact block does not match its advertised tip hash".to_string(),
        ));
    }
    if compact_tip.prev_hash.len() != 32 {
        return Err(Error::Connection(format!(
            "lightwalletd compact block previous hash is {} bytes, expected 32",
            compact_tip.prev_hash.len()
        )));
    }

    Ok(())
}

/// Lightwalletd gRPC client.
///
/// Provides tip queries, bounded compact-block streams, endpoint failover, and
/// transaction broadcast through the configured privacy transport.
pub struct LightClient {
    config: LightClientConfig,
    channel: Arc<Mutex<Option<Channel>>>,
    endpoint_pool: Arc<RwLock<EndpointPoolState>>,
    endpoint_pool_probe_inflight: Arc<AtomicBool>,
    endpoint_pool_probe_notify: Arc<Notify>,
    subtree_root_capabilities: Arc<StdRwLock<HashMap<(String, i32), SubtreeRootCapability>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubtreeRootCapability {
    Available,
    RetryAfter(Instant),
}

const SUBTREE_ROOT_TRANSIENT_RETRY: Duration = Duration::from_secs(60);
const SUBTREE_ROOT_TIMEOUT_RETRY: Duration = Duration::from_secs(10 * 60);
const SUBTREE_ROOT_UNSUPPORTED_RETRY: Duration = Duration::from_secs(24 * 60 * 60);
const REDUNDANT_BROADCAST_MAX_ALTERNATES: usize = 2;

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
    /// Default: uses the TLS-enabled DEFAULT_LIGHTD_URL via Tor.
    pub fn new(endpoint: String) -> Self {
        Self {
            config: LightClientConfig {
                endpoint,
                ..Default::default()
            },
            channel: Arc::new(Mutex::new(None)),
            endpoint_pool: Arc::new(RwLock::new(EndpointPoolState::default())),
            endpoint_pool_probe_inflight: Arc::new(AtomicBool::new(false)),
            endpoint_pool_probe_notify: Arc::new(Notify::new()),
            subtree_root_capabilities: Arc::new(StdRwLock::new(HashMap::new())),
        }
    }

    /// Create client with custom configuration
    pub fn with_config(config: LightClientConfig) -> Self {
        Self {
            config,
            channel: Arc::new(Mutex::new(None)),
            endpoint_pool: Arc::new(RwLock::new(EndpointPoolState::default())),
            endpoint_pool_probe_inflight: Arc::new(AtomicBool::new(false)),
            endpoint_pool_probe_notify: Arc::new(Notify::new()),
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
            endpoint_pool_probe_inflight: Arc::new(AtomicBool::new(false)),
            endpoint_pool_probe_notify: Arc::new(Notify::new()),
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

    pub(crate) async fn endpoint_pool_is_probed(&self) -> bool {
        Arc::clone(&self.endpoint_pool).read_owned().await.probed
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
            endpoint_pool_probe_inflight: Arc::clone(&self.endpoint_pool_probe_inflight),
            endpoint_pool_probe_notify: Arc::clone(&self.endpoint_pool_probe_notify),
            subtree_root_capabilities: Arc::clone(&self.subtree_root_capabilities),
        })
    }

    async fn connected_candidate_client(&self, index: usize) -> Option<Self> {
        let candidate = self.endpoint_candidate(index)?;
        let pooled_channel = Arc::clone(&self.endpoint_pool)
            .read_owned()
            .await
            .channels
            .get(&index)
            .cloned();
        let channel = match pooled_channel {
            Some(channel) => channel,
            None if index == 0 => Arc::clone(&self.channel).lock_owned().await.clone()?,
            None => return None,
        };
        let mut config = self.config.clone();
        config.endpoint = candidate.endpoint;
        config.tls = candidate.tls;
        config.failover_endpoints.clear();
        config.retry.max_attempts = 1;
        Some(Self {
            config,
            channel: Arc::new(Mutex::new(Some(channel))),
            endpoint_pool: Arc::clone(&self.endpoint_pool),
            endpoint_pool_probe_inflight: Arc::clone(&self.endpoint_pool_probe_inflight),
            endpoint_pool_probe_notify: Arc::clone(&self.endpoint_pool_probe_notify),
            subtree_root_capabilities: Arc::clone(&self.subtree_root_capabilities),
        })
    }

    fn endpoint_count(&self) -> usize {
        1usize.saturating_add(self.config.failover_endpoints.len())
    }

    async fn probe_candidate(config: LightClientConfig) -> Result<(LightdInfo, u64, Channel)> {
        let channel = Self::try_connect_for_probe(config).await?;
        let (info, tip) = Self::probe_connected_candidate(channel.clone()).await?;
        Ok((info, tip, channel))
    }

    async fn probe_connected_candidate(channel: Channel) -> Result<(LightdInfo, u64)> {
        let mut client = CompactTxStreamerClient::new(channel.clone());
        let info = client
            .get_lightd_info(tonic::Request::new(Empty {}))
            .await?
            .into_inner();
        let tip = client
            .get_latest_block(tonic::Request::new(ChainSpec {
                network: String::new(),
            }))
            .await?
            .into_inner();
        let tip_block = client
            .get_block(tonic::Request::new(BlockId {
                height: tip.height,
                hash: Vec::new(),
            }))
            .await?
            .into_inner();
        validate_compact_cache_tip(&info, &tip, &tip_block)?;
        Ok((LightdInfo::from(info), tip.height))
    }

    async fn probe_candidate_anchor(channel: Channel, height: u32) -> Result<CompactBlock> {
        let mut client = CompactTxStreamerClient::new(channel);
        let response = client
            .get_block(tonic::Request::new(BlockId {
                height: u64::from(height),
                hash: Vec::new(),
            }))
            .await?;
        Ok(CompactBlock::from(response.into_inner()))
    }

    /// Probe configured endpoints through the selected transport and retain only
    /// candidates that match a canonical endpoint at a common chain anchor.
    pub async fn probe_endpoints(&self) -> Vec<EndpointHealth> {
        self.clone().probe_endpoints_owned(None).await
    }

    fn endpoint_probe_timeout(&self) -> Duration {
        match self.config.transport {
            TransportMode::Direct => Duration::from_secs(12),
            TransportMode::Tor | TransportMode::I2p | TransportMode::Socks5 => {
                Duration::from_secs(35)
            }
        }
    }

    fn report_endpoint_pool_health(health: &[EndpointHealth]) {
        let healthy = health.iter().filter(|endpoint| endpoint.healthy).count();
        write_endpoint_pool_debug_event(
            "log_endpoint_pool_validated",
            "endpoint pool validation completed",
            &format!(r#"{{"healthy":{},"total":{}}}"#, healthy, health.len()),
        );
        if healthy > 0 {
            info!(
                healthy,
                total = health.len(),
                "Validated canonical lightwalletd endpoint pool"
            );
        } else {
            warn!(
                total = health.len(),
                "No canonical lightwalletd alternate passed validation"
            );
        }
    }

    async fn start_endpoint_pool_probe(&self) -> bool {
        if !self.has_failover_endpoints() || self.endpoint_pool_is_probed().await {
            return false;
        }
        if self
            .endpoint_pool_probe_inflight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }

        // The pool may have completed between the first check and acquiring
        // ownership of the single-flight probe.
        if self.endpoint_pool_is_probed().await {
            self.endpoint_pool_probe_inflight
                .store(false, Ordering::Release);
            self.endpoint_pool_probe_notify.notify_waiters();
            return false;
        }

        let pool_client = self.clone();
        let guard = EndpointPoolProbeGuard {
            inflight: Arc::clone(&self.endpoint_pool_probe_inflight),
            notify: Arc::clone(&self.endpoint_pool_probe_notify),
        };
        tokio::spawn(async move {
            let _guard = guard;
            let health = pool_client.probe_endpoints_owned(None).await;
            Self::report_endpoint_pool_health(&health);
        });
        true
    }

    pub(crate) async fn start_historical_endpoint_pool_probe(
        &self,
        start: u64,
        end_exclusive: u64,
    ) -> bool {
        if !should_probe_historical_pool(self.config.transport, start, end_exclusive) {
            return false;
        }
        self.start_endpoint_pool_probe().await
    }

    pub(crate) async fn prepare_subtree_root_routing(&self) {
        if !self.has_failover_endpoints() || self.endpoint_pool_is_probed().await {
            return;
        }

        self.start_endpoint_pool_probe().await;
        self.wait_for_endpoint_pool_probe().await;
    }

    async fn wait_for_endpoint_pool_probe(&self) {
        let completion_timeout = self
            .endpoint_probe_timeout()
            .saturating_mul(2)
            .saturating_add(Duration::from_secs(1));
        let deadline = Instant::now() + completion_timeout;

        loop {
            let notified = self.endpoint_pool_probe_notify.notified();
            if self.endpoint_pool_is_probed().await
                || !self.endpoint_pool_probe_inflight.load(Ordering::Acquire)
            {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || tokio::time::timeout(remaining, notified).await.is_err() {
                warn!(
                    timeout = ?completion_timeout,
                    "Timed out waiting for lightwalletd endpoint pool validation"
                );
                return;
            }
        }
    }

    async fn probe_endpoints_owned(
        self,
        skipped_primary_reason: Option<String>,
    ) -> Vec<EndpointHealth> {
        let endpoint_count = self.endpoint_count();
        let probe_timeout = self.endpoint_probe_timeout();
        let mut probes: Vec<Option<EndpointProbe>> = std::iter::repeat_with(|| None)
            .take(endpoint_count)
            .collect();
        let mut health = (0..endpoint_count)
            .filter_map(|index| {
                self.endpoint_candidate(index)
                    .map(|candidate| EndpointHealth {
                        endpoint: candidate.endpoint,
                        healthy: false,
                        tip_height: None,
                        latency_ms: None,
                        reason: Some("endpoint has not completed validation".to_string()),
                    })
            })
            .collect::<Vec<_>>();
        if let Some(reason) = skipped_primary_reason.as_ref() {
            health[0].reason = Some(reason.clone());
        }
        let mut pending_probes = FuturesUnordered::new();
        for index in 0..endpoint_count {
            if index == 0 && skipped_primary_reason.is_some() {
                continue;
            }
            let Some(candidate) = self.candidate_client(index) else {
                continue;
            };
            pending_probes.push(async move {
                let started = Instant::now();
                let result =
                    tokio::time::timeout(probe_timeout, Self::probe_candidate(candidate.config))
                        .await;
                (index, started.elapsed(), result)
            });
        }
        while let Some((index, elapsed, result)) = pending_probes.next().await {
            match result {
                Ok(Ok((info, tip, channel))) => {
                    probes[index] = Some(EndpointProbe {
                        info,
                        tip,
                        channel,
                        elapsed,
                    });
                    health[index].tip_height = Some(tip);
                    health[index].reason = None;
                }
                Ok(Err(error)) => health[index].reason = Some(error.to_string()),
                Err(_) => {
                    health[index].reason =
                        Some(format!("health probe timed out after {:?}", probe_timeout));
                }
            }
        }

        let Some(reference_index) = probes
            .first()
            .is_some_and(Option::is_some)
            .then_some(0)
            .or_else(|| probes.iter().position(Option::is_some))
        else {
            let mut state = Arc::clone(&self.endpoint_pool).write_owned().await;
            state.probed = true;
            state.active_index = 0;
            state.healthy_indices.clear();
            state.tips.clear();
            state.probe_latencies.clear();
            state.channels.clear();
            state.last_tip_refresh = None;
            return health;
        };
        let (reference_info, reference_tip) = {
            let reference = probes[reference_index]
                .as_ref()
                .expect("reference endpoint exists");
            (reference.info.clone(), reference.tip)
        };

        let mut metadata_matches = vec![false; endpoint_count];
        for (index, probe) in probes.iter().enumerate() {
            let Some(probe) = probe else {
                continue;
            };
            let matches = probe
                .info
                .chain_name
                .eq_ignore_ascii_case(&reference_info.chain_name)
                && probe.info.sapling_activation_height == reference_info.sapling_activation_height
                && (probe.tip != reference_tip
                    || probe.info.consensus_branch_id == reference_info.consensus_branch_id);
            metadata_matches[index] = matches;
            if !matches {
                health[index].reason =
                    Some("server chain metadata differs from canonical reference".to_string());
            }
        }

        let common_anchor = probes
            .iter()
            .enumerate()
            .filter(|(index, _)| metadata_matches[*index])
            .filter_map(|(_, probe)| probe.as_ref().map(|probe| probe.tip))
            .min()
            .unwrap_or(reference_tip)
            .saturating_sub(10);
        let common_anchor_u32 = u32::try_from(common_anchor).unwrap_or(u32::MAX);
        let mut anchor_hashes: Vec<Option<Vec<u8>>> = std::iter::repeat_with(|| None)
            .take(endpoint_count)
            .collect();
        let mut pending_anchors = FuturesUnordered::new();
        for index in 0..endpoint_count {
            if !metadata_matches[index] {
                continue;
            }
            let Some(channel) = probes[index].as_ref().map(|probe| probe.channel.clone()) else {
                continue;
            };
            pending_anchors.push(async move {
                let started = Instant::now();
                let result = tokio::time::timeout(
                    probe_timeout,
                    Self::probe_candidate_anchor(channel, common_anchor_u32),
                )
                .await;
                (index, started.elapsed(), result)
            });
        }
        while let Some((index, elapsed, result)) = pending_anchors.next().await {
            if let Some(probe) = probes[index].as_mut() {
                probe.elapsed = probe.elapsed.saturating_add(elapsed);
            }
            match result {
                Ok(Ok(block)) => anchor_hashes[index] = Some(block.hash),
                Ok(Err(error)) => health[index].reason = Some(error.to_string()),
                Err(_) => {
                    health[index].reason = Some(format!(
                        "canonical anchor probe timed out after {:?}",
                        probe_timeout
                    ));
                }
            }
        }

        let canonical_anchor = if let Some(primary) = anchor_hashes.first().and_then(Clone::clone) {
            Some(primary)
        } else {
            let mut counts = HashMap::<Vec<u8>, usize>::new();
            for hash in anchor_hashes.iter().flatten() {
                *counts.entry(hash.clone()).or_default() += 1;
            }
            let responding = counts.values().sum::<usize>();
            counts
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .filter(|(_, count)| responding <= 1 || count.saturating_mul(2) > responding)
                .map(|(hash, _)| hash)
        };

        for index in 0..endpoint_count {
            if !metadata_matches[index] {
                continue;
            }
            match (&canonical_anchor, &anchor_hashes[index]) {
                (Some(canonical), Some(candidate)) if candidate == canonical => {
                    health[index].healthy = true;
                    health[index].reason = None;
                }
                (Some(_), Some(_)) => {
                    health[index].reason = Some(format!(
                        "server block hash differs at canonical height {}",
                        common_anchor
                    ));
                }
                (None, _) => {
                    health[index].reason = Some(format!(
                        "no majority block hash at canonical height {}",
                        common_anchor
                    ));
                }
                _ => {}
            }
        }

        let mut healthy_indices = health
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.healthy.then_some(index))
            .collect::<Vec<_>>();
        let tips = probes
            .iter()
            .enumerate()
            .filter_map(|(index, probe)| probe.as_ref().map(|probe| (index, probe.tip)))
            .collect::<HashMap<_, _>>();
        let probe_latencies = probes
            .iter()
            .enumerate()
            .filter_map(|(index, probe)| probe.as_ref().map(|probe| (index, probe.elapsed)))
            .collect::<HashMap<_, _>>();
        let active_index = preferred_active_endpoint(&healthy_indices, &tips, &probe_latencies);
        for (index, entry) in health.iter_mut().enumerate() {
            entry.latency_ms = probe_latencies
                .get(&index)
                .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
        }
        if let Some(channel) = active_index
            .and_then(|index| probes[index].as_ref())
            .map(|probe| probe.channel.clone())
        {
            *Arc::clone(&self.channel).lock_owned().await = Some(channel);
        }
        healthy_indices.sort_by_key(|index| {
            probes[*index]
                .as_ref()
                .map(|probe| probe.elapsed)
                .unwrap_or(Duration::MAX)
        });
        let mut state = Arc::clone(&self.endpoint_pool).write_owned().await;
        state.probed = true;
        state.active_index = active_index.unwrap_or(0);
        state.healthy_indices = healthy_indices;
        state.failures.clear();
        state.tips = tips;
        state.probe_latencies = probe_latencies;
        state.last_tip_refresh = Some(Instant::now());
        state.channels = probes
            .into_iter()
            .enumerate()
            .filter_map(|(index, probe)| {
                health
                    .get(index)
                    .is_some_and(|entry| entry.healthy)
                    .then(|| probe.map(|probe| (index, probe.channel)))
                    .flatten()
            })
            .collect();
        health
    }

    /// Return the endpoint currently selected by the validated pool.
    pub async fn active_endpoint(&self) -> String {
        let active_index = self.endpoint_pool.read().await.active_index;
        self.endpoint_candidate(active_index)
            .map(|candidate| candidate.endpoint)
            .unwrap_or_else(|| self.config.endpoint.clone())
    }

    fn endpoint_tip_refresh_timeout(&self) -> Duration {
        match self.config.transport {
            TransportMode::Direct => Duration::from_secs(3),
            TransportMode::Tor | TransportMode::Socks5 => Duration::from_secs(8),
            TransportMode::I2p => Duration::from_secs(12),
        }
    }

    async fn canonical_pool_tip(&self, force_refresh: bool) -> Option<u64> {
        if !self.has_failover_endpoints() {
            return None;
        }

        let now = Instant::now();
        let refresh_candidates = {
            let mut state = self.endpoint_pool.write().await;
            if !state.probed || state.healthy_indices.is_empty() {
                return None;
            }
            let refresh_due = force_refresh
                || state.last_tip_refresh.is_none_or(|last_refresh| {
                    now.saturating_duration_since(last_refresh)
                        >= ENDPOINT_POOL_TIP_REFRESH_INTERVAL
                });
            if refresh_due {
                state.last_tip_refresh = Some(now);
                state
                    .healthy_indices
                    .iter()
                    .filter_map(|index| {
                        state
                            .channels
                            .get(index)
                            .cloned()
                            .map(|channel| (*index, channel))
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };

        let refresh_attempted = !refresh_candidates.is_empty();
        let refresh_total = refresh_candidates.len();
        let mut observations = Vec::with_capacity(refresh_total);
        if refresh_attempted {
            let timeout = self.endpoint_tip_refresh_timeout();
            let mut pending = FuturesUnordered::new();
            for (index, channel) in refresh_candidates {
                pending.push(async move {
                    let mut client = CompactTxStreamerClient::new(channel);
                    let result = tokio::time::timeout(
                        timeout,
                        client.get_latest_block(tonic::Request::new(ChainSpec {
                            network: String::new(),
                        })),
                    )
                    .await;
                    (index, result)
                });
            }
            while let Some((index, result)) = pending.next().await {
                if let Ok(Ok(response)) = result {
                    observations.push((index, response.into_inner().height));
                }
            }

            if observations.is_empty() {
                write_endpoint_pool_debug_event(
                    "log_endpoint_pool_tip_refresh_failed",
                    "endpoint pool tip refresh returned no usable responses",
                    &format!(r#"{{"total":{}}}"#, refresh_total),
                );
                return None;
            }

            let mut state = self.endpoint_pool.write().await;
            let healthy_indices = state.healthy_indices.clone();
            for index in healthy_indices {
                state.tips.remove(&index);
            }
            for (index, tip) in &observations {
                if state.healthy_indices.contains(index) {
                    state.tips.insert(*index, *tip);
                }
            }
        }

        let selected = {
            let state = self.endpoint_pool.read().await;
            highest_tip_endpoint(&state.healthy_indices, &state.tips, &state.probe_latencies)
                .map(|(index, tip)| (tip, state.channels.get(&index).cloned()))
        };
        let (tip, channel) = selected?;
        if let Some(channel) = channel {
            *Arc::clone(&self.channel).lock_owned().await = Some(channel);
        }

        if refresh_attempted {
            write_endpoint_pool_debug_event(
                "log_endpoint_pool_tips_refreshed",
                "endpoint pool tips refreshed",
                &format!(
                    r#"{{"responded":{},"total":{},"highest":{}}}"#,
                    observations.len(),
                    refresh_total,
                    tip
                ),
            );
        }
        Some(tip)
    }

    async fn candidate_order(&self, minimum_tip: u64) -> Vec<usize> {
        let candidates = {
            let state = self.endpoint_pool.read().await;
            eligible_candidate_order(&state, minimum_tip)
        };
        if !candidates.is_empty() || !self.has_failover_endpoints() {
            return candidates;
        }

        // Endpoint validation proves chain identity, but its tip heights are
        // only snapshots. Refresh those already-validated channels when a new
        // target advances beyond the snapshot so the tail cannot deadlock one
        // block behind the chain.
        let _ = self.canonical_pool_tip(true).await;
        let state = self.endpoint_pool.read().await;
        eligible_candidate_order(&state, minimum_tip)
    }

    async fn historical_stripe_plan(
        &self,
        start: u64,
        end_exclusive: u64,
    ) -> Option<HistoricalStripePlan> {
        if self.config.transport == TransportMode::I2p
            || end_exclusive.saturating_sub(start) < HISTORICAL_STRIPE_MIN_BLOCKS
        {
            return None;
        }

        let state = self.endpoint_pool.read().await;
        if !state.probed || state.healthy_indices.len() < 2 {
            return None;
        }
        let highest_tip = state.tips.values().copied().max()?;
        let max_sources = match self.config.transport {
            TransportMode::Direct => HISTORICAL_STRIPE_MAX_SOURCES,
            TransportMode::Tor | TransportMode::Socks5 => 2,
            TransportMode::I2p => 1,
        };
        let mut candidate_indices = state
            .healthy_indices
            .iter()
            .copied()
            .filter(|index| {
                state.failures.get(index).copied().unwrap_or_default()
                    < HISTORICAL_STRIPE_SOURCE_FAILURES
            })
            .filter(|index| {
                state.tips.get(index).is_some_and(|tip| {
                    tip.saturating_add(HISTORICAL_STRIPE_MAX_TIP_LAG) >= highest_tip
                        && *tip >= start
                })
            })
            .collect::<Vec<_>>();
        candidate_indices.sort_by_key(|index| {
            state
                .probe_latencies
                .get(index)
                .copied()
                .unwrap_or(Duration::MAX)
        });
        candidate_indices.truncate(max_sources);
        if candidate_indices.len() < 2 {
            return None;
        }

        let stable_tip = candidate_indices
            .iter()
            .filter_map(|index| state.tips.get(index).copied())
            .min()?;
        let stable_end_exclusive = end_exclusive.min(
            stable_tip
                .saturating_sub(HISTORICAL_STRIPE_TIP_MARGIN)
                .saturating_add(1),
        );
        if stable_end_exclusive.saturating_sub(start) < HISTORICAL_STRIPE_MIN_BLOCKS {
            return None;
        }

        Some(HistoricalStripePlan {
            candidate_indices,
            end_exclusive: stable_end_exclusive,
        })
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
        if self.has_failover_endpoints() {
            let primary = self
                .candidate_client(0)
                .expect("the primary endpoint is always present");
            let primary_failure = match primary.clone().connect_single_endpoint().await {
                Ok(()) => {
                    let channel = Arc::clone(&primary.channel)
                        .lock_owned()
                        .await
                        .clone()
                        .ok_or_else(|| {
                            Error::Connection(
                                "connected primary lightwalletd endpoint has no channel"
                                    .to_string(),
                            )
                        })?;
                    let readiness_timeout = self.endpoint_probe_timeout();
                    match tokio::time::timeout(
                        readiness_timeout,
                        Self::probe_connected_candidate(channel.clone()),
                    )
                    .await
                    {
                        Ok(Ok((_, tip))) => {
                            *Arc::clone(&self.channel).lock_owned().await = Some(channel);
                            write_endpoint_pool_debug_event(
                                "log_endpoint_primary_ready",
                                "selected Auto endpoint passed compact-cache readiness",
                                &serde_json::json!({
                                    "endpoint": self.config.endpoint,
                                    "tip": tip,
                                })
                                .to_string(),
                            );
                            info!(
                                tip,
                                "Connected to ready lightwalletd endpoint {}; alternate validation is deferred until network streaming",
                                self.config.endpoint
                            );
                            return Ok(());
                        }
                        Ok(Err(error)) => error.to_string(),
                        Err(_) => format!(
                            "compact-cache readiness timed out after {:?}",
                            readiness_timeout
                        ),
                    }
                }
                Err(error) => error.to_string(),
            };

            warn!(
                reason = %primary_failure,
                "Selected lightwalletd endpoint {} is not ready; probing canonical alternates",
                self.config.endpoint,
            );
            write_endpoint_pool_debug_event(
                "log_endpoint_primary_rejected",
                "selected Auto endpoint failed compact-cache readiness",
                &serde_json::json!({
                    "endpoint": self.config.endpoint,
                    "reason": &primary_failure,
                })
                .to_string(),
            );
            let health = self
                .clone()
                .probe_endpoints_owned(Some(primary_failure))
                .await;
            Self::report_endpoint_pool_health(&health);
            if health.iter().any(|endpoint| endpoint.healthy) {
                info!(
                    healthy = health.iter().filter(|endpoint| endpoint.healthy).count(),
                    total = health.len(),
                    "Connected to canonical lightwalletd endpoint pool"
                );
                return Ok(());
            }
            return Err(Error::Connection(
                "no canonical lightwalletd endpoint in the configured pool is available"
                    .to_string(),
            ));
        }

        self.clone().connect_single_endpoint().await
    }

    async fn connect_single_endpoint(self) -> Result<()> {
        let LightClient {
            config,
            channel: channel_state,
            ..
        } = self;
        let mut attempt = 0;
        let mut backoff = config.retry.initial_backoff;

        loop {
            match Self::try_connect(config.clone(), true).await {
                Ok(channel) => {
                    info!("Connected to lightwalletd at {}", config.endpoint);
                    *channel_state.lock_owned().await = Some(channel);
                    return Ok(());
                }
                Err(e) => {
                    attempt += 1;
                    if attempt >= config.retry.max_attempts {
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
                            (backoff.as_millis() as f64 * config.retry.backoff_multiplier) as u64,
                        ),
                        config.retry.max_backoff,
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

    fn configured_grpc_endpoint(config: &LightClientConfig) -> Result<Endpoint> {
        let endpoint_url = config.endpoint.clone();
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
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout);

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
                id, ts, config.tls.enabled, endpoint_url
            );
        });
        // #endregion
        if config.tls.enabled {
            // `ClientTlsConfig::new()` starts with an empty trust store. Keep
            // public CA validation enabled when overriding Tonic's automatic
            // HTTPS configuration to set an explicit server name.
            let mut tls_config = ClientTlsConfig::new().with_enabled_roots();

            // Set server name for SNI (required for TLS)
            if let Some(ref server_name) = config.tls.server_name {
                debug!("Using explicit server name for TLS: {}", server_name);
                tls_config = tls_config.domain_name(server_name.clone());
            } else {
                // Extract hostname from endpoint for SNI
                if let Some(host) = extract_host(&endpoint_url) {
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
            if config.tls.spki_pin.is_some() {
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

        Ok(endpoint)
    }

    async fn try_connect_for_probe(config: LightClientConfig) -> Result<Channel> {
        if config.tls.spki_pin.is_some() {
            return Err(Error::Connection(
                "pinned endpoints cannot participate in automatic endpoint pools".to_string(),
            ));
        }
        let endpoint = Self::configured_grpc_endpoint(&config)?;
        let transport_config = build_transport_config(&config)?;
        let manager = GLOBAL_TRANSPORT
            .clone()
            .get_matching(transport_config.clone())
            .await
            .ok_or(Error::Cancelled)?;
        if desired_transport_config()
            .as_ref()
            .is_some_and(|desired| desired != &transport_config)
        {
            return Err(Error::Cancelled);
        }
        Ok(manager.create_grpc_channel_lazy(endpoint))
    }

    async fn try_connect(config: LightClientConfig, initialize_transport: bool) -> Result<Channel> {
        let endpoint_url = config.endpoint.clone();
        debug!("Connecting to {} via {:?}", endpoint_url, config.transport);

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
                id, ts, endpoint_url, config.tls.enabled, config.transport, config.tls.server_name
            );
        });
        // #endregion

        let endpoint = Self::configured_grpc_endpoint(&config)?;

        if config.transport == TransportMode::Direct {
            warn!("Using DIRECT connection - IP address exposed to server!");
        }

        let transport_config = build_transport_config(&config)?;
        let manager = if initialize_transport {
            GLOBAL_TRANSPORT
                .clone()
                .get_or_init(transport_config.clone())
                .await?
        } else {
            GLOBAL_TRANSPORT
                .clone()
                .get_matching(transport_config.clone())
                .await
                .ok_or_else(|| Error::Cancelled)?
        };
        if !initialize_transport
            && desired_transport_config()
                .as_ref()
                .is_some_and(|desired| desired != &transport_config)
        {
            return Err(Error::Cancelled);
        }
        if config.tls.enabled {
            if let Some(expected_pin) = config.tls.spki_pin.as_deref() {
                let host = extract_host(&endpoint_url).ok_or_else(|| {
                    Error::Connection(format!(
                        "Could not extract host from endpoint URL '{}'",
                        endpoint_url
                    ))
                })?;
                let port = extract_port(&endpoint_url).unwrap_or(DEFAULT_LIGHTD_PORT);
                let server_name = config
                    .tls
                    .server_name
                    .clone()
                    .unwrap_or_else(|| host.clone());
                let actual_pin = manager
                    .clone()
                    .fetch_spki_pin(host.clone(), port, server_name.clone())
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
        if !initialize_transport
            && desired_transport_config()
                .as_ref()
                .is_some_and(|desired| desired != &transport_config)
        {
            return Err(Error::Cancelled);
        }
        let result = manager.create_grpc_channel(endpoint).await;

        match result {
            Ok(channel) => Ok(channel),
            Err(e) => {
                error!("Connection failed to {}: {}", endpoint_url, e);
                let error_msg = e.to_string();

                if matches!(config.transport, TransportMode::Direct) {
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
        let guard = Arc::clone(&self.channel).lock_owned().await;
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

    async fn get_latest_block_from_available_source(&self) -> Result<u64> {
        if let Some(tip) = self.canonical_pool_tip(false).await {
            return Ok(tip);
        }
        self.get_latest_block_internal().await
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

        let mut result = self.get_latest_block_from_available_source().await;

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
                    result = self.get_latest_block_from_available_source().await;
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
        let max_chunk_bytes = self
            .config
            .request_timeout
            .as_secs()
            .clamp(1, 256)
            .saturating_mul(1024 * 1024);
        let mut receiver =
            self.compact_block_chunk_stream(range, max_chunk_bytes, wallet_id.map(str::to_string));
        let mut blocks = Vec::new();
        while let Some(chunk) = receiver.recv().await {
            blocks.extend(chunk?.blocks);
        }
        Ok(blocks)
    }

    /// Start an exact-byte bounded compact-block stream.
    ///
    /// The returned channel has capacity one. This applies backpressure to the
    /// gRPC stream, validates strict height/hash ordering, emits a validated
    /// partial chunk before retry, and resumes from its next height.
    pub fn compact_block_chunk_stream(
        &self,
        range: Range<u32>,
        max_chunk_bytes: u64,
        wallet_id: Option<String>,
    ) -> mpsc::Receiver<Result<CompactBlockChunk>> {
        self.compact_block_segment_stream(range, max_chunk_bytes, u64::MAX, 1, wallet_id)
    }

    /// Starts a compact-block stream with device-independent segment boundaries.
    ///
    /// `max_segment_blocks` controls only the local stream handoff. The server
    /// still sees one long-lived range request, independent of scan-batch size.
    pub fn compact_block_segment_stream(
        &self,
        range: Range<u32>,
        max_segment_bytes: u64,
        max_segment_blocks: u64,
        channel_capacity: usize,
        wallet_id: Option<String>,
    ) -> mpsc::Receiver<Result<CompactBlockChunk>> {
        self.compact_block_adaptive_segment_stream(
            range,
            max_segment_bytes,
            Arc::new(AtomicU64::new(max_segment_blocks.max(1))),
            channel_capacity,
            wallet_id,
        )
    }

    pub(crate) fn compact_block_adaptive_segment_stream(
        &self,
        range: Range<u32>,
        max_segment_bytes: u64,
        segment_block_target: Arc<AtomicU64>,
        channel_capacity: usize,
        wallet_id: Option<String>,
    ) -> mpsc::Receiver<Result<CompactBlockChunk>> {
        let (sender, receiver) = mpsc::channel(channel_capacity.max(1));
        let client = self.clone();
        let error_sender = sender.clone();
        tokio::spawn(async move {
            if let Err(error) = client
                .produce_compact_block_chunks(
                    range,
                    max_segment_bytes,
                    segment_block_target,
                    wallet_id,
                    sender,
                )
                .await
            {
                let _ = error_sender.send(Err(error)).await;
            }
        });
        receiver
    }

    async fn produce_compact_block_chunks(
        self,
        range: Range<u32>,
        max_chunk_bytes: u64,
        segment_block_target: Arc<AtomicU64>,
        wallet_id: Option<String>,
        sender: mpsc::Sender<Result<CompactBlockChunk>>,
    ) -> Result<()> {
        if range.is_empty() {
            return Ok(());
        }

        let start = u64::from(range.start);
        let end_exclusive = u64::from(range.end);
        let assembler_bytes = if self.has_failover_endpoints() {
            max_chunk_bytes.min(HISTORICAL_STRIPE_HANDOFF_BYTES)
        } else {
            max_chunk_bytes
        };
        let mut assembler = OrderedBlockAssembler::with_limits(
            start,
            end_exclusive,
            assembler_bytes,
            segment_block_target.load(Ordering::Acquire),
        )?;

        let should_probe_pool = self.has_failover_endpoints()
            && should_probe_historical_pool(self.config.transport, start, end_exclusive);
        if should_probe_pool && !self.endpoint_pool_is_probed().await {
            self.start_endpoint_pool_probe().await;
        }

        while should_probe_pool
            && !self.endpoint_pool_is_probed().await
            && assembler.next_height() < end_exclusive
        {
            let prefix_end = assembler
                .next_height()
                .saturating_add(HISTORICAL_STRIPE_BLOCKS)
                .min(end_exclusive);
            self.stream_remaining_with_failover(
                prefix_end,
                wallet_id.clone(),
                &segment_block_target,
                &sender,
                &mut assembler,
            )
            .await?;
        }

        let stripe_plan = self
            .historical_stripe_plan(assembler.next_height(), end_exclusive)
            .await;
        if let Some(plan) = stripe_plan {
            write_endpoint_pool_debug_event(
                "log_historical_striping_started",
                "historical endpoint striping started",
                &format!(
                    r#"{{"sources":{},"start":{},"end_exclusive":{}}}"#,
                    plan.candidate_indices.len(),
                    assembler.next_height(),
                    plan.end_exclusive
                ),
            );
            info!(
                sources = plan.candidate_indices.len(),
                start,
                end_exclusive = plan.end_exclusive,
                "Starting canonical historical compact-block striping"
            );
            if let Err(error) = self
                .stream_historical_stripes(
                    &plan,
                    wallet_id.clone(),
                    assembler_bytes,
                    &segment_block_target,
                    &sender,
                    &mut assembler,
                )
                .await
            {
                if matches!(error, Error::Cancelled) || Self::is_non_retryable_error(&error) {
                    return Err(error);
                }
                warn!(
                    resume_height = assembler.next_height(),
                    error = %error,
                    "Historical endpoint striping degraded; resuming through one validated endpoint"
                );
            }
        }

        self.stream_remaining_with_failover(
            end_exclusive,
            wallet_id,
            &segment_block_target,
            &sender,
            &mut assembler,
        )
        .await?;

        if let Some(chunk) = assembler.finish()? {
            send_ordered_chunk(&sender, chunk, self.endpoint().to_string()).await?;
        }
        Ok(())
    }

    async fn stream_remaining_with_failover(
        &self,
        end_exclusive: u64,
        wallet_id: Option<String>,
        segment_block_target: &AtomicU64,
        sender: &mpsc::Sender<Result<CompactBlockChunk>>,
        assembler: &mut OrderedBlockAssembler,
    ) -> Result<()> {
        let max_rounds = self.config.retry.max_attempts.max(1);
        let mut round = 0u32;
        let mut backoff = self.config.retry.initial_backoff;
        let mut last_error = None;

        while assembler.next_height() < end_exclusive && round < max_rounds {
            let round_start = assembler.next_height();
            let candidates = self.candidate_order(end_exclusive.saturating_sub(1)).await;
            if candidates.is_empty() {
                return Err(Error::Connection(format!(
                    "no healthy lightwalletd endpoint reaches height {}",
                    end_exclusive.saturating_sub(1)
                )));
            }

            for index in candidates {
                let Some(candidate) = self.connected_candidate_client(index).await else {
                    last_error = Some(Error::Connection(format!(
                        "lightwalletd endpoint {} has no validated channel",
                        index
                    )));
                    continue;
                };
                let endpoint = candidate.endpoint().to_string();

                let attempt_start = assembler.next_height();
                match candidate
                    .stream_compact_blocks_once(
                        attempt_start,
                        end_exclusive,
                        wallet_id.clone(),
                        assembler,
                        segment_block_target,
                        sender,
                    )
                    .await
                {
                    Ok(()) => {
                        self.record_candidate_success(index).await;
                        if assembler.next_height() >= end_exclusive {
                            return Ok(());
                        }
                    }
                    Err(error) if Self::is_non_retryable_error(&error) => return Err(error),
                    Err(error) => {
                        if let Some(chunk) = assembler.take_partial() {
                            send_ordered_chunk(sender, chunk, endpoint).await?;
                        }
                        self.record_candidate_failure(index).await;
                        last_error = Some(error);
                    }
                }
            }

            if assembler.next_height() == round_start
                && self.has_failover_endpoints()
                && !self.endpoint_pool_is_probed().await
            {
                self.start_endpoint_pool_probe().await;
                self.wait_for_endpoint_pool_probe().await;
                if self.endpoint_pool_is_probed().await {
                    continue;
                }
            }

            round = round.saturating_add(1);
            if assembler.next_height() < end_exclusive && round < max_rounds {
                tokio::time::sleep(jitter_duration(backoff)).await;
                backoff = std::cmp::min(
                    Duration::from_millis(
                        (backoff.as_millis() as f64 * self.config.retry.backoff_multiplier) as u64,
                    ),
                    self.config.retry.max_backoff,
                );
            }
        }

        if assembler.next_height() >= end_exclusive {
            return Ok(());
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Network(format!(
                "compact block stream ended at {}, expected {}",
                assembler.next_height(),
                end_exclusive
            ))
        }))
    }

    async fn stream_historical_stripes(
        &self,
        plan: &HistoricalStripePlan,
        wallet_id: Option<String>,
        max_buffer_bytes: u64,
        segment_block_target: &AtomicU64,
        sender: &mpsc::Sender<Result<CompactBlockChunk>>,
        assembler: &mut OrderedBlockAssembler,
    ) -> Result<()> {
        let source_count = plan.candidate_indices.len();
        let source_buffer_bytes = historical_source_buffer_bytes(max_buffer_bytes, source_count);
        let (event_sender, mut event_receiver) = mpsc::channel(source_count.saturating_mul(2));
        let mut command_senders = Vec::with_capacity(source_count);
        let mut worker_handles = StripeWorkerGuard::default();

        for (worker_index, candidate_index) in plan.candidate_indices.iter().copied().enumerate() {
            let candidate = self
                .connected_candidate_client(candidate_index)
                .await
                .ok_or_else(|| {
                    Error::Connection(format!(
                        "validated lightwalletd endpoint {} has no connected channel",
                        candidate_index
                    ))
                })?;
            let (command_sender, command_receiver) = mpsc::channel(1);
            let worker_events = event_sender.clone();
            let worker_wallet = wallet_id.clone();
            let permits = Arc::new(Semaphore::new(
                source_buffer_bytes.min(u64::from(u32::MAX)) as usize
            ));
            let handle = tokio::spawn(Self::run_historical_stripe_worker(
                worker_index,
                candidate,
                command_receiver,
                worker_events,
                permits,
                source_buffer_bytes,
                worker_wallet,
            ));
            command_senders.push(command_sender);
            worker_handles.push(handle);
        }
        drop(event_sender);

        let mut idle_workers = (0..source_count).collect::<VecDeque<_>>();
        let mut disabled_workers = vec![false; source_count];
        let mut worker_failures = vec![0u32; source_count];
        let mut pending_ranges = VecDeque::<StripeRange>::new();
        let mut next_unassigned = assembler.next_height();
        let mut active_ranges = 0usize;
        let mut buffered = BTreeMap::<u64, BufferedStripeChunk>::new();

        loop {
            while let Some(worker_index) = idle_workers.pop_front() {
                if disabled_workers[worker_index] {
                    continue;
                }
                let range = if let Some(range) = pending_ranges.pop_front() {
                    Some(range)
                } else if next_unassigned < plan.end_exclusive {
                    let range = StripeRange {
                        start: next_unassigned,
                        end_exclusive: next_unassigned
                            .saturating_add(HISTORICAL_STRIPE_BLOCKS)
                            .min(plan.end_exclusive),
                        attempt: 1,
                    };
                    next_unassigned = range.end_exclusive;
                    Some(range)
                } else {
                    None
                };
                let Some(range) = range else {
                    idle_workers.push_front(worker_index);
                    break;
                };
                if command_senders[worker_index].send(range).await.is_err() {
                    disabled_workers[worker_index] = true;
                    pending_ranges.push_front(range);
                    continue;
                }
                active_ranges = active_ranges.saturating_add(1);
            }

            Self::flush_canonical_stripe_chunks(
                &mut buffered,
                assembler,
                segment_block_target,
                sender,
            )
            .await?;
            if assembler.next_height() >= plan.end_exclusive {
                return Ok(());
            }
            if active_ranges == 0 {
                return Err(Error::Network(format!(
                    "all canonical historical sources stopped before height {}",
                    assembler.next_height()
                )));
            }

            let Some(event) = event_receiver.recv().await else {
                return Err(Error::Network(format!(
                    "historical source workers ended before height {}",
                    assembler.next_height()
                )));
            };
            match event {
                StripeEvent::Chunk {
                    worker_index,
                    range,
                    chunk,
                    _permit,
                } => {
                    let chunk_start = chunk.start_height().ok_or_else(|| {
                        Error::Sync("historical source returned an empty chunk".to_string())
                    })?;
                    let chunk_end = chunk.end_height().ok_or_else(|| {
                        Error::Sync("historical source returned an empty chunk".to_string())
                    })?;
                    if chunk_start < range.start
                        || chunk_end >= range.end_exclusive
                        || chunk_start < assembler.next_height()
                        || buffered.contains_key(&chunk_start)
                    {
                        return Err(Error::Sync(format!(
                            "historical source {} returned overlapping range {}-{} for {}..{}",
                            worker_index, chunk_start, chunk_end, range.start, range.end_exclusive
                        )));
                    }
                    buffered.insert(chunk_start, BufferedStripeChunk { chunk, _permit });
                }
                StripeEvent::Complete { worker_index } => {
                    active_ranges = active_ranges.saturating_sub(1);
                    worker_failures[worker_index] = 0;
                    self.record_candidate_success(plan.candidate_indices[worker_index])
                        .await;
                    idle_workers.push_back(worker_index);
                }
                StripeEvent::Failed {
                    worker_index,
                    range,
                    resume_height,
                    error,
                } => {
                    active_ranges = active_ranges.saturating_sub(1);
                    self.record_candidate_failure(plan.candidate_indices[worker_index])
                        .await;
                    worker_failures[worker_index] = worker_failures[worker_index].saturating_add(1);
                    if resume_height < range.end_exclusive {
                        if should_leave_historical_striping(
                            range,
                            resume_height,
                            worker_failures[worker_index],
                            self.config.retry.max_attempts,
                        ) {
                            // A future range may already occupy every other
                            // worker's bounded handoff buffer. Leaving striped
                            // mode here releases those buffers and lets the
                            // ordinary failover path resume at the last
                            // contiguous validated height without deadlocking.
                            Self::flush_canonical_stripe_chunks(
                                &mut buffered,
                                assembler,
                                segment_block_target,
                                sender,
                            )
                            .await?;
                            return Err(error);
                        }
                        pending_ranges.push_front(StripeRange {
                            start: resume_height,
                            end_exclusive: range.end_exclusive,
                            attempt: range.attempt.saturating_add(1),
                        });
                    }
                    if worker_failures[worker_index] >= HISTORICAL_STRIPE_SOURCE_FAILURES {
                        disabled_workers[worker_index] = true;
                    } else {
                        idle_workers.push_back(worker_index);
                    }
                }
            }
        }
    }

    async fn run_historical_stripe_worker(
        worker_index: usize,
        candidate: LightClient,
        mut commands: mpsc::Receiver<StripeRange>,
        events: mpsc::Sender<StripeEvent>,
        permits: Arc<Semaphore>,
        chunk_bytes: u64,
        wallet_id: Option<String>,
    ) {
        let permit_capacity = chunk_bytes.min(u64::from(u32::MAX)).max(1);
        while let Some(range) = commands.recv().await {
            let start_u32 = match u32::try_from(range.start) {
                Ok(height) => height,
                Err(_) => {
                    let _ = events
                        .send(StripeEvent::Failed {
                            worker_index,
                            range,
                            resume_height: range.start,
                            error: Error::Sync(format!(
                                "historical stripe height {} exceeds u32",
                                range.start
                            )),
                        })
                        .await;
                    continue;
                }
            };
            let end_u32 = match u32::try_from(range.end_exclusive) {
                Ok(height) => height,
                Err(_) => {
                    let _ = events
                        .send(StripeEvent::Failed {
                            worker_index,
                            range,
                            resume_height: range.start,
                            error: Error::Sync(format!(
                                "historical stripe end {} exceeds u32",
                                range.end_exclusive
                            )),
                        })
                        .await;
                    continue;
                }
            };
            let mut receiver = candidate.compact_block_segment_stream(
                start_u32..end_u32,
                chunk_bytes,
                u64::MAX,
                1,
                wallet_id.clone(),
            );
            let mut next_height = range.start;
            let mut failure = None;
            while let Some(result) = receiver.recv().await {
                match result {
                    Ok(chunk) => {
                        let Some(chunk_start) = chunk.start_height() else {
                            failure = Some(Error::Sync(
                                "historical source returned an empty chunk".to_string(),
                            ));
                            break;
                        };
                        let Some(chunk_end) = chunk.end_height() else {
                            failure = Some(Error::Sync(
                                "historical source returned an empty chunk".to_string(),
                            ));
                            break;
                        };
                        if chunk_start != next_height || chunk_end >= range.end_exclusive {
                            failure = Some(Error::Sync(format!(
                                "historical stripe expected {}, received {}-{}",
                                next_height, chunk_start, chunk_end
                            )));
                            break;
                        }
                        let charge = chunk.encoded_bytes.max(1).min(permit_capacity) as u32;
                        let permit = match Arc::clone(&permits).acquire_many_owned(charge).await {
                            Ok(permit) => permit,
                            Err(_) => return,
                        };
                        next_height = chunk_end.saturating_add(1);
                        if events
                            .send(StripeEvent::Chunk {
                                worker_index,
                                range,
                                chunk,
                                _permit: permit,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(error) => {
                        failure = Some(error);
                        break;
                    }
                }
            }

            let event = if let Some(error) = failure {
                StripeEvent::Failed {
                    worker_index,
                    range,
                    resume_height: next_height,
                    error,
                }
            } else if next_height == range.end_exclusive {
                StripeEvent::Complete { worker_index }
            } else {
                StripeEvent::Failed {
                    worker_index,
                    range,
                    resume_height: next_height,
                    error: Error::Network(format!(
                        "historical source ended at {}, expected {}",
                        next_height, range.end_exclusive
                    )),
                }
            };
            if events.send(event).await.is_err() {
                return;
            }
        }
    }

    async fn flush_canonical_stripe_chunks(
        buffered: &mut BTreeMap<u64, BufferedStripeChunk>,
        assembler: &mut OrderedBlockAssembler,
        segment_block_target: &AtomicU64,
        sender: &mpsc::Sender<Result<CompactBlockChunk>>,
    ) -> Result<()> {
        while let Some(buffered_chunk) = buffered.remove(&assembler.next_height()) {
            let endpoint = buffered_chunk.chunk.endpoint;
            for (block, encoded_bytes) in buffered_chunk
                .chunk
                .blocks
                .into_iter()
                .zip(buffered_chunk.chunk.encoded_block_bytes)
            {
                assembler.set_next_chunk_max_blocks(segment_block_target.load(Ordering::Acquire));
                if let Some(chunk) = assembler.push(block, encoded_bytes)? {
                    send_ordered_chunk(sender, chunk, endpoint.clone()).await?;
                }
            }
        }
        Ok(())
    }

    async fn stream_compact_blocks_once(
        &self,
        start: u64,
        end_exclusive: u64,
        wallet_id: Option<String>,
        assembler: &mut OrderedBlockAssembler,
        segment_block_target: &AtomicU64,
        sender: &mpsc::Sender<Result<CompactBlockChunk>>,
    ) -> Result<()> {
        if start >= end_exclusive {
            return Ok(());
        }
        let start_u32 = u32::try_from(start)
            .map_err(|_| Error::Sync(format!("compact block height {} exceeds u32", start)))?;
        let end_u32 = u32::try_from(end_exclusive).map_err(|_| {
            Error::Sync(format!(
                "compact block end height {} exceeds u32",
                end_exclusive
            ))
        })?;
        let mut client = self.get_client().await?;
        let range_blocks = end_exclusive.saturating_sub(start).max(1);
        let (first_msg_timeout, next_msg_timeout, request_timeout) = compact_block_range_timeouts(
            self.config.transport,
            range_blocks,
            self.config.request_timeout,
        );
        let open_timeout = first_msg_timeout.saturating_add(Duration::from_secs(10));
        let mut request = tonic::Request::new(BlockRange {
            start: Some(BlockId {
                height: u64::from(start_u32),
                hash: Vec::new(),
            }),
            end: Some(BlockId {
                height: u64::from(end_u32 - 1),
                hash: Vec::new(),
            }),
        });
        request.set_timeout(request_timeout);

        let response = tokio::time::timeout(open_timeout, client.get_block_range(request))
            .await
            .map_err(|_| {
                Error::Network(format!(
                    "timed out opening compact block stream {}..{} via {}",
                    start,
                    end_exclusive,
                    self.endpoint()
                ))
            })??;
        let mut stream = response.into_inner();
        let mut received = 0u64;
        loop {
            let idle_timeout = if received == 0 {
                first_msg_timeout
            } else {
                next_msg_timeout
            };
            let message = tokio::time::timeout(idle_timeout, stream.message())
                .await
                .map_err(|_| {
                    Error::Network(format!(
                        "compact block stream stalled at height {} via {} after {:?}",
                        assembler.next_height(),
                        self.endpoint(),
                        idle_timeout
                    ))
                })??;
            let Some(proto_block) = message else {
                break;
            };
            let encoded_bytes = proto_block.encoded_len() as u64;
            assembler.set_next_chunk_max_blocks(segment_block_target.load(Ordering::Acquire));
            if let Some(chunk) = assembler.push(CompactBlock::from(proto_block), encoded_bytes)? {
                send_ordered_chunk(sender, chunk, self.endpoint().to_string()).await?;
            }
            received = received.saturating_add(1);
        }

        if assembler.next_height() >= end_exclusive {
            Ok(())
        } else {
            Err(Error::Network(format!(
                "compact block stream via {} ended early at height {} for requested range {}..{} (wallet={})",
                self.endpoint(),
                assembler.next_height(),
                start,
                end_exclusive,
                wallet_id.as_deref().unwrap_or("unknown")
            )))
        }
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

    /// Broadcast through the active endpoint and, when Auto mode has a pool,
    /// relay the identical signed transaction to a validated alternate in the
    /// background.
    ///
    /// Reusing the same transaction bytes is idempotent at consensus level and
    /// avoids creating a competing transaction. The primary acknowledgement is
    /// returned immediately so alternate validation does not add Tor or I2P
    /// probe latency to the send flow.
    pub async fn broadcast_redundant(&self, raw_tx: Vec<u8>) -> Result<String> {
        let txid = self.broadcast(raw_tx.clone()).await?;
        if !self.has_failover_endpoints() {
            return Ok(txid);
        }

        let source_index = self.endpoint_pool.read().await.active_index;
        let client = self.clone();
        let txid_for_log = txid.clone();
        tokio::spawn(async move {
            client
                .rebroadcast_to_validated_alternate(raw_tx, source_index, &txid_for_log)
                .await;
        });

        Ok(txid)
    }

    async fn rebroadcast_to_validated_alternate(
        &self,
        raw_tx: Vec<u8>,
        source_index: usize,
        txid: &str,
    ) {
        if !self.endpoint_pool_is_probed().await {
            let health = self.clone().probe_endpoints_owned(None).await;
            Self::report_endpoint_pool_health(&health);
        }

        let candidates = {
            let state = self.endpoint_pool.read().await;
            state
                .healthy_indices
                .iter()
                .copied()
                .filter(|index| *index != source_index)
                .take(REDUNDANT_BROADCAST_MAX_ALTERNATES)
                .collect::<Vec<_>>()
        };
        for index in candidates {
            let Some(candidate) = self.connected_candidate_client(index).await else {
                continue;
            };
            let endpoint = candidate.endpoint().to_string();
            match candidate.broadcast(raw_tx.clone()).await {
                Ok(_) => {
                    info!(
                        txid,
                        endpoint = %endpoint,
                        "Relayed transaction through a validated alternate endpoint"
                    );
                    return;
                }
                Err(error) => {
                    warn!(
                        txid,
                        endpoint = %endpoint,
                        %error,
                        "Validated alternate did not accept transaction relay"
                    );
                }
            }
        }

        warn!(
            txid,
            "No validated alternate endpoint accepted the background transaction relay"
        );
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

            let mut request = tonic::Request::new(Empty {});
            request.set_timeout(self.config.request_timeout);
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

    fn subtree_root_capability_key(endpoint: &str, protocol: ShieldedProtocol) -> (String, i32) {
        (normalize_endpoint_identity(endpoint), protocol as i32)
    }

    fn subtree_root_capability(
        &self,
        endpoint: &str,
        protocol: ShieldedProtocol,
    ) -> Option<SubtreeRootCapability> {
        let key = Self::subtree_root_capability_key(endpoint, protocol);
        self.subtree_root_capabilities
            .read()
            .ok()
            .and_then(|capabilities| capabilities.get(&key).copied())
    }

    fn subtree_root_endpoint_priority(
        &self,
        endpoint: &str,
        protocol: ShieldedProtocol,
        now: Instant,
    ) -> Option<u8> {
        match self.subtree_root_capability(endpoint, protocol) {
            Some(SubtreeRootCapability::Available) => Some(0),
            Some(SubtreeRootCapability::RetryAfter(retry_after)) if now < retry_after => None,
            Some(SubtreeRootCapability::RetryAfter(_)) | None => Some(1),
        }
    }

    /// Return whether any configured endpoint's cached capability permits an optional probe.
    pub(crate) fn subtree_root_probe_allowed(&self, protocol: ShieldedProtocol) -> bool {
        let now = Instant::now();
        (0..self.endpoint_count())
            .filter_map(|index| self.endpoint_candidate(index))
            .any(|candidate| {
                self.subtree_root_endpoint_priority(&candidate.endpoint, protocol, now)
                    .is_some()
            })
    }

    fn record_subtree_root_result(
        &self,
        protocol: ShieldedProtocol,
        result: &Result<Vec<SubtreeRoot>>,
    ) {
        let capability = match result {
            Ok(_) => SubtreeRootCapability::Available,
            Err(Error::Status(status)) if status.code() == tonic::Code::Unimplemented => {
                SubtreeRootCapability::RetryAfter(Instant::now() + SUBTREE_ROOT_UNSUPPORTED_RETRY)
            }
            Err(_) => {
                SubtreeRootCapability::RetryAfter(Instant::now() + SUBTREE_ROOT_TRANSIENT_RETRY)
            }
        };
        if let Ok(mut capabilities) = self.subtree_root_capabilities.write() {
            capabilities.insert(
                Self::subtree_root_capability_key(&self.config.endpoint, protocol),
                capability,
            );
        }
    }

    pub(crate) fn record_subtree_root_timeout(&self, protocol: ShieldedProtocol) {
        // The outer prefill timeout cannot identify which member of an automatic
        // pool was in flight. Candidate clients record their own deterministic
        // and transient results, so avoid poisoning the configured primary here.
        if self.has_failover_endpoints() {
            return;
        }
        if let Ok(mut capabilities) = self.subtree_root_capabilities.write() {
            capabilities.insert(
                Self::subtree_root_capability_key(&self.config.endpoint, protocol),
                SubtreeRootCapability::RetryAfter(Instant::now() + SUBTREE_ROOT_TIMEOUT_RETRY),
            );
        }
    }

    async fn subtree_root_candidate_order(&self, protocol: ShieldedProtocol) -> Vec<usize> {
        let state = self.endpoint_pool.read().await;
        let mut candidates = if state.probed {
            state.healthy_indices.clone()
        } else {
            vec![0]
        };
        let now = Instant::now();
        candidates.retain(|index| {
            self.endpoint_candidate(*index).is_some_and(|candidate| {
                self.subtree_root_endpoint_priority(&candidate.endpoint, protocol, now)
                    .is_some()
            })
        });
        candidates.sort_by_key(|index| {
            self.endpoint_candidate(*index)
                .and_then(|candidate| {
                    self.subtree_root_endpoint_priority(&candidate.endpoint, protocol, now)
                })
                .unwrap_or(u8::MAX)
        });
        candidates
    }

    async fn get_subtree_roots_single_endpoint(
        &self,
        start_index: u32,
        shielded_protocol: ShieldedProtocol,
        max_entries: u32,
    ) -> Result<Vec<SubtreeRoot>> {
        let result = self
            .with_retry(|| async {
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
            .await;
        self.record_subtree_root_result(shielded_protocol, &result);
        result
    }

    /// Fetch historical subtree roots for a shielded pool.
    pub async fn get_subtree_roots(
        &self,
        start_index: u32,
        shielded_protocol: ShieldedProtocol,
        max_entries: u32,
    ) -> Result<Vec<SubtreeRoot>> {
        let mut pool_ready = self.endpoint_pool_is_probed().await;
        if self.has_failover_endpoints() && !pool_ready {
            self.start_endpoint_pool_probe().await;
        }

        let mut attempted = HashSet::new();
        let mut last_error = None;
        loop {
            for index in self.subtree_root_candidate_order(shielded_protocol).await {
                let Some(candidate_endpoint) = self.endpoint_candidate(index) else {
                    continue;
                };
                let identity = normalize_endpoint_identity(&candidate_endpoint.endpoint);
                if !attempted.insert(identity) {
                    continue;
                }
                let candidate = match self.connected_candidate_client(index).await {
                    Some(candidate) => candidate,
                    None if !pool_ready && index == 0 => {
                        let Some(candidate) = self.candidate_client(index) else {
                            continue;
                        };
                        candidate
                    }
                    None => continue,
                };

                debug!(
                    endpoint = %candidate.endpoint(),
                    protocol = ?shielded_protocol,
                    start_index,
                    "Requesting subtree roots from validated endpoint"
                );
                match candidate
                    .get_subtree_roots_single_endpoint(start_index, shielded_protocol, max_entries)
                    .await
                {
                    Ok(roots) => {
                        debug!(
                            endpoint = %candidate.endpoint(),
                            protocol = ?shielded_protocol,
                            roots = roots.len(),
                            "Subtree-root endpoint completed optional request"
                        );
                        return Ok(roots);
                    }
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(error) => {
                        debug!(
                            endpoint = %candidate.endpoint(),
                            protocol = ?shielded_protocol,
                            %error,
                            "Subtree-root endpoint unavailable; trying another validated endpoint"
                        );
                        last_error = Some(error);
                    }
                }
            }

            if self.has_failover_endpoints() && !pool_ready {
                self.wait_for_endpoint_pool_probe().await;
                pool_ready = self.endpoint_pool_is_probed().await;
                if pool_ready {
                    continue;
                }
            }
            break;
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Network(format!(
                "No validated lightwalletd endpoint currently supports {:?} subtree roots",
                shielded_protocol
            ))
        }))
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
            endpoint_pool_probe_inflight: Arc::clone(&self.endpoint_pool_probe_inflight),
            endpoint_pool_probe_notify: Arc::clone(&self.endpoint_pool_probe_notify),
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

    static TRANSPORT_STATE_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    fn auto_pool_client(transport: TransportMode) -> LightClient {
        let mut config = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        config.transport = transport;
        if transport == TransportMode::Socks5 {
            config.socks5_url = Some("socks5://127.0.0.1:9050".to_string());
        }
        LightClient::with_config(config.with_pirate_mainnet_auto_pool())
    }

    async fn seed_endpoint_pool(client: &LightClient, tips: &[u64], failures: &[(usize, u32)]) {
        let mut state = Arc::clone(&client.endpoint_pool).write_owned().await;
        state.probed = true;
        state.healthy_indices = (0..tips.len()).collect();
        state.tips = tips.iter().copied().enumerate().collect();
        state.probe_latencies = (0..tips.len())
            .map(|index| (index, Duration::from_millis((index + 1) as u64)))
            .collect();
        state.failures = failures.iter().copied().collect();
    }

    fn valid_subtree_root(height: u64) -> SubtreeRoot {
        SubtreeRoot {
            root_hash: vec![1; 32],
            completing_block_hash: vec![2; 32],
            completing_block_height: height,
        }
    }

    fn compact_block(height: u64, hash_byte: u8, previous_hash: Vec<u8>) -> CompactBlock {
        CompactBlock {
            proto_version: 1,
            height,
            hash: vec![hash_byte; 32],
            prev_hash: previous_hash,
            time: height as u32,
            header: Vec::new(),
            transactions: Vec::new(),
        }
    }

    async fn buffered_chunk(
        chunk: CompactBlockChunk,
        permits: Arc<Semaphore>,
    ) -> BufferedStripeChunk {
        BufferedStripeChunk {
            chunk,
            _permit: permits.acquire_owned().await.expect("buffer permit"),
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
    fn single_endpoint_subtree_root_capability_recovers_on_success() {
        let client = LightClient::new("https://roots.example:443".to_string());
        assert!(client.subtree_root_probe_allowed(ShieldedProtocol::Sapling));
        assert!(client.subtree_root_probe_allowed(ShieldedProtocol::Ironwood));

        client.record_subtree_root_timeout(ShieldedProtocol::Sapling);
        assert!(!client.subtree_root_probe_allowed(ShieldedProtocol::Sapling));
        assert!(client.subtree_root_probe_allowed(ShieldedProtocol::Ironwood));

        let success = Ok(Vec::new());
        client.record_subtree_root_result(ShieldedProtocol::Sapling, &success);
        assert!(client.subtree_root_probe_allowed(ShieldedProtocol::Sapling));
    }

    #[tokio::test]
    async fn subtree_root_capabilities_are_scoped_to_the_actual_endpoint() {
        let client = auto_pool_client(TransportMode::Direct);
        seed_endpoint_pool(&client, &[1_000, 1_000, 1_000], &[]).await;
        let primary = client.candidate_client(0).expect("primary endpoint");
        let crypto_forge = client.candidate_client(1).expect("alternate endpoint");

        let unsupported: Result<Vec<SubtreeRoot>> = Err(Error::Status(
            tonic::Status::unimplemented("GetSubtreeRoots is unavailable"),
        ));
        primary.record_subtree_root_result(ShieldedProtocol::Sapling, &unsupported);
        let success = Ok(Vec::new());
        crypto_forge.record_subtree_root_result(ShieldedProtocol::Sapling, &success);

        assert!(client.subtree_root_probe_allowed(ShieldedProtocol::Sapling));
        assert!(client.subtree_root_probe_allowed(ShieldedProtocol::Ironwood));
        let order = client
            .subtree_root_candidate_order(ShieldedProtocol::Sapling)
            .await;
        assert_eq!(order.first(), Some(&1));
        assert!(!order.contains(&0));
    }

    #[tokio::test]
    async fn subtree_root_routing_uses_only_validated_pool_members() {
        let client = auto_pool_client(TransportMode::Direct);
        seed_endpoint_pool(&client, &[1_000, 1_000, 1_000], &[]).await;
        {
            let mut state = client.endpoint_pool.write().await;
            state.healthy_indices = vec![0, 2];
        }
        let unvalidated = client.candidate_client(1).expect("unvalidated endpoint");
        let success = Ok(Vec::new());
        unvalidated.record_subtree_root_result(ShieldedProtocol::Sapling, &success);

        let order = client
            .subtree_root_candidate_order(ShieldedProtocol::Sapling)
            .await;
        assert_eq!(order, vec![0, 2]);
    }

    #[test]
    fn automatic_pool_timeout_is_not_attributed_to_its_primary() {
        let client = auto_pool_client(TransportMode::Direct);
        client.record_subtree_root_timeout(ShieldedProtocol::Sapling);

        assert!(client.subtree_root_probe_allowed(ShieldedProtocol::Sapling));
        assert_eq!(
            client.subtree_root_capability(DEFAULT_LIGHTD_URL, ShieldedProtocol::Sapling),
            None
        );
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
    fn canonical_mainnet_pool_contains_only_curated_tls_servers() {
        assert_eq!(MAINNET_AUTO_LIGHTD_URLS.len(), 7);
        for endpoint in MAINNET_AUTO_LIGHTD_URLS {
            assert!(endpoint.starts_with("https://"), "{endpoint}");
            assert!(is_pirate_mainnet_auto_endpoint(endpoint), "{endpoint}");
        }
        assert!(!MAINNET_AUTO_LIGHTD_URLS.contains(&"https://lightd.pirate.black:443"));
        for endpoint in [
            "http://64.23.167.130:9067",
            "http://example.com:9067",
            "http://lx34l6evvk7vynbulx6brxqyzzes4balb3owhteb4jyqpdoosbfc3oid.onion:9067",
        ] {
            assert!(!is_pirate_mainnet_auto_endpoint(endpoint), "{endpoint}");
        }
    }

    #[test]
    fn automatic_pool_preserves_transport_and_endpoint_tls_identity() {
        for transport in [
            TransportMode::Direct,
            TransportMode::Tor,
            TransportMode::Socks5,
        ] {
            let client = auto_pool_client(transport);
            assert_eq!(client.endpoint_count(), MAINNET_AUTO_LIGHTD_URLS.len());
            let mut identities = vec![normalize_endpoint_identity(client.endpoint())];
            for index in 1..client.endpoint_count() {
                let candidate = client.candidate_client(index).expect("pool candidate");
                assert_eq!(candidate.config.transport, transport);
                assert_eq!(candidate.config.socks5_url, client.config.socks5_url);
                assert!(candidate.config.tls.enabled);
                assert!(candidate.config.tls.spki_pin.is_none());
                identities.push(normalize_endpoint_identity(candidate.endpoint()));
            }
            identities.sort();
            identities.dedup();
            assert_eq!(identities.len(), MAINNET_AUTO_LIGHTD_URLS.len());
        }
    }

    #[test]
    fn endpoint_pool_probe_is_reserved_for_large_network_ranges() {
        assert!(!should_probe_historical_pool(
            TransportMode::Direct,
            1_000,
            1_511
        ));
        assert!(should_probe_historical_pool(
            TransportMode::Direct,
            1_000,
            1_512
        ));
        assert!(should_probe_historical_pool(
            TransportMode::Tor,
            1_000,
            2_000
        ));
        assert!(!should_probe_historical_pool(
            TransportMode::I2p,
            1_000,
            2_000
        ));
    }

    #[test]
    fn cloned_clients_share_endpoint_probe_coordination() {
        let client = auto_pool_client(TransportMode::Direct);
        let clone = client.clone();

        assert!(Arc::ptr_eq(
            &client.endpoint_pool_probe_inflight,
            &clone.endpoint_pool_probe_inflight
        ));
        assert!(Arc::ptr_eq(
            &client.endpoint_pool_probe_notify,
            &clone.endpoint_pool_probe_notify
        ));
    }

    #[test]
    fn pinned_custom_and_i2p_endpoints_remain_single_source() {
        let pinned = LightClientConfig::direct(DEFAULT_LIGHTD_URL)
            .with_spki_pin("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .with_pirate_mainnet_auto_pool();
        assert!(pinned.failover_endpoints.is_empty());

        let custom =
            LightClientConfig::direct("https://example.com:443").with_pirate_mainnet_auto_pool();
        assert!(custom.failover_endpoints.is_empty());

        let mut i2p = LightClientConfig::direct(DEFAULT_LIGHTD_URL);
        i2p.transport = TransportMode::I2p;
        assert!(i2p
            .with_pirate_mainnet_auto_pool()
            .failover_endpoints
            .is_empty());
    }

    #[tokio::test]
    async fn historical_striping_uses_current_sources_and_preserves_tip_margin() {
        let client = auto_pool_client(TransportMode::Direct);
        seed_endpoint_pool(&client, &[1_000, 995, 1_000, 900], &[]).await;

        let plan = client
            .historical_stripe_plan(100, 1_001)
            .await
            .expect("historical stripe plan");
        assert_eq!(plan.candidate_indices, vec![0, 1, 2]);
        assert_eq!(plan.end_exclusive, 896);
    }

    #[tokio::test]
    async fn historical_striping_respects_transport_and_failure_bounds() {
        let tor_client = auto_pool_client(TransportMode::Tor);
        seed_endpoint_pool(
            &tor_client,
            &[2_000, 2_000, 2_000, 2_000],
            &[(0, HISTORICAL_STRIPE_SOURCE_FAILURES)],
        )
        .await;
        let plan = tor_client
            .historical_stripe_plan(1_000, 1_900)
            .await
            .expect("Tor stripe plan");
        assert_eq!(plan.candidate_indices, vec![1, 2]);

        let i2p_client = auto_pool_client(TransportMode::I2p);
        seed_endpoint_pool(&i2p_client, &[2_000, 2_000], &[]).await;
        assert!(i2p_client
            .historical_stripe_plan(1_000, 1_900)
            .await
            .is_none());

        let short_client = auto_pool_client(TransportMode::Direct);
        seed_endpoint_pool(&short_client, &[2_000, 2_000], &[]).await;
        assert!(short_client
            .historical_stripe_plan(1_000, 1_511)
            .await
            .is_none());
    }

    #[test]
    fn historical_handoff_memory_is_bounded_across_sources() {
        for source_count in 1..=HISTORICAL_STRIPE_MAX_SOURCES {
            let per_source =
                historical_source_buffer_bytes(HISTORICAL_STRIPE_HANDOFF_BYTES, source_count);
            assert!(per_source >= 1);
            assert!(
                per_source.saturating_mul(source_count as u64) <= HISTORICAL_STRIPE_HANDOFF_BYTES
            );
        }
    }

    #[test]
    fn quarantined_incomplete_source_leaves_striped_mode() {
        let range = StripeRange {
            start: 1_000,
            end_exclusive: 1_256,
            attempt: 2,
        };
        assert!(should_leave_historical_striping(
            range,
            1_128,
            HISTORICAL_STRIPE_SOURCE_FAILURES,
            5,
        ));
        assert!(!should_leave_historical_striping(range, 1_128, 1, 5,));
        assert!(!should_leave_historical_striping(
            range,
            range.end_exclusive,
            HISTORICAL_STRIPE_SOURCE_FAILURES,
            5,
        ));
    }

    #[test]
    fn active_endpoint_prefers_the_selected_primary_unless_it_is_stale() {
        let healthy = vec![0, 1, 2];
        let latencies = HashMap::from([
            (0, Duration::from_millis(20)),
            (1, Duration::from_millis(5)),
            (2, Duration::from_millis(10)),
        ]);
        let current_tips = HashMap::from([(0, 1_000), (1, 1_010), (2, 1_010)]);
        assert_eq!(
            preferred_active_endpoint(&healthy, &current_tips, &latencies),
            Some(0)
        );

        let stale_tips = HashMap::from([(0, 900), (1, 1_010), (2, 1_010)]);
        assert_eq!(
            preferred_active_endpoint(&healthy, &stale_tips, &latencies),
            Some(1)
        );
    }

    #[test]
    fn candidate_order_requires_a_fresh_tip_at_the_requested_height() {
        let mut state = EndpointPoolState {
            probed: true,
            active_index: 0,
            healthy_indices: vec![0, 1, 2],
            tips: HashMap::from([(0, 1_000), (1, 1_000), (2, 999)]),
            ..EndpointPoolState::default()
        };

        assert!(eligible_candidate_order(&state, 1_001).is_empty());

        state.tips.insert(1, 1_001);
        assert_eq!(eligible_candidate_order(&state, 1_001), vec![1]);
    }

    #[test]
    fn compact_cache_readiness_accepts_the_served_advertised_tip() {
        let info = proto::LightdInfo {
            block_height: 1_010,
            estimated_height: 1_010,
            ..proto::LightdInfo::default()
        };
        let advertised_tip = BlockId {
            height: 1_010,
            hash: vec![7; 32],
        };
        let compact_tip: proto::CompactBlock = compact_block(1_010, 7, vec![6; 32]).into();

        validate_compact_cache_tip(&info, &advertised_tip, &compact_tip)
            .expect("matching compact-cache tip should be ready");
    }

    #[test]
    fn compact_cache_readiness_rejects_an_empty_or_stale_cache() {
        let synced_info = proto::LightdInfo {
            block_height: 1_010,
            estimated_height: 1_010,
            ..proto::LightdInfo::default()
        };
        let empty_tip = BlockId {
            height: 0,
            hash: Vec::new(),
        };
        let empty_block: proto::CompactBlock = compact_block(0, 0, Vec::new()).into();
        let empty_error = validate_compact_cache_tip(&synced_info, &empty_tip, &empty_block)
            .expect_err("empty cache must not be ready");
        assert!(empty_error.to_string().contains("empty tip"));

        let reindexing_info = proto::LightdInfo {
            block_height: 700,
            estimated_height: 1_010,
            ..proto::LightdInfo::default()
        };
        let stale_tip = BlockId {
            height: 700,
            hash: vec![7; 32],
        };
        let stale_block: proto::CompactBlock = compact_block(700, 7, vec![6; 32]).into();
        let stale_error = validate_compact_cache_tip(&reindexing_info, &stale_tip, &stale_block)
            .expect_err("a reindexing server must not advertise a stale cache as ready");
        assert!(stale_error
            .to_string()
            .contains("trails reported network height"));
    }

    #[test]
    fn compact_cache_readiness_rejects_inconsistent_tip_blocks() {
        let info = proto::LightdInfo {
            block_height: 1_010,
            estimated_height: 1_010,
            ..proto::LightdInfo::default()
        };
        let advertised_tip = BlockId {
            height: 1_010,
            hash: vec![7; 32],
        };

        let wrong_height: proto::CompactBlock = compact_block(1_009, 7, vec![6; 32]).into();
        let height_error = validate_compact_cache_tip(&info, &advertised_tip, &wrong_height)
            .expect_err("wrong compact-block height must not be ready");
        assert!(height_error.to_string().contains("returned compact block"));

        let wrong_hash: proto::CompactBlock = compact_block(1_010, 8, vec![6; 32]).into();
        let hash_error = validate_compact_cache_tip(&info, &advertised_tip, &wrong_hash)
            .expect_err("wrong compact-block hash must not be ready");
        assert!(hash_error.to_string().contains("advertised tip hash"));

        let malformed_hash_tip = BlockId {
            height: 1_010,
            hash: vec![7; 31],
        };
        let valid_block: proto::CompactBlock = compact_block(1_010, 7, vec![6; 32]).into();
        let malformed_error = validate_compact_cache_tip(&info, &malformed_hash_tip, &valid_block)
            .expect_err("malformed advertised hash must not be ready");
        assert!(malformed_error.to_string().contains("31 bytes"));
    }

    #[test]
    fn refreshed_pool_tip_prefers_the_fastest_endpoint_at_the_highest_height() {
        let healthy = vec![0, 1, 2];
        let tips = HashMap::from([(0, 1_000), (1, 1_010), (2, 1_010)]);
        let latencies = HashMap::from([
            (0, Duration::from_millis(2)),
            (1, Duration::from_millis(20)),
            (2, Duration::from_millis(5)),
        ]);

        assert_eq!(
            highest_tip_endpoint(&healthy, &tips, &latencies),
            Some((2, 1_010))
        );
    }

    #[tokio::test]
    async fn striped_chunks_wait_for_and_preserve_canonical_order() {
        let permits = Arc::new(Semaphore::new(2));
        let mut buffered = BTreeMap::new();
        buffered.insert(
            11,
            buffered_chunk(
                CompactBlockChunk {
                    blocks: vec![compact_block(11, 11, vec![10; 32])],
                    encoded_block_bytes: vec![20],
                    encoded_bytes: 20,
                    endpoint: "second.example".to_string(),
                },
                Arc::clone(&permits),
            )
            .await,
        );
        let mut assembler = OrderedBlockAssembler::with_limits(10, 12, 100, 1).unwrap();
        let target = AtomicU64::new(1);
        let (sender, mut receiver) = mpsc::channel(2);

        LightClient::flush_canonical_stripe_chunks(&mut buffered, &mut assembler, &target, &sender)
            .await
            .unwrap();
        assert!(receiver.try_recv().is_err());

        buffered.insert(
            10,
            buffered_chunk(
                CompactBlockChunk {
                    blocks: vec![compact_block(10, 10, vec![9; 32])],
                    encoded_block_bytes: vec![20],
                    encoded_bytes: 20,
                    endpoint: "first.example".to_string(),
                },
                permits,
            )
            .await,
        );
        LightClient::flush_canonical_stripe_chunks(&mut buffered, &mut assembler, &target, &sender)
            .await
            .unwrap();

        let first = receiver.recv().await.unwrap().unwrap();
        let second = receiver.recv().await.unwrap().unwrap();
        assert_eq!(first.start_height(), Some(10));
        assert_eq!(second.start_height(), Some(11));
        assert!(assembler.is_complete());
    }

    #[tokio::test]
    async fn striped_chunks_reject_cross_source_chain_discontinuity() {
        let permits = Arc::new(Semaphore::new(2));
        let mut buffered = BTreeMap::new();
        for (height, previous_hash) in [(20, vec![19; 32]), (21, vec![99; 32])] {
            buffered.insert(
                height,
                buffered_chunk(
                    CompactBlockChunk {
                        blocks: vec![compact_block(height, height as u8, previous_hash)],
                        encoded_block_bytes: vec![20],
                        encoded_bytes: 20,
                        endpoint: format!("source-{height}"),
                    },
                    Arc::clone(&permits),
                )
                .await,
            );
        }
        let mut assembler = OrderedBlockAssembler::with_limits(20, 22, 100, 1).unwrap();
        let target = AtomicU64::new(1);
        let (sender, _receiver) = mpsc::channel(2);

        let error = LightClient::flush_canonical_stripe_chunks(
            &mut buffered,
            &mut assembler,
            &target,
            &sender,
        )
        .await
        .expect_err("disconnected striped chain");
        assert!(error.to_string().contains("disconnected at height 21"));
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
    async fn global_transport_initialization_is_single_flight() {
        let _test_guard = TRANSPORT_STATE_TEST_LOCK.lock().await;
        clear_desired_transport_config();
        let state = Arc::new(GlobalTransportState {
            manager: Arc::new(RwLock::new(None)),
            initialization: Arc::new(Mutex::new(())),
        });
        let config = NetTransportConfig {
            mode: NetTransportMode::Direct,
            ..NetTransportConfig::default()
        };

        let first_state = Arc::clone(&state);
        let first_config = config.clone();
        let second_state = Arc::clone(&state);
        let (first, second) = tokio::join!(
            async move { first_state.get_or_init(first_config).await },
            async move { second_state.get_or_init(config).await },
        );
        let first = first.expect("first transport initialization");
        let second = second.expect("second transport initialization");

        assert!(Arc::ptr_eq(&first, &second));
        state.shutdown().await;
        clear_desired_transport_config();
    }

    #[tokio::test]
    async fn stale_background_probe_cannot_reselect_an_old_transport() {
        let _test_guard = TRANSPORT_STATE_TEST_LOCK.lock().await;
        clear_desired_transport_config();
        let state = Arc::new(GlobalTransportState {
            manager: Arc::new(RwLock::new(None)),
            initialization: Arc::new(Mutex::new(())),
        });
        let direct = NetTransportConfig {
            mode: NetTransportMode::Direct,
            ..NetTransportConfig::default()
        };
        state
            .clone()
            .get_or_init(direct.clone())
            .await
            .expect("direct transport");

        let tor = NetTransportConfig::default();
        set_desired_transport_config(tor);
        assert!(state.clone().get_matching(direct.clone()).await.is_none());
        assert!(matches!(
            state.clone().get_or_init(direct).await,
            Err(Error::Cancelled)
        ));

        state.shutdown().await;
        clear_desired_transport_config();
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
    use crate::intake::{
        AdaptiveDurableSegmentController, DurableSegmentObservation, DEFAULT_DURABLE_SEGMENT_BLOCKS,
    };

    #[derive(Clone, Copy)]
    enum SegmentBenchmarkStrategy {
        Fixed,
        Adaptive,
    }

    impl SegmentBenchmarkStrategy {
        fn name(self) -> &'static str {
            match self {
                Self::Fixed => "fixed 1024",
                Self::Adaptive => "adaptive",
            }
        }
    }

    async fn drain_segment_benchmark(
        client: &LightClient,
        start: u32,
        end: u32,
        strategy: SegmentBenchmarkStrategy,
    ) -> Result<(u64, u64)> {
        const MAX_SEGMENT_BYTES: u64 = 64 * 1024 * 1024;
        let target = Arc::new(AtomicU64::new(DEFAULT_DURABLE_SEGMENT_BLOCKS));
        let mut controller = AdaptiveDurableSegmentController::new(MAX_SEGMENT_BYTES);
        let mut receiver = match strategy {
            SegmentBenchmarkStrategy::Fixed => client.compact_block_segment_stream(
                start..end,
                MAX_SEGMENT_BYTES,
                DEFAULT_DURABLE_SEGMENT_BLOCKS,
                1,
                None,
            ),
            SegmentBenchmarkStrategy::Adaptive => client.compact_block_adaptive_segment_stream(
                start..end,
                MAX_SEGMENT_BYTES,
                Arc::clone(&target),
                1,
                None,
            ),
        };
        let mut expected = u64::from(start);
        let mut chunks = 0u64;
        while let Some(chunk) = {
            let wait_started = Instant::now();
            let chunk = receiver.recv().await.transpose()?;
            let network_wait = wait_started.elapsed();
            if let (SegmentBenchmarkStrategy::Adaptive, Some(chunk)) = (strategy, chunk.as_ref()) {
                let chunk_end = chunk.end_height().unwrap_or(expected);
                let next = controller.observe(DurableSegmentObservation {
                    blocks: chunk.blocks.len() as u64,
                    encoded_bytes: chunk.encoded_bytes,
                    network_wait,
                    cache_write: Duration::from_millis(20),
                    queued_bytes: 0,
                    high_water_bytes: MAX_SEGMENT_BYTES,
                    stream_tail: chunk_end.saturating_add(1) == u64::from(end),
                });
                target.store(next, Ordering::Release);
            }
            chunk
        } {
            for block in &chunk.blocks {
                if block.height != expected {
                    return Err(Error::Sync(format!(
                        "segment stream expected {}, received {}",
                        expected, block.height
                    )));
                }
                expected = expected.saturating_add(1);
            }
            chunks = chunks.saturating_add(1);
        }
        if expected != u64::from(end) {
            return Err(Error::Sync(format!(
                "segment stream ended at {}, expected {}",
                expected, end
            )));
        }
        Ok((chunks, controller.target_blocks()))
    }

    #[tokio::test]
    #[ignore = "manual live durable-segment benchmark"]
    async fn benchmark_live_adaptive_durable_segments() {
        let endpoint = std::env::var("PIRATE_SEGMENT_BENCH_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_LIGHTD_URL.to_string());
        let start = std::env::var("PIRATE_SEGMENT_BENCH_START")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(4_000_000);
        let blocks = std::env::var("PIRATE_SEGMENT_BENCH_BLOCKS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(4_000);
        let runs = std::env::var("PIRATE_SEGMENT_BENCH_RUNS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(4)
            .max(1);
        let end = start.checked_add(blocks).expect("benchmark range");
        let client = LightClient::with_config(LightClientConfig::direct(&endpoint));
        client.connect().await.expect("benchmark connection");

        let strategies = [
            SegmentBenchmarkStrategy::Fixed,
            SegmentBenchmarkStrategy::Adaptive,
        ];
        let mut totals = [Duration::ZERO; 2];
        for run in 0..runs as usize {
            for offset in 0..strategies.len() {
                let index = (run + offset) % strategies.len();
                let strategy = strategies[index];
                let started = Instant::now();
                let (chunks, final_target) = drain_segment_benchmark(&client, start, end, strategy)
                    .await
                    .expect("durable segment benchmark");
                let elapsed = started.elapsed();
                totals[index] += elapsed;
                println!(
                    "durable segment run {}/{}: {:<10} {:.3}s, {:.1} blocks/s, chunks={}, final_target={}",
                    run + 1,
                    runs,
                    strategy.name(),
                    elapsed.as_secs_f64(),
                    f64::from(blocks) / elapsed.as_secs_f64(),
                    chunks,
                    final_target
                );
            }
        }
        for (index, strategy) in strategies.into_iter().enumerate() {
            let average = totals[index] / runs;
            println!(
                "durable segment average: {:<10} {:.3}s, {:.1} blocks/s",
                strategy.name(),
                average.as_secs_f64(),
                f64::from(blocks) / average.as_secs_f64()
            );
        }
    }

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

    #[tokio::test]
    #[ignore = "Requires live network connection"]
    async fn test_live_mainnet_endpoint_pool() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL).with_pirate_mainnet_auto_pool();
        let client = LightClient::with_config(config);
        client
            .candidate_client(0)
            .expect("primary endpoint")
            .connect_single_endpoint()
            .await
            .expect("connect primary endpoint and initialize transport");
        let health = client.probe_endpoints().await;
        for endpoint in &health {
            println!(
                "{}: healthy={}, tip={:?}, reason={:?}",
                endpoint.endpoint, endpoint.healthy, endpoint.tip_height, endpoint.reason
            );
        }
        let healthy_count = health.iter().filter(|endpoint| endpoint.healthy).count();
        assert!(
            healthy_count >= 2,
            "automatic historical sync requires at least two canonical endpoints"
        );

        let tip = health
            .iter()
            .filter(|endpoint| endpoint.healthy)
            .filter_map(|endpoint| endpoint.tip_height)
            .max()
            .expect("healthy endpoint tip");
        let start = u32::try_from(tip.saturating_sub(2_048)).expect("mainnet height fits u32");
        let end = u32::try_from(tip.saturating_sub(128)).expect("mainnet height fits u32");
        let mut chunks = client.compact_block_segment_stream(
            start..end,
            HISTORICAL_STRIPE_HANDOFF_BYTES,
            HISTORICAL_STRIPE_BLOCKS,
            HISTORICAL_STRIPE_MAX_SOURCES,
            None,
        );
        let mut next_height = u64::from(start);
        let mut source_endpoints = std::collections::BTreeSet::new();
        while let Some(chunk) = chunks.recv().await {
            let chunk = chunk.expect("historical pool stream");
            assert_eq!(chunk.start_height(), Some(next_height));
            next_height = chunk.end_height().expect("non-empty chunk") + 1;
            source_endpoints.insert(chunk.endpoint);
        }
        assert_eq!(next_height, u64::from(end));
        assert!(
            source_endpoints.len() >= 2,
            "historical stream should use at least two validated endpoints; got {source_endpoints:?}"
        );
        println!("historical stream sources: {source_endpoints:?}");
        shutdown_transport().await;
    }

    #[tokio::test]
    #[ignore = "Requires live network connection"]
    async fn test_live_mainnet_subtree_root_capability_routing() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL).with_pirate_mainnet_auto_pool();
        let client = LightClient::with_config(config);
        client
            .candidate_client(0)
            .expect("primary endpoint")
            .connect_single_endpoint()
            .await
            .expect("connect primary endpoint and initialize transport");

        let health = client.probe_endpoints().await;
        assert!(
            health.iter().any(|endpoint| endpoint.healthy),
            "automatic endpoint pool has no canonical member"
        );
        let roots = client
            .get_subtree_roots(0, ShieldedProtocol::Sapling, 1)
            .await
            .expect("a validated endpoint should provide Sapling subtree roots");
        assert_eq!(roots.len(), 1);

        let capable = MAINNET_AUTO_LIGHTD_URLS
            .iter()
            .filter(|endpoint| {
                client.subtree_root_capability(endpoint, ShieldedProtocol::Sapling)
                    == Some(SubtreeRootCapability::Available)
            })
            .copied()
            .collect::<Vec<_>>();
        assert!(
            !capable.is_empty(),
            "successful subtree-root endpoint was not cached"
        );
        println!("Sapling subtree-root endpoints: {capable:?}");
        shutdown_transport().await;
    }

    #[tokio::test]
    #[ignore = "Requires live network connection"]
    async fn test_live_cryptoforge2_subtree_roots() {
        let client = LightClient::with_config(LightClientConfig::direct(
            "https://lightwalletd2.cryptoforge.cc:443",
        ));
        client.connect().await.expect("connect CryptoForge2");

        let roots = client
            .get_subtree_roots(0, ShieldedProtocol::Sapling, 1)
            .await
            .expect("CryptoForge2 should provide Sapling subtree roots");
        assert_eq!(roots.len(), 1);
        shutdown_transport().await;
    }

    #[tokio::test]
    #[ignore = "Requires live network connection"]
    async fn test_live_stale_pool_tip_refreshes_before_tail_fetch() {
        let config = LightClientConfig::direct(DEFAULT_LIGHTD_URL).with_pirate_mainnet_auto_pool();
        let client = LightClient::with_config(config);
        client
            .candidate_client(0)
            .expect("primary endpoint")
            .connect_single_endpoint()
            .await
            .expect("connect primary endpoint and initialize transport");
        let health = client.probe_endpoints().await;
        let highest_tip = health
            .iter()
            .filter(|endpoint| endpoint.healthy)
            .filter_map(|endpoint| endpoint.tip_height)
            .max()
            .expect("healthy canonical endpoint tip");

        {
            let mut state = client.endpoint_pool.write().await;
            let healthy_indices = state.healthy_indices.clone();
            for index in healthy_indices {
                state.tips.insert(index, highest_tip.saturating_sub(1));
            }
        }
        assert!(!client.candidate_order(highest_tip).await.is_empty());

        let state = client.endpoint_pool.read().await;
        assert!(state
            .healthy_indices
            .iter()
            .any(|index| state.tips.get(index).is_some_and(|tip| *tip >= highest_tip)));
        drop(state);
        shutdown_transport().await;
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
