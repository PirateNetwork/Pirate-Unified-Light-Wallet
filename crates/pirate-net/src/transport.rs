//! Privacy-preserving network transport layer
//!
//! Ensures all wallet traffic is tunneled through Tor/SOCKS5.

use crate::debug_log::log_debug_event;
use crate::lightwalletd_pins::extract_spki_from_cert_der;
use crate::{DnsConfig, DnsResolver, Error, I2pClient, I2pConfig, Result, TorClient, TorConfig};
use http::Uri;
use hyper_util::rt::TokioIo;
use native_tls::TlsConnector as NativeTlsConnector;
use std::net::SocketAddr;
use std::sync::Arc;
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
    config: Arc<Mutex<TransportConfig>>,
    tor_client: Arc<Mutex<Option<TorClient>>>,
    i2p_client: Arc<Mutex<Option<I2pClient>>>,
    dns_resolver: Arc<Mutex<DnsResolver>>,
}

#[allow(dead_code)]
fn _assert_transport_manager_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TransportManager>();
}

trait AsyncReadWrite: AsyncRead + AsyncWrite {}

impl<T: AsyncRead + AsyncWrite + ?Sized> AsyncReadWrite for T {}

type BoxedStream = Box<dyn AsyncReadWrite + Send + Unpin>;
type ConnectorStream = TokioIo<BoxedStream>;

