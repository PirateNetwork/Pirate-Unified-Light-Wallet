//! I2P integration (desktop only)
//!
//! Provides an embedded I2P router process with ephemeral identities.

use crate::debug_log::log_debug_event;
use crate::{Error, Result, Socks5Config};
use directories::ProjectDirs;
use std::env;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

/// I2P bootstrap status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I2pStatus {
    /// Not started
    NotStarted,
    /// Starting router
    Starting,
    /// Ready for connections
    Ready,
    /// Error state with message
    Error(String),
}

/// I2P router configuration (desktop only)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2pConfig {
    /// Enable I2P
    pub enabled: bool,
    /// Optional path to embedded i2pd binary
    pub binary_path: Option<PathBuf>,
    /// Optional persistent data dir (ignored if ephemeral)
    pub data_dir: Option<PathBuf>,
    /// Router listens on this address
    pub address: String,
    /// SOCKS proxy port
    pub socks_port: u16,
    /// Use ephemeral identities (new data dir per launch)
    pub ephemeral: bool,
    /// Router startup timeout
    pub startup_timeout: Duration,
    /// Extra CLI arguments to pass to i2pd
    pub extra_args: Vec<String>,
}

impl Default for I2pConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            binary_path: None,
            data_dir: None,
            address: "127.0.0.1".to_string(),
            socks_port: 4447,
            ephemeral: true,
            startup_timeout: Duration::from_secs(180),
            extra_args: Vec::new(),
        }
    }
}

fn i2pd_binary_filenames() -> Vec<String> {
    let mut names = Vec::new();
    if cfg!(windows) {
        names.push("i2pd.exe".to_string());
    } else {
        let arch = env::consts::ARCH;
        names.push(format!("i2pd-{}", arch));
        names.push("i2pd".to_string());
    }
    names
}

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

fn bundled_i2p_candidates(file_name: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let exe = match env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return candidates,
    };
    let exe_dir = match exe.parent() {
        Some(dir) => dir,
        None => return candidates,
    };

    candidates.push(exe_dir.join("i2p").join(file_name));
    candidates.push(exe_dir.join("data").join("i2p").join(file_name));

    if let Some(parent) = exe_dir.parent() {
        candidates.push(parent.join("i2p").join(file_name));
        candidates.push(parent.join("Resources").join("i2p").join(file_name));
        candidates.push(parent.join("Frameworks").join("i2p").join(file_name));
    }

    candidates
}

fn find_i2pd_binary() -> Option<PathBuf> {
    for file_name in i2pd_binary_filenames() {
        for candidate in bundled_i2p_candidates(&file_name) {
            if candidate.is_file() {
                log_debug_event(
                    "i2p.rs:find_i2pd_binary",
                    "i2p_bin_bundled",
                    &format!("bin={} path={}", file_name, candidate.display()),
                );
                return Some(candidate);
            }
        }
    }

    for file_name in i2pd_binary_filenames() {
        if let Some(found) = find_on_path(&file_name) {
            log_debug_event(
                "i2p.rs:find_i2pd_binary",
                "i2p_bin_path",
                &format!("bin={} path={}", file_name, found.display()),
            );
            return Some(found);
        }
    }

    log_debug_event("i2p.rs:find_i2pd_binary", "i2p_bin_missing", "bin=i2pd");
    None
}

fn pick_available_port(address: &str, preferred: u16) -> u16 {
    if preferred == 0 {
        return TcpListener::bind((address, 0))
            .ok()
            .and_then(|listener| listener.local_addr().ok())
            .map(|addr| addr.port())
            .unwrap_or(preferred);
    }

    if TcpListener::bind((address, preferred)).is_ok() {
        return preferred;
    }

    TcpListener::bind((address, 0))
        .ok()
        .and_then(|listener| listener.local_addr().ok())
        .map(|addr| addr.port())
        .unwrap_or(preferred)
}

/// I2P router manager
pub struct I2pClient {
    config: Arc<Mutex<I2pConfig>>,
    status: Arc<Mutex<I2pStatus>>,
    child: Arc<Mutex<Option<Child>>>,
    ephemeral_dir: Arc<Mutex<Option<PathBuf>>>,
    start_lock: Arc<Mutex<()>>,
    shutdown_requested: Arc<AtomicBool>,
}

