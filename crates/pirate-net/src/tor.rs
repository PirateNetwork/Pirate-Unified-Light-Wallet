//! Tor integration via Arti
//!
//! Provides embedded Tor client for privacy-preserving network access.

use crate::debug_log::log_debug_event;
use crate::{Error, Result};
use arti_client::config::TorClientConfigBuilder;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use arti_client::config::pt::TransportConfigBuilder;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use arti_client::config::{BridgeConfigBuilder, CfgPath};
use arti_client::{BootstrapBehavior, StreamPrefs, TorClient as ArtiClient};
use directories::{BaseDirs, ProjectDirs};
use futures_util::StreamExt;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::http1;
use hyper::header;
use hyper::Request;
use hyper_util::rt::TokioIo;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use tor_rtcompat::{BlockOn, PreferredRuntime};
use tracing::{info, warn};

/// Tor bootstrap status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorStatus {
    /// Not started
    NotStarted,
    /// Bootstrapping with progress (0-100) and optional blockage message
    Bootstrapping {
        /// Percent ready (best effort)
        progress: u8,
        /// Optional blockage hint from Arti
        blocked: Option<String>,
    },
    /// Ready for connections
    Ready,
    /// Error state with message
    Error(String),
}

/// Pluggable transport selection for bridges
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TorBridgeTransport {
    /// obfs4 transport
    Obfs4,
    /// snowflake transport
    Snowflake,
    /// Custom transport name
    Custom(String),
}

/// Bridge configuration for censorship circumvention
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorBridgeConfig {
    /// Selected transport
    pub transport: TorBridgeTransport,
    /// Bridge lines (one per entry)
    pub bridge_lines: Vec<String>,
    /// Path to PT client binary (optional; falls back to name on PATH)
    pub transport_path: Option<PathBuf>,
}

/// Tor client configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorConfig {
    /// Enable Tor
    pub enabled: bool,
    /// Tor state directory
    pub state_dir: PathBuf,
    /// Tor cache directory
    pub cache_dir: PathBuf,
    /// Enable debug logging
    pub debug: bool,
    /// Bootstrap timeout
    pub bootstrap_timeout: Duration,
    /// Stream connection timeout
    pub connect_timeout: Duration,
    /// Use bridges immediately when bootstrapping
    pub use_bridges: bool,
    /// Retry with bridges if direct bootstrap fails
    pub fallback_to_bridges: bool,
    /// Bridge configuration (required for bridge usage)
    pub bridges: Option<TorBridgeConfig>,
}

impl Default for TorConfig {
    fn default() -> Self {
        let (state_dir, cache_dir) = default_tor_dirs();
        Self {
            enabled: true,
            state_dir,
            cache_dir,
            debug: false,
            bootstrap_timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(30),
            use_bridges: false,
            fallback_to_bridges: true,
            bridges: None,
        }
    }
}

fn tor_base_candidates() -> Vec<PathBuf> {
    let mut bases = Vec::new();

    if let Some(dirs) = ProjectDirs::from("com", "Pirate", "PirateWallet") {
        bases.push(dirs.data_local_dir().join("tor"));
    }
    if let Some(base) = BaseDirs::new() {
        let candidate = base.data_local_dir().join("PirateWallet").join("tor");
        if !bases.iter().any(|path| path == &candidate) {
            bases.push(candidate);
        }
    }
    if let Ok(home) = env::var("HOME") {
        if !home.trim().is_empty() {
            let candidate = PathBuf::from(home).join(".pirate_wallet").join("tor");
            if !bases.iter().any(|path| path == &candidate) {
                bases.push(candidate);
            }
        }
    }

    bases.push(env::temp_dir().join("pirate_wallet").join("tor"));
    bases
}

fn default_tor_dirs() -> (PathBuf, PathBuf) {
    let base = tor_base_candidates()
        .into_iter()
        .next()
        .unwrap_or_else(|| env::temp_dir().join("pirate_wallet").join("tor"));
    (base.join("state"), base.join("cache"))
}

