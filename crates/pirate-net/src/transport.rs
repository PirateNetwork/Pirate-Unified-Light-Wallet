//! Privacy-preserving network transport layer
//!
//! Ensures all wallet traffic is tunneled through Tor/SOCKS5.

use crate::debug_log::log_debug_event;
use crate::lightwalletd_pins::extract_spki_from_cert_der;
use crate::{
    DnsConfig, DnsResolver, Error, I2pClient, I2pConfig, Result, TorClient, TorConfig, TorStatus,
};
use http::Uri;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::header::{HeaderName, HeaderValue, HOST, LOCATION};
use hyper::Request;
use hyper_util::rt::TokioIo;
use native_tls::TlsConnector as NativeTlsConnector;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_native_tls::TlsConnector;
use tokio_socks::tcp::Socks5Stream;
use tonic::transport::{Channel, Endpoint};
use tower::service_fn;
use tracing::{debug, error, info, warn};

/// Transport mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportMode {
    /// Tor (default, most private)
    Tor,
    /// I2P (desktop only)
    I2p,
    /// SOCKS5 proxy
    Socks5,
    /// Direct connection (NOT RECOMMENDED)
    Direct,
}

impl TransportMode {
    /// Get mode name
    pub fn name(&self) -> &str {
        match self {
            Self::Tor => "Tor (Most Private)",
            Self::I2p => "I2P (Desktop Only)",
            Self::Socks5 => "SOCKS5 Proxy",
            Self::Direct => "Direct (Not Private)",
        }
    }

    /// Check if mode is privacy-preserving
    pub fn is_private(&self) -> bool {
        !matches!(self, Self::Direct)
    }
}

/// SOCKS5 configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Socks5Config {
    /// Host address
    pub host: String,
    /// Port
    pub port: u16,
    /// Username (optional)
    pub username: Option<String>,
    /// Password (optional)
    pub password: Option<String>,
}

impl Socks5Config {
    /// Get proxy URL
    pub fn proxy_url(&self) -> String {
        if let (Some(user), Some(pass)) = (&self.username, &self.password) {
            format!("socks5h://{}:{}@{}:{}", user, pass, self.host, self.port)
        } else {
            format!("socks5h://{}:{}", self.host, self.port)
        }
    }
}

/// Transport configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportConfig {
    /// Transport mode
    pub mode: TransportMode,
    /// Tor configuration
    pub tor: TorConfig,
    /// I2P configuration (desktop only)
    pub i2p: I2pConfig,
    /// SOCKS5 config (if mode is SOCKS5)
    pub socks5: Option<Socks5Config>,
    /// DNS configuration
    pub dns_config: DnsConfig,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            mode: TransportMode::Tor,
            tor: TorConfig::default(),
            i2p: I2pConfig::default(),
            socks5: None,
            dns_config: DnsConfig::default(),
        }
    }
}

/// Privacy-preserving transport manager
pub struct TransportManager {
    config: Arc<RwLock<TransportConfig>>,
    tor_client: Arc<RwLock<Option<TorClient>>>,
    i2p_client: Arc<RwLock<Option<I2pClient>>>,
    dns_resolver: Arc<RwLock<DnsResolver>>,
    update_lock: Arc<Mutex<()>>,
}

#[allow(dead_code)]
fn _assert_transport_manager_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TransportManager>();
}

