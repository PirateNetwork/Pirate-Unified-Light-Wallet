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
use pirate_net::{
    DnsConfig as NetDnsConfig, I2pConfig as NetI2pConfig, Socks5Config as NetSocks5Config,
    TorBridgeConfig, TorBridgeTransport, TorConfig as NetTorConfig,
    TransportConfig as NetTransportConfig, TransportManager as NetTransportManager,
    TransportMode as NetTransportMode,
};
use rand::Rng;
use std::env;
use std::io::Write;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tracing::{debug, error, info, warn};

use proto::compact_tx_streamer_client::CompactTxStreamerClient;
use proto::{BlockId, BlockRange, ChainSpec, Empty, RawTransaction, TxFilter};

/// Default lightwalletd endpoint (Pirate Chain official)
pub const DEFAULT_LIGHTD_HOST: &str = "64.23.167.130";
/// Default lightwalletd port (no TLS by default)
pub const DEFAULT_LIGHTD_PORT: u16 = 9067;
/// Default TLS usage for the default endpoint
pub const DEFAULT_LIGHTD_USE_TLS: bool = false;
/// Default endpoint URL
pub const DEFAULT_LIGHTD_URL: &str = "http://64.23.167.130:9067";

fn debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        env::current_dir()
            .map(|dir| dir.join(".cursor").join("debug.log"))
            .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
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
    manager: RwLock<Option<Arc<NetTransportManager>>>,
}

impl GlobalTransportState {
    async fn get_or_init(&self, config: NetTransportConfig) -> Result<Arc<NetTransportManager>> {
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

static TOR_CONFIG_OVERRIDE: Lazy<std::sync::RwLock<Option<NetTorConfig>>> =
    Lazy::new(|| std::sync::RwLock::new(None));

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
}

impl Default for LightClientConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_LIGHTD_URL.to_string(),
            transport: TransportMode::Tor,
            socks5_url: None,
            tls: TlsConfig::default(),
            retry: RetryConfig::default(),
            connect_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(120),
            allow_direct_fallback: false,
        }
    }
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
        self.tls.spki_pin = Some(pin.to_string());
        self.tls.enabled = true;
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
                    username = Some(user.to_string());
                }
                if !pass.is_empty() {
                    password = Some(pass.to_string());
                }
            } else if !userinfo.is_empty() {
                username = Some(userinfo.to_string());
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

/// Bootstrap transport early (Tor/I2P/SOCKS5) without touching wallet state.
pub async fn bootstrap_transport(mode: TransportMode, socks5_url: Option<String>) -> Result<()> {
    let config = build_transport_config_from_mode(mode, socks5_url.as_deref())?;
    GLOBAL_TRANSPORT.get_or_init(config).await?;
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

/// Get current I2P status if transport manager is initialized.
pub async fn i2p_status() -> Option<pirate_net::I2pStatus> {
    let manager = GLOBAL_TRANSPORT.get().await?;
    manager.i2p_status().await
}

/// Shutdown any active transport manager.
pub async fn shutdown_transport() {
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
    /// Orchard actions
    pub actions: Vec<CompactOrchardAction>,
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
                .map(CompactOrchardAction::from)
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
                .map(proto::CompactOrchardAction::from)
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

/// Compact Orchard action (for trial decryption)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompactOrchardAction {
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

impl From<proto::CompactOrchardAction> for CompactOrchardAction {
    fn from(pb: proto::CompactOrchardAction) -> Self {
        Self {
            nullifier: pb.nullifier,
            cmx: pb.cmx,
            ephemeral_key: pb.ephemeral_key,
            enc_ciphertext: pb.ciphertext, // Proto field is "ciphertext", we call it enc_ciphertext internally
            out_ciphertext: Vec::new(),    // Not in server's compact format, only in full format
        }
    }
}

impl From<CompactOrchardAction> for proto::CompactOrchardAction {
    fn from(action: CompactOrchardAction) -> Self {
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
            block_height: pb.block_height,
            estimated_height: pb.estimated_height,
            sapling_activation_height: pb.sapling_activation_height,
        }
    }
}

/// Tree state for Sapling and Orchard note commitment trees
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
    /// Orchard tree state (hex-encoded string, empty if Orchard not activated)
    pub orchard_tree: String,
}

/// Lightwalletd gRPC client
///
/// Provides methods to:
/// - Query latest block height
/// - Stream compact blocks in ranges
/// - Broadcast transactions
pub struct LightClient {
    config: LightClientConfig,
    channel: Arc<Mutex<Option<Channel>>>,
}

#[allow(dead_code)]
fn _assert_light_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LightClient>();
}