fn ensure_tor_dirs(state_dir: &Path, cache_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    if std::fs::create_dir_all(state_dir).is_ok()
        && std::fs::create_dir_all(cache_dir).is_ok()
    {
        return Ok((state_dir.to_path_buf(), cache_dir.to_path_buf()));
    }

    let mut last_err: Option<std::io::Error> = None;
    for base in tor_base_candidates() {
        let state = base.join("state");
        let cache = base.join("cache");
        match (std::fs::create_dir_all(&state), std::fs::create_dir_all(&cache)) {
            (Ok(_), Ok(_)) => {
                log_debug_event(
                    "tor.rs:ensure_tor_dirs",
                    "tor_dir_fallback",
                    &format!(
                        "state_dir={} cache_dir={}",
                        state.display(),
                        cache.display()
                    ),
                );
                return Ok((state, cache));
            }
            (Err(err), _) | (_, Err(err)) => last_err = Some(err),
        }
    }

    Err(Error::Tor(format!(
        "Failed to create Tor state dir: {}",
        last_err
            .map(|err| err.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    )))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn transport_binary_filenames(name: &str) -> Vec<String> {
    let mut names = Vec::new();
    if cfg!(windows) {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".exe") {
            names.push(name.to_string());
        } else {
            names.push(format!("{}.exe", name));
        }
    } else {
        let arch = env::consts::ARCH;
        names.push(format!("{}-{}", name, arch));
        names.push(name.to_string());
    }
    names
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn find_on_path(file_name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for path in env::split_paths(&path_var) {
        let candidate = path.join(file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn tor_browser_candidates(file_name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    let browser_path_vars = ["PIRATE_TOR_BROWSER_PATH", "TOR_BROWSER_PATH"];
    for var in browser_path_vars {
        if let Ok(value) = env::var(var) {
            let base = PathBuf::from(value);
            candidates.push(
                base.join("Browser")
                    .join("TorBrowser")
                    .join("Tor")
                    .join("PluggableTransports")
                    .join(file_name),
            );
            candidates.push(
                base.join("TorBrowser")
                    .join("Tor")
                    .join("PluggableTransports")
                    .join(file_name),
            );
            candidates.push(base.join("Tor").join("PluggableTransports").join(file_name));
        }
    }

    #[cfg(windows)]
    {
        if let Ok(local) = env::var("LOCALAPPDATA") {
            let base = PathBuf::from(local)
                .join("Tor Browser")
                .join("Browser")
                .join("TorBrowser")
                .join("Tor")
                .join("PluggableTransports");
            candidates.push(base.join(file_name));
        }
        if let Ok(program) = env::var("PROGRAMFILES") {
            let base = PathBuf::from(program)
                .join("Tor Browser")
                .join("Browser")
                .join("TorBrowser")
                .join("Tor")
                .join("PluggableTransports");
            candidates.push(base.join(file_name));
        }
        if let Ok(program_x86) = env::var("PROGRAMFILES(X86)") {
            let base = PathBuf::from(program_x86)
                .join("Tor Browser")
                .join("Browser")
                .join("TorBrowser")
                .join("Tor")
                .join("PluggableTransports");
            candidates.push(base.join(file_name));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let app_path = PathBuf::from("/Applications")
            .join("Tor Browser.app")
            .join("Contents")
            .join("MacOS")
            .join("Tor")
            .join("PluggableTransports")
            .join(file_name);
        candidates.push(app_path);

        if let Ok(home) = env::var("HOME") {
            let app_path = PathBuf::from(home)
                .join("Applications")
                .join("Tor Browser.app")
                .join("Contents")
                .join("MacOS")
                .join("Tor")
                .join("PluggableTransports")
                .join(file_name);
            candidates.push(app_path);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = env::var("HOME") {
            let base = PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("torbrowser")
                .join("tbb")
                .join("x86_64")
                .join("tor-browser")
                .join("Browser")
                .join("TorBrowser")
                .join("Tor")
                .join("PluggableTransports")
                .join(file_name);
            candidates.push(base);
        }
        candidates.push(
            PathBuf::from("/usr/local/share/torbrowser")
                .join("Browser")
                .join("TorBrowser")
                .join("Tor")
                .join("PluggableTransports")
                .join(file_name),
        );
        candidates.push(
            PathBuf::from("/usr/share/torbrowser")
                .join("Browser")
                .join("TorBrowser")
                .join("Tor")
                .join("PluggableTransports")
                .join(file_name),
        );
    }

    candidates
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn bundled_transport_candidates(file_name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let exe = match env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return candidates,
    };
    let exe_dir = match exe.parent() {
        Some(dir) => dir,
        None => return candidates,
    };

    candidates.push(exe_dir.join("tor-pt").join(file_name));
    candidates.push(exe_dir.join("data").join("tor-pt").join(file_name));

    if let Some(parent) = exe_dir.parent() {
        candidates.push(parent.join("tor-pt").join(file_name));
        candidates.push(parent.join("Resources").join("tor-pt").join(file_name));
        candidates.push(parent.join("Frameworks").join("tor-pt").join(file_name));
    }

    candidates
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn find_transport_binary(name: &str) -> Option<PathBuf> {
    let explicit_vars = ["PIRATE_TOR_PT_PATH", "TOR_PT_PATH"];
    for var in explicit_vars {
        if let Ok(value) = env::var(var) {
            let candidate = PathBuf::from(value);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let dir_vars = ["PIRATE_TOR_PT_DIR", "TOR_PT_DIR"];
    for var in dir_vars {
        if let Ok(value) = env::var(var) {
            for file_name in transport_binary_filenames(name) {
                let candidate = PathBuf::from(&value).join(&file_name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    for file_name in transport_binary_filenames(name) {
        for candidate in bundled_transport_candidates(&file_name) {
            if candidate.is_file() {
                log_debug_event(
                    "tor.rs:find_transport_binary",
                    "tor_pt_bundled",
                    &format!("bin={} path={}", file_name, candidate.display()),
                );
                return Some(candidate);
            }
        }
    }

    for file_name in transport_binary_filenames(name) {
        for candidate in tor_browser_candidates(&file_name) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    for file_name in transport_binary_filenames(name) {
        if let Some(found) = find_on_path(&file_name) {
            return Some(found);
        }
    }

    None
}

/// Tor client wrapper using Arti
pub struct TorClient {
    client: Arc<Mutex<Option<ArtiClient<PreferredRuntime>>>>,
    config: Arc<Mutex<TorConfig>>,
    status: Arc<Mutex<TorStatus>>,
    bootstrap_lock: Arc<Mutex<()>>,
    stream_prefs: Arc<Mutex<StreamPrefs>>,
    bootstrap_generation: Arc<AtomicU64>,
}

#[allow(dead_code)]
fn _assert_tor_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TorClient>();
}

impl TorClient {
    /// Create new Tor client
    pub fn new(config: TorConfig) -> Result<Self> {
        info!("Creating Tor client with config: {:?}", config);
        Ok(Self {
            client: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(config)),
            status: Arc::new(Mutex::new(TorStatus::NotStarted)),
            bootstrap_lock: Arc::new(Mutex::new(())),
            stream_prefs: Arc::new(Mutex::new(StreamPrefs::new())),
            bootstrap_generation: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Update Tor configuration (clears active client so it can be re-bootstrapped)
    pub async fn update_config(self, config: TorConfig) {
        *self.config.lock().await = config;
        *self.client.lock().await = None;
        *self.status.lock().await = TorStatus::NotStarted;
        self.bootstrap_generation.fetch_add(1, Ordering::SeqCst);
        log_debug_event(
            "tor.rs:TorClient::update_config",
            "tor_update_config",
            "status=not_started",
        );
    }

    /// Bootstrap Tor connection (blocking until ready or error)
    pub async fn bootstrap(self) -> Result<()> {
        let _guard = self.bootstrap_lock.clone().lock_owned().await;
        let timeout = self.config.lock().await.bootstrap_timeout;

        let status = { self.status.lock().await.clone() };
        match status {
            TorStatus::Ready => return Ok(()),
            TorStatus::Bootstrapping { .. } => return self.clone().wait_for_ready(timeout).await,
            TorStatus::Error(_) | TorStatus::NotStarted => {}
        }

        let config = self.config.lock().await.clone();
        if !config.enabled {
            return Err(Error::Tor("Tor is disabled".to_string()));
        }

        let mut attempts = Vec::new();
        if config.use_bridges && config.bridges.is_some() {
            attempts.push(true);
        } else {
            attempts.push(false);
            if config.fallback_to_bridges && config.bridges.is_some() {
                attempts.push(true);
            }
        }

        for use_bridges in attempts {
            let generation = self.next_bootstrap_generation();
            log_debug_event(
                "tor.rs:TorClient::bootstrap",
                "tor_bootstrap_attempt",
                &format!("use_bridges={} gen={}", use_bridges, generation),
            );
            match self
                .clone()
                .bootstrap_with_config(config.clone(), use_bridges, generation)
                .await
            {
                Ok(client) => {
                    *self.client.lock().await = Some(client);
                    return Ok(());
                }
                Err(e) => {
                    warn!(
                        "Tor bootstrap attempt failed (bridges={}): {}",
                        use_bridges, e
                    );
                }
            }
        }

        Err(Error::Tor("Tor bootstrap failed".to_string()))
    }

    /// Get bootstrap status
    pub async fn status(&self) -> TorStatus {
        self.status.lock().await.clone()
    }

    /// Check if Tor is ready
    pub async fn is_ready(&self) -> bool {
        matches!(*self.status.lock().await, TorStatus::Ready)
    }

    /// Connect to a target host/port using Tor
    pub async fn connect_stream(&self, host: &str, port: u16) -> Result<arti_client::DataStream> {
        let timeout = self.config.lock().await.bootstrap_timeout;
        let connect_timeout = self.config.lock().await.connect_timeout;
        self.clone().bootstrap().await?;
        self.clone().wait_for_ready(timeout).await?;

        let client = self
            .client
            .lock()
            .await
            .clone()
            .ok_or_else(|| Error::Tor("Tor client not initialized".to_string()))?;
        let prefs = { self.stream_prefs.lock().await.clone() };
        let target = format!("{}:{}", host, port);
        let stream =
            tokio::time::timeout(connect_timeout, client.connect_with_prefs(target, &prefs))
                .await
                .map_err(|_| {
                    Error::Tor(format!(
                        "Tor connect timed out to {}:{} (port may be blocked by Tor exits)",
                        host, port
                    ))
                })?
                .map_err(|e| Error::Tor(format!("Tor connect failed: {}", e)))?;
        Ok(stream)
    }

    /// Rotate exit circuits by isolating future streams.
    pub async fn rotate_exit(&self) {
        let mut prefs = StreamPrefs::new();
        prefs.new_isolation_group();
        *self.stream_prefs.lock().await = prefs;
        log_debug_event(
            "tor.rs:TorClient::rotate_exit",
            "tor_exit_rotate",
            "isolation=new",
        );
    }

    /// Fetch current Tor exit IP address using an HTTP check over Tor.
    pub async fn fetch_exit_ip(self) -> Result<String> {
        let host = "checkip.amazonaws.com";
        let timeout = Duration::from_secs(20);
        let stream = self.clone().connect_stream(host, 80).await?;
        let io = TokioIo::new(stream);
        let (mut sender, conn) = tokio::time::timeout(timeout, http1::handshake(io))
            .await
            .map_err(|_| Error::Tor("Tor exit IP handshake timed out".to_string()))?
            .map_err(|e| Error::Tor(format!("Tor exit IP handshake failed: {}", e)))?;

        tokio::spawn(async move {
            if let Err(err) = conn.await {
                warn!("Tor exit IP connection error: {}", err);
            }
        });

        let request = Request::builder()
            .method("GET")
            .uri(format!("http://{}/", host))
            .header(header::HOST, host)
            .header(header::USER_AGENT, "PirateWallet-TorExitCheck/1.0")
            .body(Empty::<Bytes>::new())
            .map_err(|e| Error::Tor(format!("Tor exit IP request build failed: {}", e)))?;

        let response = tokio::time::timeout(timeout, sender.send_request(request))
            .await
            .map_err(|_| Error::Tor("Tor exit IP request timed out".to_string()))?
            .map_err(|e| Error::Tor(format!("Tor exit IP request failed: {}", e)))?;

        let status = response.status();
        let body = tokio::time::timeout(timeout, response.into_body().collect())
            .await
            .map_err(|_| Error::Tor("Tor exit IP response timed out".to_string()))?
            .map_err(|e| Error::Tor(format!("Tor exit IP response failed: {}", e)))?
            .to_bytes();
        let body = String::from_utf8_lossy(&body).trim().to_string();

        if !status.is_success() {
            return Err(Error::Tor(format!(
                "Tor exit IP request failed: status={} body={}",
                status, body
            )));
        }
        if body.is_empty() {
            return Err(Error::Tor("Tor exit IP response empty".to_string()));
        }

        Ok(body)
    }

    /// Shutdown Tor client
    pub async fn shutdown(self) {
        info!("Shutting down Tor client...");
        *self.client.lock().await = None;
        *self.status.lock().await = TorStatus::NotStarted;
        log_debug_event(
            "tor.rs:TorClient::shutdown",
            "tor_shutdown",
            "status=not_started",
        );
    }

    async fn wait_for_ready(self, timeout: Duration) -> Result<()> {
        let wait_result = tokio::time::timeout(timeout, async {
            loop {
                let status = { self.status.lock().await.clone() };
                match status {
                    TorStatus::Ready => return Ok(()),
                    TorStatus::Error(message) => return Err(Error::Tor(message)),
                    TorStatus::Bootstrapping { .. } | TorStatus::NotStarted => {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
            }
        })
        .await;

        match wait_result {
            Ok(result) => result,
            Err(_) => {
                let message = "Tor bootstrap timed out".to_string();
                *self.status.lock().await = TorStatus::Error(message.clone());
                log_debug_event(
                    "tor.rs:TorClient::wait_for_ready",
                    "tor_bootstrap_error",
                    &format!("error={}", message),
                );
                Err(Error::Tor(message))
            }
        }
    }

    async fn bootstrap_with_config(
        self,
        config: TorConfig,
        use_bridges: bool,
        generation: u64,
    ) -> Result<ArtiClient<PreferredRuntime>> {
        if self.bootstrap_generation.load(Ordering::SeqCst) != generation {
            return Err(Error::Tor("Tor bootstrap superseded".to_string()));
        }
        *self.status.lock().await = TorStatus::Bootstrapping {
            progress: 0,
            blocked: None,
        };
        log_debug_event(
            "tor.rs:TorClient::bootstrap_with_config",
            "tor_bootstrap_start",
            &format!(
                "use_bridges={} timeout_secs={} gen={}",
                use_bridges,
                config.bootstrap_timeout.as_secs(),
                generation
            ),
        );

        let status = Arc::clone(&self.status);
        let active_generation = Arc::clone(&self.bootstrap_generation);
        let (tx, rx) = oneshot::channel();

        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (|| -> Result<ArtiClient<PreferredRuntime>> {
                    let runtime = PreferredRuntime::create()
                        .map_err(|e| Error::Tor(format!("Tor runtime init failed: {}", e)))?;
                    let runtime_for_client = runtime.clone();

                    runtime.block_on(async move {
                        let arti_config = build_arti_config(&config, use_bridges)?;

                        let client = ArtiClient::with_runtime(runtime_for_client)
                            .config(arti_config)
                            .bootstrap_behavior(BootstrapBehavior::Manual)
                            .create_unbootstrapped()
                            .map_err(|e| {
                                Error::Tor(format!("Failed to create Tor client: {}", e))
                            })?;

                        spawn_status_watcher(
                            status.clone(),
                            &client,
                            generation,
                            active_generation.clone(),
                        );

                        match tokio::time::timeout(
                            config.bootstrap_timeout,
                            client.clone().bootstrap(),
                        )
                        .await
                        {
                            Ok(Ok(())) => Ok(client),
                            Ok(Err(e)) => Err(Error::Tor(format!("Tor bootstrap failed: {}", e))),
                            Err(_) => Err(Error::Tor("Tor bootstrap timed out".to_string())),
                        }
                    })
                })()
            }));

            let result = match result {
                Ok(inner) => inner,
                Err(panic) => {
                    let message = if let Some(value) = panic.downcast_ref::<&str>() {
                        value.to_string()
                    } else if let Some(value) = panic.downcast_ref::<String>() {
                        value.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    Err(Error::Tor(format!("Tor bootstrap panic: {}", message)))
                }
            };

            let _ = tx.send(result);
        });

        match rx.await {
            Ok(Ok(client)) => {
                if self.bootstrap_generation.load(Ordering::SeqCst) == generation {
                    *self.status.lock().await = TorStatus::Ready;
                    log_debug_event(
                        "tor.rs:TorClient::bootstrap_with_config",
                        "tor_bootstrap_ready",
                        &format!("status=ready gen={}", generation),
                    );
                    Ok(client)
                } else {
                    Err(Error::Tor("Tor bootstrap superseded".to_string()))
                }
            }
            Ok(Err(e)) => {
                let message = e.to_string();
                if self.bootstrap_generation.load(Ordering::SeqCst) == generation {
                    *self.status.lock().await = TorStatus::Error(message.clone());
                    log_debug_event(
                        "tor.rs:TorClient::bootstrap_with_config",
                        "tor_bootstrap_error",
                        &format!("error={} gen={}", message, generation),
                    );
                    Err(e)
                } else {
                    Err(Error::Tor("Tor bootstrap superseded".to_string()))
                }
            }
            Err(e) => {
                let message = format!("Tor bootstrap thread error: {}", e);
                if self.bootstrap_generation.load(Ordering::SeqCst) == generation {
                    *self.status.lock().await = TorStatus::Error(message.clone());
                    log_debug_event(
                        "tor.rs:TorClient::bootstrap_with_config",
                        "tor_bootstrap_error",
                        &format!("error={} gen={}", message, generation),
                    );
                    Err(Error::Tor(message))
                } else {
                    Err(Error::Tor("Tor bootstrap superseded".to_string()))
                }
            }
        }
    }
}

fn spawn_status_watcher(
    status: Arc<Mutex<TorStatus>>,
    client: &ArtiClient<PreferredRuntime>,
    generation: u64,
    active_generation: Arc<AtomicU64>,
) {
    let mut events = client.bootstrap_events();
    tokio::spawn(async move {
        let mut last_status: Option<TorStatus> = None;
        while let Some(event) = events.next().await {
            if active_generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let percent = (event.as_frac() * 100.0).round() as u8;
            let blocked = event.blocked().map(|b| b.to_string());
            let new_status = if event.ready_for_traffic() {
                TorStatus::Ready
            } else {
                TorStatus::Bootstrapping {
                    progress: percent,
                    blocked,
                }
            };
            if last_status.as_ref() != Some(&new_status) {
                log_debug_event(
                    "tor.rs:spawn_status_watcher",
                    "tor_status_update",
                    &format!("status={:?} gen={}", new_status, generation),
                );
                last_status = Some(new_status.clone());
            }
            *status.lock().await = new_status;
        }
        if active_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let mut status_guard = status.lock().await;
        if !matches!(*status_guard, TorStatus::Ready) {
            let message = "Tor bootstrap events ended".to_string();
            log_debug_event(
                "tor.rs:spawn_status_watcher",
                "tor_status_error",
                &format!("error={} gen={}", message, generation),
            );
            *status_guard = TorStatus::Error(message);
        }
    });
}

impl Clone for TorClient {
    fn clone(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
            config: Arc::clone(&self.config),
            status: Arc::clone(&self.status),
            bootstrap_lock: Arc::clone(&self.bootstrap_lock),
            stream_prefs: Arc::clone(&self.stream_prefs),
            bootstrap_generation: Arc::clone(&self.bootstrap_generation),
        }
    }
}

impl TorClient {
    fn next_bootstrap_generation(&self) -> u64 {
        self.bootstrap_generation.fetch_add(1, Ordering::SeqCst) + 1
    }
}

fn build_arti_config(
    config: &TorConfig,
    use_bridges: bool,
) -> Result<arti_client::TorClientConfig> {
    let (state_dir, cache_dir) = ensure_tor_dirs(&config.state_dir, &config.cache_dir)?;

    let mut builder = TorClientConfigBuilder::from_directories(&state_dir, &cache_dir);

    if use_bridges {
        let bridges = config
            .bridges
            .as_ref()
            .ok_or_else(|| Error::Tor("Bridge config required but missing".to_string()))?;
        apply_bridge_config(&mut builder, bridges)?;
    }

    builder
        .build()
        .map_err(|e| Error::Tor(format!("Tor config build failed: {}", e)))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn apply_bridge_config(
    _builder: &mut TorClientConfigBuilder,
    bridges: &TorBridgeConfig,
) -> Result<()> {
    log_debug_event(
        "tor.rs:apply_bridge_config",
        "tor_pt_unsupported",
        &format!("transport={:?}", bridges.transport),
    );
    Err(Error::Tor(
        "Bridge transports are not supported on mobile builds".to_string(),
    ))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn apply_bridge_config(
    builder: &mut TorClientConfigBuilder,
    bridges: &TorBridgeConfig,
) -> Result<()> {
    if bridges.bridge_lines.is_empty() {
        return Err(Error::Tor(
            "Bridge mode selected but no bridges provided".to_string(),
        ));
    }

    for line in &bridges.bridge_lines {
        let bridge: BridgeConfigBuilder = line
            .parse()
            .map_err(|e| Error::Tor(format!("Invalid bridge line: {}", e)))?;
        builder.bridges().bridges().push(bridge);
    }

    let (protocol, default_bin) = match &bridges.transport {
        TorBridgeTransport::Obfs4 => ("obfs4", "obfs4proxy"),
        TorBridgeTransport::Snowflake => ("snowflake", "snowflake-client"),
        TorBridgeTransport::Custom(name) => (name.as_str(), name.as_str()),
    };

    let transport_path = if let Some(path) = bridges.transport_path.as_ref() {
        if path.as_os_str().is_empty() {
            None
        } else if path.is_file() {
            Some(CfgPath::new_literal(path))
        } else {
            return Err(Error::Tor(format!(
                "Pluggable transport binary not found at {}",
                path.display()
            )));
        }
    } else if let Some(found) = find_transport_binary(default_bin) {
        log_debug_event(
            "tor.rs:apply_bridge_config",
            "tor_pt_autodetect",
            &format!("transport={} path={}", protocol, found.display()),
        );
        Some(CfgPath::new_literal(found))
    } else {
        log_debug_event(
            "tor.rs:apply_bridge_config",
            "tor_pt_missing",
            &format!("transport={} bin={}", protocol, default_bin),
        );
        None
    }
    .unwrap_or_else(|| CfgPath::new(default_bin.to_string()));

    let mut transport = TransportConfigBuilder::default();
    transport
        .protocols(vec![protocol.parse().map_err(|e| {
            Error::Tor(format!("Invalid transport protocol '{}': {}", protocol, e))
        })?])
        .path(transport_path)
        .run_on_startup(true);
    builder.bridges().transports().push(transport);

    Ok(())
}

impl Default for TorClient {
    fn default() -> Self {
        Self::new(TorConfig::default()).expect("Failed to create default Tor client")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tor_config_default() {
        let config = TorConfig::default();
        assert!(config.enabled);
        assert!(!config.debug);
    }

    #[tokio::test]
    async fn test_tor_status() {
        let client = TorClient::new(TorConfig::default()).unwrap();
        assert_eq!(client.status().await, TorStatus::NotStarted);
        assert!(!client.is_ready().await);
    }
}