fn read_state<T: Clone>(state: &RwLock<T>) -> T {
    state
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

trait AsyncReadWrite: AsyncRead + AsyncWrite {}

impl<T: AsyncRead + AsyncWrite + ?Sized> AsyncReadWrite for T {}

type BoxedStream = Box<dyn AsyncReadWrite + Send + Unpin>;
type ConnectorStream = TokioIo<BoxedStream>;
type ConnectorFuture = Pin<Box<dyn Future<Output = Result<ConnectorStream>> + Send + 'static>>;

async fn fetch_peer_certificate_der(
    connector: TlsConnector,
    server_name: String,
    stream: BoxedStream,
) -> Result<Vec<u8>> {
    let stream = connector
        .connect(&server_name, stream)
        .await
        .map_err(|e| Error::Tls(format!("TLS handshake failed: {}", e)))?;
    let cert = stream
        .get_ref()
        .peer_certificate()
        .map_err(|e| Error::Tls(format!("TLS peer certificate error: {}", e)))?
        .ok_or_else(|| Error::Tls("No peer certificate presented".to_string()))?;
    cert.to_der()
        .map_err(|e| Error::Tls(format!("Failed to read DER certificate: {}", e)))
}

impl TransportManager {
    /// Whether this manager still owns the requested transport configuration.
    pub fn matches_config(&self, config: &TransportConfig) -> bool {
        read_state(&self.config) == *config
    }

    /// Create new transport manager
    pub async fn new(config: TransportConfig) -> Result<Self> {
        info!("Creating transport manager: mode={:?}", config.mode);
        let socks5_summary = config
            .socks5
            .as_ref()
            .map(|socks5| {
                let has_auth = socks5.username.as_ref().is_some_and(|u| !u.is_empty())
                    || socks5.password.as_ref().is_some_and(|p| !p.is_empty());
                format!("proxy={}:{} auth={}", socks5.host, socks5.port, has_auth)
            })
            .unwrap_or_else(|| "none".to_string());
        log_debug_event(
            "transport.rs:TransportManager::new",
            "transport_manager_new",
            &format!(
                "mode={:?} dns_provider={} dns_tunnel={} socks5={}",
                config.mode,
                config.dns_config.provider.name(),
                config.dns_config.tunnel_dns,
                socks5_summary
            ),
        );

        // Initialize Tor if enabled
        let tor_client = if config.mode == TransportMode::Tor && config.tor.enabled {
            info!("Initializing embedded Tor client...");
            let client = TorClient::new(config.tor.clone())?;

            // Bootstrap in background
            let client_clone = client.clone();
            tokio::spawn(async move {
                if let Err(e) = client_clone.bootstrap().await {
                    error!("Tor bootstrap failed: {}", e);
                }
            });

            Some(client)
        } else {
            None
        };

        // Initialize I2P if enabled
        let i2p_client = if config.mode == TransportMode::I2p && config.i2p.enabled {
            info!("Initializing embedded I2P router...");
            // Startup is performed by `ensure_ready` after this manager has
            // been installed as the single global owner. Starting here would
            // let a discarded racing manager orphan its native i2pd process.
            Some(I2pClient::new(config.i2p.clone())?)
        } else {
            None
        };

        let dns_resolver = DnsResolver::new(config.dns_config.clone());

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            tor_client: Arc::new(RwLock::new(tor_client)),
            i2p_client: Arc::new(RwLock::new(i2p_client)),
            dns_resolver: Arc::new(RwLock::new(dns_resolver)),
            update_lock: Arc::new(Mutex::new(())),
        })
    }

    /// Get current transport mode
    pub async fn mode(&self) -> TransportMode {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mode
    }

    /// Check if transport is privacy-preserving
    pub async fn is_private(&self) -> bool {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mode
            .is_private()
    }

    async fn ensure_tor_bootstrapped(self: Arc<Self>, config: TransportConfig) -> Result<()> {
        let client = {
            // Keep selection/publication atomic with update_config, but never
            // hold this lock across the potentially long bootstrap itself.
            let _update_guard = Arc::clone(&self.update_lock).lock_owned().await;
            if !self.matches_config(&config) {
                return Err(Error::Network(
                    "Transport changed before Tor startup".to_string(),
                ));
            }
            let mut current = self
                .tor_client
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(client) = current.as_ref() {
                client.clone()
            } else {
                log_debug_event(
                    "transport.rs:TransportManager::ensure_tor_bootstrapped",
                    "transport_update_config_tor_recreate",
                    "mode=Tor reason=missing_client",
                );
                let client = TorClient::new(config.tor)?;
                *current = Some(client.clone());
                client
            }
        };

        let status = client.status().await;
        if matches!(status, TorStatus::Ready) {
            log_debug_event(
                "transport.rs:TransportManager::ensure_tor_bootstrapped",
                "transport_update_config_skip",
                &format!(
                    "mode={:?} reason=config_unchanged status=ready",
                    config.mode
                ),
            );
            return Ok(());
        }

        log_debug_event(
            "transport.rs:TransportManager::ensure_tor_bootstrapped",
            "transport_update_config_tor_ensure",
            &format!("mode={:?} status={:?}", config.mode, status),
        );
        client.bootstrap().await?;
        Ok(())
    }

    async fn ensure_i2p_started(self: Arc<Self>, config: TransportConfig) -> Result<()> {
        let client = {
            // Publish a replacement before startup so update_config can see
            // and shut it down if the user changes mode while I2P is starting.
            let _update_guard = Arc::clone(&self.update_lock).lock_owned().await;
            if !self.matches_config(&config) {
                return Err(Error::Network(
                    "Transport changed before I2P startup".to_string(),
                ));
            }
            let mut current = self
                .i2p_client
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(client) = current.as_ref() {
                client.clone()
            } else {
                let client = I2pClient::new(config.i2p)?;
                *current = Some(client.clone());
                client
            }
        };

        client.start().await?;
        Ok(())
    }

    /// Ensure the active transport has completed any required startup.
    pub async fn ensure_ready(self: Arc<Self>) -> Result<()> {
        let config = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        match config.mode {
            TransportMode::Tor if config.tor.enabled => {
                self.clone().ensure_tor_bootstrapped(config).await?;
            }
            TransportMode::I2p if config.i2p.enabled => {
                self.clone().ensure_i2p_started(config).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Update transport configuration
    pub async fn update_config(self: Arc<Self>, config: TransportConfig) -> Result<()> {
        // Serialize config mutations to avoid concurrent Tor/I2P re-initialization
        // during rapid transport switches or parallel connection attempts.
        let _update_guard = Arc::clone(&self.update_lock).lock_owned().await;
        let current_config = self
            .config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if current_config == config {
            log_debug_event(
                "transport.rs:TransportManager::update_config",
                "transport_update_config_skip",
                &format!("mode={:?} reason=config_unchanged", config.mode),
            );
            return Ok(());
        }

        info!("Updating transport config: mode={:?}", config.mode);
        let socks5_summary = config
            .socks5
            .as_ref()
            .map(|socks5| {
                let has_auth = socks5.username.as_ref().is_some_and(|u| !u.is_empty())
                    || socks5.password.as_ref().is_some_and(|p| !p.is_empty());
                format!("proxy={}:{} auth={}", socks5.host, socks5.port, has_auth)
            })
            .unwrap_or_else(|| "none".to_string());
        log_debug_event(
            "transport.rs:TransportManager::update_config",
            "transport_update_config",
            &format!(
                "mode={:?} dns_provider={} dns_tunnel={} socks5={}",
                config.mode,
                config.dns_config.provider.name(),
                config.dns_config.tunnel_dns,
                socks5_summary
            ),
        );

        // Constructors validate the next configuration without starting a
        // native transport. Do this before retiring the current clients so a
        // malformed replacement cannot leave the manager half-configured.
        let next_tor = if config.mode == TransportMode::Tor && config.tor.enabled {
            info!("Initializing Tor client...");
            Some(TorClient::new(config.tor.clone())?)
        } else {
            None
        };
        let next_i2p = if config.mode == TransportMode::I2p && config.i2p.enabled {
            info!("Initializing I2P router...");
            Some(I2pClient::new(config.i2p.clone())?)
        } else {
            None
        };

        // Detach old clients before awaiting shutdown so any connector created
        // after this point cannot capture a transport that is being retired.
        let tor_current = self
            .tor_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(tor) = tor_current {
            tor.shutdown().await;
        }

        let i2p_current = self
            .i2p_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(i2p) = i2p_current {
            i2p.shutdown().await;
        }

        // Publish the new mode and fresh client handles without waiting for a
        // potentially minutes-long bootstrap. bootstrap_transport/connector
        // calls own readiness; a subsequent mode change can therefore acquire
        // update_lock immediately and cancel those clients via shutdown.
        *self
            .tor_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = next_tor;
        *self
            .i2p_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = next_i2p;

        // Update DNS resolver
        self.dns_resolver
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_config(config.dns_config.clone());

        // Update config
        *self
            .config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;

        Ok(())
    }

    /// Create HTTP client with configured transport
    pub async fn create_http_client(&self) -> Result<reqwest::Client> {
        let config = read_state(&self.config);

        let mut client_builder =
            reqwest::Client::builder().timeout(std::time::Duration::from_secs(60));

        match config.mode {
            TransportMode::Tor => {
                let tor = read_state(&self.tor_client);
                if let Some(tor) = tor {
                    if !tor.clone().is_ready().await {
                        warn!("Tor is not ready yet, waiting...");
                        // In production, we'd wait or return error
                    }

                    return Err(Error::Network(
                        "HTTP over Tor requires Arti stream integration (not available via SOCKS5)"
                            .to_string(),
                    ));
                } else {
                    return Err(Error::Network("Tor client not initialized".to_string()));
                }
            }
            TransportMode::I2p => {
                let i2p = read_state(&self.i2p_client)
                    .ok_or_else(|| Error::Network("I2P router not initialized".to_string()))?;
                i2p.clone().start().await?;
                let proxy = i2p.clone().proxy_config().await;
                let proxy_url = proxy.proxy_url();
                debug!("Creating HTTP client with I2P proxy: {}", proxy_url);

                let proxy = reqwest::Proxy::all(&proxy_url)
                    .map_err(|e| Error::Network(format!("Failed to create I2P proxy: {}", e)))?;

                client_builder = client_builder.proxy(proxy);
            }
            TransportMode::Socks5 => {
                if let Some(ref socks5) = config.socks5 {
                    let proxy_url = socks5.proxy_url();
                    debug!("Creating HTTP client with SOCKS5 proxy: {}", proxy_url);

                    let proxy = reqwest::Proxy::all(&proxy_url).map_err(|e| {
                        Error::Network(format!("Failed to create SOCKS5 proxy: {}", e))
                    })?;

                    client_builder = client_builder.proxy(proxy);
                } else {
                    return Err(Error::Network("SOCKS5 config not provided".to_string()));
                }
            }
            TransportMode::Direct => {
                warn!("Creating HTTP client with DIRECT mode - privacy not guaranteed!");
                // No proxy
            }
        }

        client_builder
            .build()
            .map_err(|e| Error::Network(format!("Failed to create HTTP client: {}", e)))
    }

    /// Fetch arbitrary HTTP(S) bytes using the configured transport.
    pub async fn fetch_url_bytes(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> Result<Vec<u8>> {
        let config = read_state(&self.config);

        match config.mode {
            TransportMode::Tor => {
                let tor = read_state(&self.tor_client)
                    .ok_or_else(|| Error::Network("Tor client not initialized".to_string()))?;
                fetch_url_bytes_via_tor(tor, url, headers, Duration::from_secs(60)).await
            }
            TransportMode::I2p => {
                require_i2p_url(url)?;
                let client = self.create_http_client().await?;
                fetch_url_bytes_with_client(&client, url, headers).await
            }
            _ => {
                let client = self.create_http_client().await?;
                fetch_url_bytes_with_client(&client, url, headers).await
            }
        }
    }

    fn grpc_connector(
        &self,
    ) -> impl tower::Service<Uri, Response = ConnectorStream, Error = Error, Future = ConnectorFuture>
           + Clone
           + Send
           + 'static {
        let config = read_state(&self.config);
        let tor_client = read_state(&self.tor_client);
        let i2p_client = read_state(&self.i2p_client);
        let dns_config = config.dns_config.clone();
        let socks5_config = config.socks5.clone();
        let mode = config.mode;

        service_fn(move |uri: Uri| -> ConnectorFuture {
            let tor_client = tor_client.clone();
            let i2p_client = i2p_client.clone();
            let dns_config = dns_config.clone();
            let socks5_config = socks5_config.clone();
            Box::pin(async move {
                match mode {
                    TransportMode::Tor => {
                        let tor = tor_client.ok_or_else(|| {
                            Error::Network("Tor client not initialized".to_string())
                        })?;
                        connect_via_tor(tor, uri).await
                    }
                    TransportMode::I2p => {
                        let i2p = i2p_client.ok_or_else(|| {
                            Error::Network("I2P router not initialized".to_string())
                        })?;
                        connect_via_i2p(i2p, uri).await
                    }
                    TransportMode::Socks5 => {
                        let socks5 = socks5_config.ok_or_else(|| {
                            Error::Network("SOCKS5 config not provided".to_string())
                        })?;
                        connect_via_socks5(socks5, uri).await
                    }
                    TransportMode::Direct => connect_direct(dns_config, mode, uri).await,
                }
            })
        })
    }

    fn log_grpc_channel_creation(&self, endpoint: &Endpoint, lazy: bool) {
        let config = read_state(&self.config);
        debug!("Creating gRPC channel via {:?}", config.mode);
        log_debug_event(
            "transport.rs:TransportManager::create_grpc_channel",
            "grpc_channel_create",
            &format!(
                "mode={:?} endpoint={} dns_provider={} dns_tunnel={} lazy={}",
                config.mode,
                endpoint.uri(),
                config.dns_config.provider.name(),
                config.dns_config.tunnel_dns,
                lazy
            ),
        );
    }

    /// Create a gRPC channel and wait for its transport connection.
    pub async fn create_grpc_channel(self: Arc<Self>, endpoint: Endpoint) -> Result<Channel> {
        self.log_grpc_channel_creation(&endpoint, false);
        let connector = self.grpc_connector();

        endpoint
            .connect_with_connector(connector)
            .await
            .map_err(|e| {
                Error::Network(format!(
                    "gRPC connection failed: {}",
                    format_error_chain(&e)
                ))
            })
    }

    /// Create a gRPC channel whose first RPC establishes the transport.
    ///
    /// This is used by concurrent health probes, which immediately issue an
    /// RPC and therefore observe the same connection and TLS failures.
    pub fn create_grpc_channel_lazy(self: Arc<Self>, endpoint: Endpoint) -> Channel {
        self.log_grpc_channel_creation(&endpoint, true);
        let connector = self.grpc_connector();

        endpoint.connect_with_connector_lazy(connector)
    }

    /// Open a raw stream using the configured transport mode.
    async fn open_stream(self: Arc<Self>, host: String, port: u16) -> Result<BoxedStream> {
        let config = read_state(&self.config);
        let tor_client = read_state(&self.tor_client);
        let i2p_client = read_state(&self.i2p_client);

        match config.mode {
            TransportMode::Tor => {
                let tor = tor_client
                    .ok_or_else(|| Error::Network("Tor client not initialized".to_string()))?;
                connect_tor_stream(tor, &host, port).await
            }
            TransportMode::I2p => {
                let i2p = i2p_client
                    .ok_or_else(|| Error::Network("I2P router not initialized".to_string()))?;
                connect_i2p_stream(i2p, &host, port).await
            }
            TransportMode::Socks5 => {
                let socks5 = config
                    .socks5
                    .ok_or_else(|| Error::Network("SOCKS5 config not provided".to_string()))?;
                connect_socks5_stream(socks5, &host, port).await
            }
            TransportMode::Direct => connect_direct_stream(config.dns_config, &host, port).await,
        }
    }

    /// Fetch the SPKI pin from the server using the configured transport.
    pub async fn fetch_spki_pin(
        self: Arc<Self>,
        host: String,
        port: u16,
        server_name: String,
    ) -> Result<String> {
        let stream = self.clone().open_stream(host, port).await?;
        let connector = NativeTlsConnector::builder()
            // A pin is an additional constraint on a normally valid TLS
            // identity, not a replacement for certificate and hostname checks.
            .danger_accept_invalid_certs(false)
            .danger_accept_invalid_hostnames(false)
            .build()
            .map_err(|e| Error::Tls(format!("TLS connector build failed: {}", e)))?;
        let connector = TlsConnector::from(connector);
        let der = fetch_peer_certificate_der(connector, server_name, stream).await?;
        extract_spki_from_cert_der(&der)
    }

    /// Resolve hostname via configured DNS
    pub async fn resolve_host(&self, hostname: &str) -> Result<Vec<std::net::IpAddr>> {
        let resolver = read_state(&self.dns_resolver);
        resolver.resolve(hostname).await
    }

    /// Get Tor bootstrap status
    pub async fn tor_status(&self) -> Option<crate::tor::TorStatus> {
        let tor = read_state(&self.tor_client);
        if let Some(tor) = tor {
            Some(tor.status().await)
        } else {
            None
        }
    }

    /// Get I2P startup status
    pub async fn i2p_status(&self) -> Option<crate::i2p::I2pStatus> {
        let i2p = read_state(&self.i2p_client);
        if let Some(i2p) = i2p {
            Some(i2p.status().await)
        } else {
            None
        }
    }

    /// Rotate Tor exit circuits by isolating future streams.
    pub async fn rotate_tor_exit(&self) -> Result<()> {
        let mode = read_state(&self.config).mode;
        if mode != TransportMode::Tor {
            log_debug_event(
                "transport.rs:TransportManager::rotate_tor_exit",
                "tor_exit_rotate_skip",
                &format!("mode={:?}", mode),
            );
            return Err(Error::Network(format!(
                "Tor exit rotation requested while mode is {:?}",
                mode
            )));
        }

        let tor = read_state(&self.tor_client)
            .ok_or_else(|| Error::Network("Tor client not initialized".to_string()))?;
        log_debug_event(
            "transport.rs:TransportManager::rotate_tor_exit",
            "tor_exit_rotate_start",
            "mode=Tor",
        );

        let mut before_ip: Option<String> = None;
        match tor.clone().fetch_exit_ip().await {
            Ok(ip) => {
                before_ip = Some(ip.clone());
                log_debug_event(
                    "transport.rs:TransportManager::rotate_tor_exit",
                    "tor_exit_ip_before",
                    &format!("ip={} source=checkip.amazonaws.com", ip),
                );
            }
            Err(e) => {
                log_debug_event(
                    "transport.rs:TransportManager::rotate_tor_exit",
                    "tor_exit_ip_error",
                    &format!("phase=before error={}", e),
                );
            }
        }

        tor.clone().rotate_exit().await;

        match tor.clone().fetch_exit_ip().await {
            Ok(ip) => {
                let changed = before_ip.as_ref() != Some(&ip);
                log_debug_event(
                    "transport.rs:TransportManager::rotate_tor_exit",
                    "tor_exit_ip_after",
                    &format!("ip={} changed={} source=checkip.amazonaws.com", ip, changed),
                );
            }
            Err(e) => {
                log_debug_event(
                    "transport.rs:TransportManager::rotate_tor_exit",
                    "tor_exit_ip_error",
                    &format!("phase=after error={}", e),
                );
            }
        }

        Ok(())
    }

    /// Shutdown transport (cleanup)
    pub async fn shutdown(&self) {
        info!("Shutting down transport manager...");

        if let Some(tor) = read_state(&self.tor_client) {
            tor.shutdown().await;
        }
        *self
            .tor_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        if let Some(i2p) = read_state(&self.i2p_client) {
            i2p.shutdown().await;
        }
        *self
            .i2p_client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
}

fn format_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause = cause.to_string();
        if !cause.is_empty() && !message.contains(&cause) {
            message.push_str(": ");
            message.push_str(&cause);
        }
        source = source.and_then(std::error::Error::source);
    }
    message
}

async fn fetch_url_bytes_with_client(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
) -> Result<Vec<u8>> {
    let mut request = client.get(url);
    for (name, value) in headers {
        request = request.header(name, value);
    }

    let response = request
        .send()
        .await
        .map_err(|e| Error::Network(format!("HTTP request failed: {}", e)))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|e| Error::Network(format!("HTTP response read failed: {}", e)))?;

    if !status.is_success() {
        let preview = String::from_utf8_lossy(&body);
        return Err(Error::Network(format!(
            "HTTP request failed with status {}: {}",
            status,
            preview.chars().take(256).collect::<String>()
        )));
    }

    Ok(body.to_vec())
}