#[allow(dead_code)]
fn _assert_i2p_client_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<I2pClient>();
}

impl I2pClient {
    /// Create new I2P client
    pub fn new(config: I2pConfig) -> Result<Self> {
        Ok(Self {
            config: Arc::new(Mutex::new(config)),
            status: Arc::new(Mutex::new(I2pStatus::NotStarted)),
            child: Arc::new(Mutex::new(None)),
            ephemeral_dir: Arc::new(Mutex::new(None)),
            start_lock: Arc::new(Mutex::new(())),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Update I2P configuration (clears active router so it can be restarted)
    pub async fn update_config(self, config: I2pConfig) {
        self.clone().shutdown().await;
        self.shutdown_requested.store(false, Ordering::SeqCst);
        *self.config.lock().await = config;
        *self.child.lock().await = None;
        *self.status.lock().await = I2pStatus::NotStarted;
        *self.ephemeral_dir.lock().await = None;
        log_debug_event(
            "i2p.rs:I2pClient::update_config",
            "i2p_update_config",
            "status=not_started",
        );
    }

    /// Start the embedded router and wait for SOCKS readiness
    pub async fn start(self) -> Result<()> {
        if !cfg!(any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux"
        )) {
            return Err(Error::Network(
                "I2P is only supported on desktop".to_string(),
            ));
        }

        let _guard = self.start_lock.clone().lock_owned().await;
        self.shutdown_requested.store(false, Ordering::SeqCst);
        let status = { self.status.lock().await.clone() };
        match status {
            I2pStatus::Ready => return Ok(()),
            I2pStatus::Starting => {
                return self.clone().wait_for_ready().await;
            }
            I2pStatus::NotStarted | I2pStatus::Error(_) => {}
        }

        let mut config = self.config.lock().await.clone();
        if !config.enabled {
            return Err(Error::Network("I2P is disabled".to_string()));
        }
        if self.shutdown_requested.load(Ordering::SeqCst) {
            return Err(Error::Network("I2P shutdown requested".to_string()));
        }

        let selected_port = pick_available_port(&config.address, config.socks_port);
        if selected_port != config.socks_port {
            log_debug_event(
                "i2p.rs:I2pClient::start",
                "i2p_port_override",
                &format!("from={} to={}", config.socks_port, selected_port),
            );
            config.socks_port = selected_port;
            *self.config.lock().await = config.clone();
        }

        *self.status.lock().await = I2pStatus::Starting;
        log_debug_event(
            "i2p.rs:I2pClient::start",
            "i2p_start",
            &format!(
                "address={} port={} ephemeral={}",
                config.address, config.socks_port, config.ephemeral
            ),
        );

        let data_dir = prepare_data_dir(&config)?;
        *self.ephemeral_dir.lock().await = if config.ephemeral {
            Some(data_dir.clone())
        } else {
            None
        };

        let binary = if let Some(path) = config.binary_path.clone() {
            if path.is_file() {
                path
            } else {
                let message = format!("I2P binary not found at {}", path.display());
                log_debug_event(
                    "i2p.rs:I2pClient::start",
                    "i2p_start_error",
                    &format!("error={}", message),
                );
                return Err(Error::Network(message));
            }
        } else if let Some(found) = find_i2pd_binary() {
            found
        } else {
            return Err(Error::Network(
                "I2P binary not found (expected bundled i2pd or PIRATE_I2P_BINARY)".to_string(),
            ));
        };

        let conf_path = data_dir.join("i2pd.conf");
        write_config(&conf_path, &config)?;

        let mut cmd = Command::new(binary);
        cmd.arg("--datadir").arg(&data_dir);
        cmd.arg("--conf").arg(&conf_path);
        cmd.args(&config.extra_args);
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        let child = cmd.spawn().map_err(|e| {
            let message = format!("Failed to start i2pd: {}", e);
            *self.status.blocking_lock() = I2pStatus::Error(message.clone());
            log_debug_event(
                "i2p.rs:I2pClient::start",
                "i2p_start_error",
                &format!("error={}", message),
            );
            Error::Network(message)
        })?;

        *self.child.lock().await = Some(child);

        if self.shutdown_requested.load(Ordering::SeqCst) {
            if let Some(mut child) = self.child.lock().await.take() {
                let _ = child.kill();
            }
            *self.status.lock().await = I2pStatus::NotStarted;
            return Err(Error::Network("I2P shutdown requested".to_string()));
        }

        self.clone().wait_for_ready().await
    }

    /// Get current status
    pub async fn status(self) -> I2pStatus {
        self.status.lock().await.clone()
    }

    /// Check if I2P is ready
    pub async fn is_ready(self) -> bool {
        matches!(*self.status.lock().await, I2pStatus::Ready)
    }

    /// Get SOCKS proxy configuration
    pub async fn proxy_config(self) -> Socks5Config {
        let config = self.config.lock().await.clone();
        Socks5Config {
            host: config.address,
            port: config.socks_port,
            username: None,
            password: None,
        }
    }

    /// Stop the embedded router
    pub async fn shutdown(self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = child.kill();
        }
        *self.status.lock().await = I2pStatus::NotStarted;
        log_debug_event(
            "i2p.rs:I2pClient::shutdown",
            "i2p_shutdown",
            "status=not_started",
        );

        if let Some(dir) = self.ephemeral_dir.lock().await.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    async fn wait_for_ready(self) -> Result<()> {
        let config = self.config.lock().await.clone();
        let addr = format!("{}:{}", config.address, config.socks_port);
        let shutdown_requested = self.shutdown_requested.clone();

        let result = tokio::time::timeout(config.startup_timeout, async move {
            loop {
                if shutdown_requested.load(Ordering::SeqCst) {
                    return Err(Error::Network("I2P startup cancelled".to_string()));
                }
                if TcpStream::connect(&addr).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Ok(())
        })
        .await;

        match result {
            Ok(Ok(())) => {
                *self.status.lock().await = I2pStatus::Ready;
                log_debug_event(
                    "i2p.rs:I2pClient::wait_for_ready",
                    "i2p_ready",
                    "status=ready",
                );
                Ok(())
            }
            Ok(Err(e)) => {
                *self.status.lock().await = I2pStatus::NotStarted;
                Err(e)
            }
            Err(_) => {
                let message = "I2P startup timed out".to_string();
                *self.status.lock().await = I2pStatus::Error(message.clone());
                log_debug_event(
                    "i2p.rs:I2pClient::wait_for_ready",
                    "i2p_error",
                    &format!("error={}", message),
                );
                Err(Error::Network(message))
            }
        }
    }
}

impl Clone for I2pClient {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            status: Arc::clone(&self.status),
            child: Arc::clone(&self.child),
            ephemeral_dir: Arc::clone(&self.ephemeral_dir),
            start_lock: Arc::clone(&self.start_lock),
            shutdown_requested: Arc::clone(&self.shutdown_requested),
        }
    }
}

fn prepare_data_dir(config: &I2pConfig) -> Result<PathBuf> {
    if config.ephemeral {
        let mut dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        dir.push(format!("pirate-i2p-{}-{}", std::process::id(), nonce));
        std::fs::create_dir_all(&dir)
            .map_err(|e| Error::Network(format!("Failed to create I2P temp dir: {}", e)))?;
        return Ok(dir);
    }

    if let Some(ref dir) = config.data_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::Network(format!("Failed to create I2P data dir: {}", e)))?;
        return Ok(dir.clone());
    }

    let base = ProjectDirs::from("com", "Pirate", "PirateWallet")
        .map(|dirs| dirs.data_local_dir().join("i2p"))
        .unwrap_or_else(|| PathBuf::from("i2p_data"));
    std::fs::create_dir_all(&base)
        .map_err(|e| Error::Network(format!("Failed to create I2P data dir: {}", e)))?;
    Ok(base)
}

fn write_config(path: &PathBuf, config: &I2pConfig) -> Result<()> {
    let contents = format!(
        "[http]\n\
enabled = false\n\
\n\
[socksproxy]\n\
enabled = true\n\
address = {}\n\
port = {}\n\
\n\
[sam]\n\
enabled = false\n",
        config.address, config.socks_port
    );

    std::fs::write(path, contents)
        .map_err(|e| Error::Network(format!("Failed to write i2pd config: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i2p_config_default() {
        let config = I2pConfig::default();
        assert!(!config.enabled);
        assert!(config.ephemeral);
    }
}