impl TransportManager {
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
            let client = I2pClient::new(config.i2p.clone())?;
            let client_clone = client.clone();
            tokio::spawn(async move {
                if let Err(e) = client_clone.start().await {
                    error!("I2P startup failed: {}", e);
                }
            });
            Some(client)
        } else {
            None
        };

        let dns_resolver = DnsResolver::new(config.dns_config.clone());

        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            tor_client: Arc::new(Mutex::new(tor_client)),
            i2p_client: Arc::new(Mutex::new(i2p_client)),
            dns_resolver: Arc::new(Mutex::new(dns_resolver)),
        })
    }

    /// Get current transport mode
    pub async fn mode(&self) -> TransportMode {
        self.config.lock().await.mode
    }

    /// Check if transport is privacy-preserving
    pub async fn is_private(&self) -> bool {
        self.config.lock().await.mode.is_private()
    }

    /// Update transport configuration
    pub async fn update_config(&self, config: TransportConfig) -> Result<()> {
        let current_config = { self.config.lock().await.clone() };
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

        let tor_current = { self.tor_client.lock().await.clone() };
        if config.mode == TransportMode::Tor && config.tor.enabled {
            if let Some(tor) = tor_current {
                tor.clone().update_config(config.tor.clone()).await;
                tor.clone().bootstrap().await?;
            } else {
                info!("Initializing Tor client...");
                let client = TorClient::new(config.tor.clone())?;
                client.clone().bootstrap().await?;
                *self.tor_client.lock().await = Some(client);
            }
        } else if let Some(tor) = tor_current {
            tor.shutdown().await;
            *self.tor_client.lock().await = None;
        }

        let i2p_current = { self.i2p_client.lock().await.clone() };
        if config.mode == TransportMode::I2p && config.i2p.enabled {
            if let Some(i2p) = i2p_current {
                i2p.clone().update_config(config.i2p.clone()).await;
                i2p.clone().start().await?;
            } else {
                info!("Initializing I2P router...");
                let client = I2pClient::new(config.i2p.clone())?;
                client.clone().start().await?;
                *self.i2p_client.lock().await = Some(client);
            }
        } else if let Some(i2p) = i2p_current {
            i2p.shutdown().await;
            *self.i2p_client.lock().await = None;
        }

        // Update DNS resolver
        self.dns_resolver
            .lock()
            .await
            .set_config(config.dns_config.clone());

        // Update config
        *self.config.lock().await = config;

        Ok(())
    }

    /// Create HTTP client with configured transport
    pub async fn create_http_client(&self) -> Result<reqwest::Client> {
        let config = { self.config.lock().await.clone() };

        let mut client_builder =
            reqwest::Client::builder().timeout(std::time::Duration::from_secs(60));

        match config.mode {
            TransportMode::Tor => {
                let tor = { self.tor_client.lock().await.clone() };
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
                let i2p = { self.i2p_client.lock().await.clone() }
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

    /// Create gRPC channel with configured transport
    pub async fn create_grpc_channel(&self, endpoint: Endpoint) -> Result<Channel> {
        let config = { self.config.lock().await.clone() };
        let tor_client = { self.tor_client.lock().await.clone() };
        let i2p_client = { self.i2p_client.lock().await.clone() };
        let dns_config = config.dns_config.clone();
        let socks5_config = config.socks5.clone();
        let mode = config.mode;
        let endpoint_uri = endpoint.uri().to_string();

        debug!("Creating gRPC channel via {:?}", mode);
        log_debug_event(
            "transport.rs:TransportManager::create_grpc_channel",
            "grpc_channel_create",
            &format!(
                "mode={:?} endpoint={} dns_provider={} dns_tunnel={}",
                mode,
                endpoint_uri,
                dns_config.provider.name(),
                dns_config.tunnel_dns
            ),
        );

        let connector = service_fn(move |uri: Uri| {
            let tor_client = tor_client.clone();
            let i2p_client = i2p_client.clone();
            let dns_config = dns_config.clone();
            let socks5_config = socks5_config.clone();
            async move {
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
            }
        });

        endpoint
            .connect_with_connector(connector)
            .await
            .map_err(|e| Error::Network(format!("gRPC connection failed: {}", e)))
    }

    /// Open a raw stream using the configured transport mode.
    async fn open_stream(&self, host: &str, port: u16) -> Result<BoxedStream> {
        let config = { self.config.lock().await.clone() };
        let tor_client = { self.tor_client.lock().await.clone() };
        let i2p_client = { self.i2p_client.lock().await.clone() };

        match config.mode {
            TransportMode::Tor => {
                let tor = tor_client
                    .ok_or_else(|| Error::Network("Tor client not initialized".to_string()))?;
                connect_tor_stream(tor, host, port).await
            }
            TransportMode::I2p => {
                let i2p = i2p_client
                    .ok_or_else(|| Error::Network("I2P router not initialized".to_string()))?;
                connect_i2p_stream(i2p, host, port).await
            }
            TransportMode::Socks5 => {
                let socks5 = config
                    .socks5
                    .ok_or_else(|| Error::Network("SOCKS5 config not provided".to_string()))?;
                connect_socks5_stream(socks5, host, port).await
            }
            TransportMode::Direct => connect_direct_stream(config.dns_config, host, port).await,
        }
    }

    /// Fetch the SPKI pin from the server using the configured transport.
    pub async fn fetch_spki_pin(&self, host: &str, port: u16, server_name: &str) -> Result<String> {
        let stream = self.open_stream(host, port).await?;
        let connector = NativeTlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .map_err(|e| Error::Tls(format!("TLS connector build failed: {}", e)))?;
        let connector = TlsConnector::from(connector);
        let stream = connector
            .connect(server_name, stream)
            .await
            .map_err(|e| Error::Tls(format!("TLS handshake failed: {}", e)))?;
        let cert = stream
            .get_ref()
            .peer_certificate()
            .map_err(|e| Error::Tls(format!("TLS peer certificate error: {}", e)))?
            .ok_or_else(|| Error::Tls("No peer certificate presented".to_string()))?;
        let der = cert
            .to_der()
            .map_err(|e| Error::Tls(format!("Failed to read DER certificate: {}", e)))?;
        extract_spki_from_cert_der(&der)
    }

    /// Resolve hostname via configured DNS
    pub async fn resolve_host(&self, hostname: &str) -> Result<Vec<std::net::IpAddr>> {
        let resolver = { self.dns_resolver.lock().await.clone() };
        resolver.resolve(hostname).await
    }

    /// Get Tor bootstrap status
    pub async fn tor_status(&self) -> Option<crate::tor::TorStatus> {
        let tor = { self.tor_client.lock().await.clone() };
        if let Some(tor) = tor {
            Some(tor.status().await)
        } else {
            None
        }
    }

    /// Get I2P startup status
    pub async fn i2p_status(&self) -> Option<crate::i2p::I2pStatus> {
        let i2p = { self.i2p_client.lock().await.clone() };
        if let Some(i2p) = i2p {
            Some(i2p.status().await)
        } else {
            None
        }
    }

    /// Rotate Tor exit circuits by isolating future streams.
    pub async fn rotate_tor_exit(&self) -> Result<()> {
        let mode = { self.config.lock().await.mode };
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

        let tor = { self.tor_client.lock().await.clone() }
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

        if let Some(tor) = { self.tor_client.lock().await.clone() } {
            tor.shutdown().await;
        }
        *self.tor_client.lock().await = None;
        if let Some(i2p) = { self.i2p_client.lock().await.clone() } {
            i2p.shutdown().await;
        }
        *self.i2p_client.lock().await = None;
    }
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
    let addrs = resolver.resolve(&host).await?;
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

    let stream = match (socks5.username.as_ref(), socks5.password.as_ref()) {
        (Some(user), Some(pass)) => {
            Socks5Stream::connect_with_password(proxy_addr, (host.as_str(), port), user, pass)
                .await
                .map_err(|e| Error::Network(format!("SOCKS5 connect failed: {}", e)))?
        }
        _ => Socks5Stream::connect(proxy_addr, (host.as_str(), port))
            .await
            .map_err(|e| Error::Network(format!("SOCKS5 connect failed: {}", e)))?,
    };

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

async fn connect_socks5_stream(
    socks5: Socks5Config,
    host: &str,
    port: u16,
) -> Result<BoxedStream> {
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
    let status = tor.clone().status().await;
    log_debug_event(
        "transport.rs:connect_via_tor",
        "connect_tor_start",
        &format!("target={}:{} status={:?}", host, port, status),
    );
    match tor.connect_stream(&host, port).await {
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
    let stream = tor.connect_stream(host, port).await?;
    Ok(Box::new(stream))
}

async fn connect_via_i2p(i2p: I2pClient, uri: Uri) -> Result<ConnectorStream> {
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
    connect_via_socks5(proxy, uri).await
}

async fn connect_i2p_stream(i2p: I2pClient, host: &str, port: u16) -> Result<BoxedStream> {
    i2p.clone().start().await?;
    let proxy = i2p.clone().proxy_config().await;
    connect_socks5_stream(proxy, host, port).await
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

    #[tokio::test]
    async fn test_transport_manager_creation() {
        let config = TransportConfig {
            mode: TransportMode::Direct, // Avoid Tor bootstrap in test
            ..Default::default()
        };

        let manager = TransportManager::new(config).await.unwrap();
        assert_eq!(manager.mode().await, TransportMode::Direct);
    }
}