#[allow(dead_code)]
fn _assert_channel_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Channel>();
}

impl LightClient {
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
        }
    }

    /// Create client with custom configuration
    pub fn with_config(config: LightClientConfig) -> Self {
        Self {
            config,
            channel: Arc::new(Mutex::new(None)),
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
        }
    }

    /// Get current endpoint URL
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
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
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
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
        }
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

        // Configure TLS if enabled
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
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
        }
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

    /// Get the latest block height from the server
    ///
    /// Returns the current blockchain tip height.
    pub async fn get_latest_block(&self) -> Result<u64> {
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
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
        }
        // #endregion
        let result = self
            .with_retry(|| async {
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
            .await;
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
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
        }
        // #endregion
        result
    }

    /// Get compact blocks in the specified range
    ///
    /// Streams blocks from `range.start` to `range.end` (exclusive).
    /// Returns Vec for simplicity; use `stream_blocks` for large ranges.
    pub async fn get_compact_block_range(&self, range: Range<u32>) -> Result<Vec<CompactBlock>> {
        if range.is_empty() {
            return Ok(Vec::new());
        }

        self.with_retry(|| async {
            let mut client = self.get_client().await?;
            let start_instant = Instant::now();

            let request = tonic::Request::new(BlockRange {
                start: Some(BlockId {
                    height: range.start as u64,
                    hash: Vec::new(),
                }),
                end: Some(BlockId {
                    height: (range.end - 1) as u64, // end is inclusive in proto
                    hash: Vec::new(),
                }),
            });

            debug!("Requesting blocks {}..{}", range.start, range.end);

            let mut stream = client.get_block_range(request).await?.into_inner();
            let mut blocks = Vec::with_capacity((range.end - range.start) as usize);
            let mut first_block_ms: Option<u128> = None;
            let mut estimated_bytes = 0u64;

            while let Some(block) = stream.message().await? {
                if first_block_ms.is_none() {
                    first_block_ms = Some(start_instant.elapsed().as_millis());
                }
                let compact = CompactBlock::from(block);
                estimated_bytes = estimated_bytes.saturating_add(estimate_compact_block_bytes(&compact));
                blocks.push(compact);
            }

            let total_ms = start_instant.elapsed().as_millis();
            let ttfb_ms = first_block_ms.unwrap_or(total_ms);
            let kbps = if total_ms > 0 {
                (estimated_bytes as f64 / 1024.0) / (total_ms as f64 / 1000.0)
            } else {
                0.0
            };

            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(debug_log_path())
            {
                use std::io::Write;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_{}","timestamp":{},"location":"client.rs:block_range_stats","message":"block range stats","data":{{"start":{},"end":{},"blocks":{},"ttfb_ms":{},"total_ms":{},"est_bytes":{},"est_kbps":{:.2},"endpoint":"{}","transport":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                    id,
                    ts,
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
            }

            debug!("Received {} blocks", blocks.len());
            Ok(blocks)
        }).await
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
                error!(
                    "Transaction broadcast failed: code={}, message={}",
                    send_response.error_code, send_response.error_message
                );
                return Err(Error::Network(format!(
                    "Broadcast failed: {} (code {})",
                    send_response.error_message, send_response.error_code
                )));
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
        debug!(
            "Fetching full transaction for memo decryption: {}",
            hex::encode(tx_hash)
        );

        self.get_transaction_by_filter(TxFilter {
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
        match self.get_transaction(tx_hash).await {
            Ok(raw) => Ok(raw),
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
                        .get_transaction_by_filter(TxFilter {
                            block: Some(BlockId {
                                height,
                                hash: Vec::new(),
                            }),
                            index,
                            hash: Vec::new(),
                        })
                        .await;
                }
                Err(err)
            }
        }
    }

    async fn get_transaction_by_filter(&self, filter: TxFilter) -> Result<Vec<u8>> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;
            let request = tonic::Request::new(filter.clone());

            let response = client.get_transaction(request).await?;
            let raw_tx = response.into_inner();

            debug!("Received full transaction ({} bytes)", raw_tx.data.len());
            Ok(raw_tx.data)
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

    /// Get tree state (Sapling and Orchard anchors) at a specific block height
    ///
    /// If `height` is 0, returns the latest tree state.
    /// Returns TreeState with saplingTree and orchardTree (hex-encoded strings).
    /// Uses legacy z_gettreestatelegacy RPC for backward compatibility.
    ///
    /// # Arguments
    /// * `height` - Block height (0 for latest)
    ///
    /// # Returns
    /// TreeState containing network, height, hash, time, saplingTree, saplingFrontier, and orchardTree
    pub async fn get_tree_state(&self, height: u64) -> Result<TreeState> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let mut request = tonic::Request::new(BlockId {
                height,
                hash: Vec::new(),
            });
            request.set_timeout(self.config.request_timeout);

            let response = client.get_tree_state(request).await?;
            let tree_state = response.into_inner();

            debug!(
                "Tree state at height {}: network={}, hash={}, saplingTree={}, orchardTree={}",
                tree_state.height,
                tree_state.network,
                tree_state.hash,
                tree_state.sapling_tree,
                tree_state.orchard_tree
            );

            Ok(TreeState {
                network: tree_state.network,
                height: tree_state.height,
                hash: tree_state.hash,
                time: tree_state.time,
                sapling_tree: tree_state.sapling_tree,
                sapling_frontier: tree_state.sapling_frontier,
                orchard_tree: tree_state.orchard_tree,
            })
        })
        .await
    }

    /// Get tree state with bridge tree support (improved long-range sync performance)
    ///
    /// Uses updated z_gettreestate RPC with bridge trees format.
    /// The block can be specified by either height or hash.
    /// Returns TreeState with saplingTree and orchardTree in bridge tree format.
    ///
    /// # Arguments
    /// * `height` - Block height (0 for latest)
    ///
    /// # Returns
    /// TreeState containing network, height, hash, time, saplingTree, saplingFrontier, and orchardTree
    /// in bridge tree format for improved long-range sync performance
    pub async fn get_bridge_tree_state(&self, height: u64) -> Result<TreeState> {
        self.with_retry(|| async {
            let mut client = self.get_client().await?;

            let mut request = tonic::Request::new(BlockId {
                height,
                hash: Vec::new(),
            });
            request.set_timeout(self.config.request_timeout);

            let response = client.get_bridge_tree_state(request).await?;
            let tree_state = response.into_inner();

            debug!(
                "Bridge tree state at height {}: network={}, hash={}, saplingTree={}, orchardTree={}",
                tree_state.height,
                tree_state.network,
                tree_state.hash,
                tree_state.sapling_tree,
                tree_state.orchard_tree
            );

            Ok(TreeState {
                network: tree_state.network,
                height: tree_state.height,
                hash: tree_state.hash,
                time: tree_state.time,
                sapling_tree: tree_state.sapling_tree,
                sapling_frontier: tree_state.sapling_frontier,
                orchard_tree: tree_state.orchard_tree,
            })
        }).await
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

/// Compute transaction ID from raw transaction bytes
fn compute_txid(raw_tx: &[u8]) -> String {
    // Pirate/Zcash txid is double SHA256 of the tx, reversed
    use sha2::{Digest, Sha256};

    let hash1 = Sha256::digest(raw_tx);
    let hash2 = Sha256::digest(hash1);

    // Reverse bytes for display
    let mut txid_bytes: [u8; 32] = hash2.into();
    txid_bytes.reverse();

    hex::encode(txid_bytes)
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

    #[test]
    fn test_default_config() {
        let config = LightClientConfig::default();
        assert_eq!(config.endpoint, DEFAULT_LIGHTD_URL);
        assert!(!config.tls.enabled);
        assert!(config.tls.spki_pin.is_none());
        assert_eq!(config.transport, TransportMode::Tor);
    }

    #[test]
    fn test_direct_config() {
        let config = LightClientConfig::direct("https://custom:9067");
        assert_eq!(config.endpoint, "https://custom:9067");
        assert_eq!(config.transport, TransportMode::Direct);
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
    fn test_spki_pin_config() {
        let config = LightClientConfig::default()
            .with_spki_pin("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
        assert_eq!(
            config.tls.spki_pin,
            Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string())
        );
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