async fn fetch_url_bytes_via_tor(
    tor: TorClient,
    url: &str,
    headers: &[(String, String)],
    timeout: Duration,
) -> Result<Vec<u8>> {
    let mut current = reqwest::Url::parse(url)
        .map_err(|e| Error::Network(format!("Invalid URL '{}': {}", url, e)))?;

    for _ in 0..=5 {
        let (status, location, body) =
            fetch_url_once_via_tor(tor.clone(), &current, headers, timeout).await?;

        if status.is_redirection() {
            let location = location.ok_or_else(|| {
                Error::Network(format!(
                    "Redirect from '{}' missing Location header",
                    current
                ))
            })?;
            current = current.join(&location).map_err(|e| {
                Error::Network(format!(
                    "Invalid redirect target '{}' from '{}': {}",
                    location, url, e
                ))
            })?;
            continue;
        }

        if !status.is_success() {
            let preview = String::from_utf8_lossy(&body);
            return Err(Error::Network(format!(
                "HTTP request failed with status {}: {}",
                status,
                preview.chars().take(256).collect::<String>()
            )));
        }

        return Ok(body);
    }

    Err(Error::Network(format!(
        "Too many redirects while fetching '{}'",
        url
    )))
}

async fn fetch_url_once_via_tor(
    tor: TorClient,
    url: &reqwest::Url,
    headers: &[(String, String)],
    timeout: Duration,
) -> Result<(http::StatusCode, Option<String>, Vec<u8>)> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::Network(format!("URL '{}' is missing a host", url)))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| Error::Network(format!("URL '{}' is missing a port", url)))?;

    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }

    let default_port = match url.scheme() {
        "http" => 80,
        "https" => 443,
        other => {
            return Err(Error::Network(format!(
                "Unsupported URL scheme '{}' for '{}'",
                other, url
            )))
        }
    };

    let host_header = if port == default_port {
        host.to_string()
    } else {
        format!("{}:{}", host, port)
    };

    let stream = tor.connect_stream(host, port).await?;

    let mut request = Request::builder().method("GET").uri(path);
    request = request.header(HOST, host_header);

    for (name, value) in headers {
        let header_name = HeaderName::try_from(name.as_str())
            .map_err(|e| Error::Network(format!("Invalid header name '{}': {}", name, e)))?;
        let header_value = HeaderValue::from_str(value)
            .map_err(|e| Error::Network(format!("Invalid value for header '{}': {}", name, e)))?;
        request = request.header(header_name, header_value);
    }

    let request = request
        .body(Empty::<Bytes>::new())
        .map_err(|e| Error::Network(format!("Failed to build HTTP request: {}", e)))?;

    let (status, location, body) = if url.scheme() == "https" {
        let connector = NativeTlsConnector::builder()
            .danger_accept_invalid_certs(false)
            .danger_accept_invalid_hostnames(false)
            .build()
            .map_err(|e| Error::Tls(format!("TLS connector build failed: {}", e)))?;
        let connector = TlsConnector::from(connector);
        let tls_stream = connector
            .connect(host, stream)
            .await
            .map_err(|e| Error::Tls(format!("TLS handshake failed: {}", e)))?;
        let io = TokioIo::new(tls_stream);
        fetch_over_tunnel_stream(io, request, timeout).await?
    } else {
        let io = TokioIo::new(stream);
        fetch_over_tunnel_stream(io, request, timeout).await?
    };

    Ok((status, location, body))
}

async fn fetch_over_tunnel_stream<T>(
    io: TokioIo<T>,
    request: Request<Empty<Bytes>>,
    timeout: Duration,
) -> Result<(http::StatusCode, Option<String>, Vec<u8>)>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sender, conn) = tokio::time::timeout(timeout, http1::handshake(io))
        .await
        .map_err(|_| Error::Network("HTTP handshake timed out".to_string()))?
        .map_err(|e| Error::Network(format!("HTTP handshake failed: {}", e)))?;

    tokio::spawn(async move {
        if let Err(err) = conn.await {
            warn!("HTTP tunnel connection error: {}", err);
        }
    });

    let response = tokio::time::timeout(timeout, sender.send_request(request))
        .await
        .map_err(|_| Error::Network("HTTP request timed out".to_string()))?
        .map_err(|e| Error::Network(format!("HTTP request failed: {}", e)))?;

    let status = response.status();
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let body = tokio::time::timeout(timeout, response.into_body().collect())
        .await
        .map_err(|_| Error::Network("HTTP response timed out".to_string()))?
        .map_err(|e| Error::Network(format!("HTTP body read failed: {}", e)))?
        .to_bytes()
        .to_vec();

    Ok((status, location, body))
}

fn uri_host_port(uri: &Uri) -> Result<(String, u16)> {
    let host = uri
        .host()
        .ok_or_else(|| Error::Network("Endpoint missing host".to_string()))?
        .to_string();
    let port = uri.port_u16().unwrap_or_else(|| {
        if uri.scheme_str() == Some("https") {
            443
        } else {
            80
        }
    });
    Ok((host, port))
}

async fn connect_direct(
    mut dns_config: DnsConfig,
    mode: TransportMode,
    uri: Uri,
) -> Result<ConnectorStream> {
    let (host, port) = uri_host_port(&uri)?;
    let dns_provider = dns_config.provider.name().to_string();
    let leak_guard = mode != TransportMode::Direct;
    log_debug_event(
        "transport.rs:connect_direct",
        "connect_direct_start",
        &format!(
            "host={} port={} dns_provider={} dns_tunnel={} mode={:?} leak_guard={}",
            host, port, dns_provider, dns_config.tunnel_dns, mode, leak_guard
        ),
    );
    if leak_guard {
        log_debug_event(
            "transport.rs:connect_direct",
            "connect_direct_leak_guard",
            &format!("host={} port={} mode={:?}", host, port, mode),
        );
    }

    if let Ok(ip) = host.parse() {
        let addr = SocketAddr::new(ip, port);
        let stream = TcpStream::connect(addr).await?;
        log_debug_event(
            "transport.rs:connect_direct",
            "connect_direct_ok",
            &format!("host={} port={} via=ip", host, port),
        );
        return Ok(TokioIo::new(Box::new(stream)));
    }

    dns_config.tunnel_dns = false;
    dns_config.socks_proxy = None;
    let resolver = DnsResolver::new(dns_config);
    let addrs = resolver.resolve_owned(host.clone()).await?;
    let mut last_err = None;

    for ip in addrs {
        let addr = SocketAddr::new(ip, port);
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                log_debug_event(
                    "transport.rs:connect_direct",
                    "connect_direct_ok",
                    &format!("host={} port={} via=dns", host, port),
                );
                return Ok(TokioIo::new(Box::new(stream)));
            }
            Err(e) => last_err = Some(e),
        }
    }

    let error = Error::Connection(format!(
        "Direct connection to {}:{} failed: {:?}",
        host, port, last_err
    ));
    log_debug_event(
        "transport.rs:connect_direct",
        "connect_direct_error",
        &format!("host={} port={} error={}", host, port, error),
    );
    Err(error)
}

async fn connect_direct_stream(
    mut dns_config: DnsConfig,
    host: &str,
    port: u16,
) -> Result<BoxedStream> {
    if let Ok(ip) = host.parse() {
        let addr = SocketAddr::new(ip, port);
        let stream = TcpStream::connect(addr).await?;
        return Ok(Box::new(stream));
    }

    dns_config.tunnel_dns = false;
    dns_config.socks_proxy = None;
    let resolver = DnsResolver::new(dns_config);
    let addrs = resolver.resolve(host).await?;
    let mut last_err = None;

    for ip in addrs {
        let addr = SocketAddr::new(ip, port);
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                return Ok(Box::new(stream));
            }
            Err(e) => last_err = Some(e),
        }
    }

    Err(Error::Connection(format!(
        "Direct connection to {}:{} failed: {:?}",
        host, port, last_err
    )))
}

async fn connect_via_socks5(socks5: Socks5Config, uri: Uri) -> Result<ConnectorStream> {
    let (host, port) = uri_host_port(&uri)?;
    let proxy_addr = (socks5.host.as_str(), socks5.port);
    let has_auth = socks5.username.as_ref().is_some_and(|u| !u.is_empty())
        || socks5.password.as_ref().is_some_and(|p| !p.is_empty());
    log_debug_event(
        "transport.rs:connect_via_socks5",
        "connect_socks5_start",
        &format!(
            "target={}:{} proxy={}:{} auth={}",
            host, port, socks5.host, socks5.port, has_auth
        ),
    );

    let result = match (socks5.username.as_ref(), socks5.password.as_ref()) {
        (Some(user), Some(pass)) => {
            Socks5Stream::connect_with_password(proxy_addr, (host.as_str(), port), user, pass)
                .await
                .map_err(|e| Error::Network(format!("SOCKS5 connect failed: {}", e)))
        }
        _ => Socks5Stream::connect(proxy_addr, (host.as_str(), port))
            .await
            .map_err(|e| Error::Network(format!("SOCKS5 connect failed: {}", e))),
    };
    let stream = result.map_err(|error| {
        log_debug_event(
            "transport.rs:connect_via_socks5",
            "connect_socks5_error",
            &format!(
                "target={}:{} proxy={}:{} auth={} error={}",
                host, port, socks5.host, socks5.port, has_auth, error
            ),
        );
        error
    })?;

    log_debug_event(
        "transport.rs:connect_via_socks5",
        "connect_socks5_ok",
        &format!(
            "target={}:{} proxy={}:{} auth={}",
            host, port, socks5.host, socks5.port, has_auth
        ),
    );
    Ok(TokioIo::new(Box::new(stream)))
}

async fn connect_socks5_stream(socks5: Socks5Config, host: &str, port: u16) -> Result<BoxedStream> {
    let proxy_addr = (socks5.host.as_str(), socks5.port);
    let stream = match (socks5.username.as_ref(), socks5.password.as_ref()) {
        (Some(user), Some(pass)) => {
            Socks5Stream::connect_with_password(proxy_addr, (host, port), user, pass)
                .await
                .map_err(|e| Error::Network(format!("SOCKS5 connect failed: {}", e)))?
        }
        _ => Socks5Stream::connect(proxy_addr, (host, port))
            .await
            .map_err(|e| Error::Network(format!("SOCKS5 connect failed: {}", e)))?,
    };
    Ok(Box::new(stream))
}

async fn connect_via_tor(tor: TorClient, uri: Uri) -> Result<ConnectorStream> {
    let (host, port) = uri_host_port(&uri)?;
    let status = tor.clone().status_owned().await;
    log_debug_event(
        "transport.rs:connect_via_tor",
        "connect_tor_start",
        &format!("target={}:{} status={:?}", host, port, status),
    );
    match tor.connect_stream_owned(host.clone(), port).await {
        Ok(stream) => {
            log_debug_event(
                "transport.rs:connect_via_tor",
                "connect_tor_ok",
                &format!("target={}:{} status={:?}", host, port, status),
            );
            Ok(TokioIo::new(Box::new(stream)))
        }
        Err(e) => {
            log_debug_event(
                "transport.rs:connect_via_tor",
                "connect_tor_error",
                &format!("target={}:{} error={}", host, port, e),
            );
            Err(e)
        }
    }
}

async fn connect_tor_stream(tor: TorClient, host: &str, port: u16) -> Result<BoxedStream> {
    let stream = tor.connect_stream_owned(host.to_string(), port).await?;
    Ok(Box::new(stream))
}

async fn connect_via_i2p(i2p: I2pClient, uri: Uri) -> Result<ConnectorStream> {
    let (host, _) = uri_host_port(&uri)?;
    if !is_i2p_destination(&host) {
        return Err(Error::Network(format!(
            "I2P transport refuses non-I2P destination '{}'",
            host
        )));
    }
    let status = i2p.clone().status().await;
    log_debug_event(
        "transport.rs:connect_via_i2p",
        "connect_i2p_start",
        &format!("status={:?}", status),
    );
    i2p.clone().start().await?;
    let proxy = i2p.clone().proxy_config().await;
    log_debug_event(
        "transport.rs:connect_via_i2p",
        "connect_i2p_proxy",
        &format!("proxy={}:{} auth=false", proxy.host, proxy.port),
    );
    let _connection_guard = i2p.clone().connection_guard().await;
    let retry_delays = [0_u64, 1, 2, 4, 6];
    let mut last_error = None;
    for (attempt, delay_secs) in retry_delays.into_iter().enumerate() {
        if delay_secs > 0 {
            tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        }
        match connect_via_socks5(proxy.clone(), uri.clone()).await {
            Ok(stream) => return Ok(stream),
            Err(error) => {
                log_debug_event(
                    "transport.rs:connect_via_i2p",
                    "connect_i2p_retry",
                    &format!(
                        "target={} attempt={} max_attempts={} error={}",
                        host,
                        attempt + 1,
                        retry_delays.len(),
                        error
                    ),
                );
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| Error::Network("I2P connection failed".to_string())))
}

async fn connect_i2p_stream(i2p: I2pClient, host: &str, port: u16) -> Result<BoxedStream> {
    if !is_i2p_destination(host) {
        return Err(Error::Network(format!(
            "I2P transport refuses non-I2P destination '{}'",
            host
        )));
    }
    i2p.clone().start().await?;
    let proxy = i2p.clone().proxy_config().await;
    let _connection_guard = i2p.clone().connection_guard().await;
    connect_socks5_stream(proxy, host, port).await
}

fn is_i2p_destination(host: &str) -> bool {
    let normalized = host.trim_end_matches('.').to_ascii_lowercase();
    normalized.ends_with(".i2p")
}

fn require_i2p_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| Error::Network(format!("Invalid URL for I2P transport: {error}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Network("I2P URL has no destination host".to_string()))?;

    if !is_i2p_destination(host) {
        return Err(Error::Network(format!(
            "I2P transport refuses non-I2P URL destination '{host}'"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_mode_privacy() {
        assert!(TransportMode::Tor.is_private());
        assert!(TransportMode::I2p.is_private());
        assert!(TransportMode::Socks5.is_private());
        assert!(!TransportMode::Direct.is_private());
    }

    #[test]
    fn test_socks5_proxy_url() {
        let config = Socks5Config {
            host: "localhost".to_string(),
            port: 9050,
            username: None,
            password: None,
        };
        assert_eq!(config.proxy_url(), "socks5h://localhost:9050");

        let config_auth = Socks5Config {
            host: "proxy.example.com".to_string(),
            port: 1080,
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
        };
        assert_eq!(
            config_auth.proxy_url(),
            "socks5h://user:pass@proxy.example.com:1080"
        );
    }

    #[test]
    fn i2p_transport_accepts_only_i2p_destinations() {
        assert!(is_i2p_destination("example.i2p"));
        assert!(is_i2p_destination("HASH.B32.I2P."));
        assert!(!is_i2p_destination("example.com"));
        assert!(!is_i2p_destination("example.onion"));

        assert!(require_i2p_url("https://example.i2p/path").is_ok());
        assert!(require_i2p_url("http://hash.b32.i2p:9067/").is_ok());
        assert!(require_i2p_url("https://example.com/").is_err());
        assert!(require_i2p_url("https://example.onion/").is_err());
    }

    #[tokio::test]
    async fn test_transport_manager_creation() {
        let config = TransportConfig {
            mode: TransportMode::Direct, // Avoid Tor bootstrap in test
            ..Default::default()
        };

        let manager = TransportManager::new(config.clone()).await.unwrap();
        assert_eq!(manager.mode().await, TransportMode::Direct);
        assert!(manager.matches_config(&config));

        let mut other = config;
        other.mode = TransportMode::Socks5;
        assert!(!manager.matches_config(&other));
    }

    #[tokio::test]
    async fn config_updates_do_not_wait_for_private_transport_bootstrap() {
        let direct = TransportConfig {
            mode: TransportMode::Direct,
            ..TransportConfig::default()
        };
        let manager = Arc::new(
            TransportManager::new(direct.clone())
                .await
                .expect("manager"),
        );
        let i2p = TransportConfig {
            mode: TransportMode::I2p,
            i2p: I2pConfig {
                enabled: true,
                binary_path: Some(std::path::PathBuf::from("missing-i2pd-for-config-test")),
                ..I2pConfig::default()
            },
            ..TransportConfig::default()
        };

        tokio::time::timeout(
            Duration::from_secs(1),
            Arc::clone(&manager).update_config(i2p.clone()),
        )
        .await
        .expect("config publication must not await I2P startup")
        .expect("I2P config");
        assert!(manager.matches_config(&i2p));

        tokio::time::timeout(
            Duration::from_secs(1),
            Arc::clone(&manager).update_config(direct.clone()),
        )
        .await
        .expect("switching away must remain responsive")
        .expect("direct config");
        assert!(manager.matches_config(&direct));
    }
}
