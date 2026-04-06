use super::*;
use pirate_sync_lightd::client::{LightClient, RetryConfig, TransportMode};
use std::time::Duration;

fn parse_tunnel_mode_setting(mode: &str, socks5_url: Option<String>) -> Option<TunnelMode> {
    let normalized = mode.trim().to_lowercase();
    match normalized.as_str() {
        "tor" => Some(TunnelMode::Tor),
        "i2p" => Some(TunnelMode::I2p),
        "socks5" => {
            let url = socks5_url
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "socks5h://localhost:1080".to_string());
            Some(TunnelMode::Socks5 { url })
        }
        "direct" => Some(TunnelMode::Direct),
        _ => None,
    }
}

pub(super) fn load_registry_tunnel_mode(db: &Database) -> Result<Option<TunnelMode>> {
    let mode = get_registry_setting(db, REGISTRY_TUNNEL_MODE_KEY)?;
    let Some(mode_str) = mode else {
        return Ok(None);
    };
    let socks5_url = get_registry_setting(db, REGISTRY_TUNNEL_SOCKS5_URL_KEY)?;
    let parsed = parse_tunnel_mode_setting(&mode_str, socks5_url);
    if parsed.is_none() {
        tracing::warn!("Unknown tunnel mode setting: {}", mode_str);
    }
    Ok(parsed)
}

pub(super) fn persist_registry_tunnel_mode(db: &Database, mode: &TunnelMode) -> Result<()> {
    let (mode_str, socks5_url) = match mode {
        TunnelMode::Tor => ("tor", None),
        TunnelMode::I2p => ("i2p", None),
        TunnelMode::Socks5 { url } => ("socks5", Some(url.as_str())),
        TunnelMode::Direct => ("direct", None),
    };
    set_registry_setting(db, REGISTRY_TUNNEL_MODE_KEY, Some(mode_str))?;
    set_registry_setting(db, REGISTRY_TUNNEL_SOCKS5_URL_KEY, socks5_url)?;
    Ok(())
}

fn redact_socks5_url(url: &str) -> String {
    if let Some(scheme_pos) = url.find("://") {
        let auth_start = scheme_pos + 3;
        if let Some(at_pos) = url[auth_start..].find('@') {
            let at_pos = auth_start + at_pos;
            let mut redacted = String::new();
            redacted.push_str(&url[..auth_start]);
            redacted.push_str("***@");
            redacted.push_str(&url[at_pos + 1..]);
            return redacted;
        }
    }
    url.to_string()
}

fn tunnel_transport_config_for(mode: &TunnelMode) -> (TransportMode, Option<String>, bool) {
    match mode {
        TunnelMode::Tor => (TransportMode::Tor, None, false),
        TunnelMode::I2p => (TransportMode::I2p, None, false),
        TunnelMode::Socks5 { url } => (TransportMode::Socks5, Some(url.clone()), false),
        TunnelMode::Direct => (TransportMode::Direct, None, true),
    }
}

pub(super) fn tunnel_transport_config() -> (TransportMode, Option<String>, bool) {
    let tunnel_mode = TUNNEL_MODE.read().clone();
    tunnel_transport_config_for(&tunnel_mode)
}

pub(super) fn light_client_config_for_endpoint(
    endpoint: &endpoint::LightdEndpoint,
    retry: RetryConfig,
    connect_timeout: Duration,
    request_timeout: Duration,
) -> pirate_sync_lightd::client::LightClientConfig {
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    endpoint::build_light_client_config(
        endpoint,
        transport,
        socks5_url,
        allow_direct_fallback,
        retry,
        connect_timeout,
        request_timeout,
    )
}

fn spawn_bootstrap_transport(mode: TunnelMode) {
    let (transport, socks5_url, _) = tunnel_transport_config_for(&mode);
    let task = async move {
        if let Err(e) = pirate_sync_lightd::bootstrap_transport(transport, socks5_url).await {
            tracing::warn!("Failed to bootstrap transport: {}", e);
        }
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(task);
    } else {
        std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Runtime::new() {
                runtime.block_on(task);
            }
        });
    }
}

pub(super) async fn disconnect_active_sync_channels(reason: &'static str) {
    sync_control::disconnect_foreground_sync_channels(reason).await;
}

fn spawn_disconnect_active_sync_channels(reason: &'static str) {
    let task = async move {
        disconnect_active_sync_channels(reason).await;
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(task);
    } else {
        std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Runtime::new() {
                runtime.block_on(task);
            }
        });
    }
}

pub fn set_tunnel(mode: TunnelMode) -> Result<()> {
    tracing::info!("Setting tunnel mode: {:?}", mode);
    *TUNNEL_MODE.write() = mode.clone();
    // #region agent log
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let (mode_label, socks5_label) = match &mode {
            TunnelMode::Tor => ("tor", "none".to_string()),
            TunnelMode::I2p => ("i2p", "none".to_string()),
            TunnelMode::Direct => ("direct", "none".to_string()),
            TunnelMode::Socks5 { url } => ("socks5", redact_socks5_url(url)),
        };
        let _ = writeln!(
            file,
            r#"{{"id":"log_tunnel_set","timestamp":{},"location":"tunnel.rs:{}", "message":"set_tunnel","data":{{"mode":"{}","socks5":"{}"}}}}"#,
            ts,
            line!(),
            mode_label,
            socks5_label
        );
    });
    // #endregion
    if let Ok(registry_db) = open_wallet_registry() {
        if let Err(e) = persist_registry_tunnel_mode(&registry_db, &mode) {
            tracing::warn!("Failed to persist tunnel mode: {}", e);
            *PENDING_TUNNEL_MODE.write() = Some(mode.clone());
        }
    } else {
        *PENDING_TUNNEL_MODE.write() = Some(mode.clone());
    }
    spawn_bootstrap_transport(mode);
    // Force active sync channels to reconnect on the new transport immediately
    // instead of waiting for long-lived gRPC streams to churn on their own.
    spawn_disconnect_active_sync_channels("tunnel_mode_changed");
    Ok(())
}

pub fn get_tunnel() -> Result<TunnelMode> {
    // Ensure registry-backed settings are loaded before reading in-memory mode.
    // Without this, startup reads can observe the default (Tor) and overwrite
    // persisted user choice (e.g. Direct) in higher layers.
    ensure_wallet_registry_loaded()?;
    Ok(TUNNEL_MODE.read().clone())
}

pub async fn bootstrap_tunnel(mode: TunnelMode) -> Result<()> {
    let (transport, socks5_url, _) = tunnel_transport_config_for(&mode);
    // #region agent log
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let (mode_label, socks5_label) = match &mode {
            TunnelMode::Tor => ("tor", "none".to_string()),
            TunnelMode::I2p => ("i2p", "none".to_string()),
            TunnelMode::Direct => ("direct", "none".to_string()),
            TunnelMode::Socks5 { url } => ("socks5", redact_socks5_url(url)),
        };
        let _ = writeln!(
            file,
            r#"{{"id":"log_tunnel_bootstrap","timestamp":{},"location":"tunnel.rs:{}", "message":"bootstrap_tunnel","data":{{"mode":"{}","socks5":"{}"}}}}"#,
            ts,
            line!(),
            mode_label,
            socks5_label
        );
    });
    // #endregion
    pirate_sync_lightd::bootstrap_transport(transport, socks5_url)
        .await
        .map_err(|e| anyhow!("Failed to bootstrap transport: {}", e))?;
    Ok(())
}

/// Shutdown any active transport manager (Tor/I2P/SOCKS5).
pub async fn shutdown_transport() -> Result<()> {
    // #region agent log
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_tunnel_shutdown","timestamp":{},"location":"tunnel.rs:{}", "message":"shutdown_transport","data":{{}}}}"#,
            ts,
            line!()
        );
    });
    // #endregion
    pirate_sync_lightd::shutdown_transport().await;
    mark_runtime_clean_shutdown("shutdown_transport");
    Ok(())
}

/// Configure Tor bridge settings (Snowflake/obfs4/custom) for censorship circumvention.
pub async fn set_tor_bridge_settings(
    use_bridges: bool,
    fallback_to_bridges: bool,
    transport: String,
    bridge_lines: Vec<String>,
    transport_path: Option<String>,
) -> Result<()> {
    pirate_sync_lightd::client::set_tor_bridge_settings(
        use_bridges,
        fallback_to_bridges,
        transport.clone(),
        bridge_lines.clone(),
        transport_path.clone(),
    )?;

    // #region agent log
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_tor_bridge_settings","timestamp":{},"location":"tunnel.rs:{}", "message":"set_tor_bridge_settings","data":{{"use_bridges":{},"fallback_to_bridges":{},"transport":"{}","bridge_lines":{},"transport_path_set":{}}}}}"#,
            ts,
            line!(),
            use_bridges,
            fallback_to_bridges,
            escape_json(&transport),
            bridge_lines.len(),
            transport_path
                .as_ref()
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false)
        );
    });
    // #endregion

    let current = TUNNEL_MODE.read().clone();
    if matches!(current, TunnelMode::Tor) {
        pirate_sync_lightd::bootstrap_transport(TransportMode::Tor, None)
            .await
            .map_err(|e| anyhow!("Failed to bootstrap transport: {}", e))?;
    }

    Ok(())
}

/// Get current Tor bootstrap status for UI.
pub async fn get_tor_status() -> Result<String> {
    let status = pirate_sync_lightd::tor_status().await;
    let payload = match status {
        Some(pirate_sync_lightd::TorStatus::Ready) => "{\"status\":\"ready\"}".to_string(),
        Some(pirate_sync_lightd::TorStatus::Bootstrapping { progress, blocked }) => {
            if let Some(blocked) = blocked {
                format!(
                    "{{\"status\":\"bootstrapping\",\"progress\":{},\"blocked\":\"{}\"}}",
                    progress,
                    escape_json(&blocked)
                )
            } else {
                format!("{{\"status\":\"bootstrapping\",\"progress\":{}}}", progress)
            }
        }
        Some(pirate_sync_lightd::TorStatus::Error(message)) => {
            format!(
                "{{\"status\":\"error\",\"error\":\"{}\"}}",
                escape_json(&message)
            )
        }
        Some(pirate_sync_lightd::TorStatus::NotStarted) | None => {
            "{\"status\":\"not_started\"}".to_string()
        }
    };
    Ok(payload)
}

/// Rotate Tor exit circuits for new streams and reconnect sync channels.
pub async fn rotate_tor_exit() -> Result<()> {
    pirate_sync_lightd::rotate_tor_exit()
        .await
        .map_err(|e| anyhow!("Failed to rotate Tor exit: {}", e))?;

    // #region agent log
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_tor_exit_rotate","timestamp":{},"location":"tunnel.rs:{}", "message":"tor_exit_rotate","data":{{}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            ts,
            line!()
        );
    });
    // #endregion

    disconnect_active_sync_channels("tor_exit_rotate").await;

    Ok(())
}

fn outbound_headers(accept: Option<String>, user_agent: Option<String>) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    if let Some(value) = accept {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            headers.push(("Accept".to_string(), trimmed.to_string()));
        }
    }
    if let Some(value) = user_agent {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            headers.push(("User-Agent".to_string(), trimmed.to_string()));
        }
    }
    headers
}

pub async fn fetch_external_bytes(
    url: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<Vec<u8>> {
    run_on_runtime(move || fetch_external_bytes_inner(url, accept, user_agent)).await
}

async fn fetch_external_bytes_inner(
    url: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<Vec<u8>> {
    let (transport, socks5_url, _) = tunnel_transport_config();
    let headers = outbound_headers(accept, user_agent);
    pirate_sync_lightd::client::fetch_http_bytes(url, headers, transport, socks5_url)
        .await
        .map_err(|e| anyhow!("External fetch failed: {}", e))
}

pub async fn fetch_external_text(
    url: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<String> {
    let bytes = fetch_external_bytes(url, accept, user_agent).await?;
    String::from_utf8(bytes)
        .map_err(|e| anyhow!("External text response was not valid UTF-8: {}", e))
}

pub async fn download_external_to_file(
    url: String,
    destination_path: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<()> {
    let bytes = fetch_external_bytes(url, accept, user_agent).await?;
    let path = PathBuf::from(destination_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, bytes)?;
    Ok(())
}

pub async fn test_node(
    url: String,
    tls_pin: Option<String>,
) -> Result<crate::models::NodeTestResult> {
    run_on_runtime(move || test_node_inner(url, tls_pin)).await
}

async fn test_node_inner(
    url: String,
    tls_pin: Option<String>,
) -> Result<crate::models::NodeTestResult> {
    let start_time = std::time::Instant::now();

    // Extract all needed data before async context to ensure Send
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    tracing::info!(
        "test_node: Using transport mode: {:?}, endpoint: {}",
        transport,
        url
    );

    let endpoint_config = endpoint::endpoint_from_url(&url, true, tls_pin.clone(), None)?;
    let host = endpoint_config.host.clone();
    let port = endpoint_config.port;
    let tls_enabled = endpoint_config.use_tls;
    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();
    let endpoint = endpoint_config.url();

    tracing::info!(
        "test_node: Parsed endpoint URL: {} (TLS: {}, host: {}, port: {}, is_ip: {})",
        endpoint,
        tls_enabled,
        host,
        port,
        is_ip_address
    );

    // Create client config (all data is Send-safe now)
    // For TLS SNI: If connecting via IP address, we need to use the hostname from the certificate
    // The certificate is likely issued for lightd1.piratechain.com, not the IP
    // So we should use the hostname for SNI even when connecting via IP
    let tls_server_name = endpoint::tls_server_name(&endpoint_config);
    if tls_enabled && is_ip_address {
        tracing::info!("test_node: Connecting via IP {}, using hostname 'lightd1.piratechain.com' for TLS SNI to match certificate", host);
    }

    let actual_pin = if tls_enabled {
        let server_name = tls_server_name.clone().unwrap_or_else(|| host.clone());
        match pirate_sync_lightd::fetch_spki_pin(
            &host,
            port,
            Some(server_name),
            transport,
            socks5_url.clone(),
        )
        .await
        {
            Ok(pin) => Some(pin),
            Err(e) => {
                tracing::warn!("test_node: Failed to extract SPKI pin: {}", e);
                None
            }
        }
    } else {
        None
    };

    let tls_pin_matched = match (tls_pin.as_deref(), actual_pin.as_deref()) {
        (Some(expected), Some(actual)) => {
            let expected = expected.strip_prefix("sha256/").unwrap_or(expected);
            let actual = actual.strip_prefix("sha256/").unwrap_or(actual);
            Some(expected == actual)
        }
        (Some(_), None) => None,
        _ => None,
    };

    let (connect_timeout, request_timeout, retry, info_timeout) = match transport {
        TransportMode::Direct => (
            std::time::Duration::from_secs(8),
            std::time::Duration::from_secs(10),
            RetryConfig {
                max_attempts: 1,
                initial_backoff: std::time::Duration::from_millis(100),
                max_backoff: std::time::Duration::from_millis(100),
                backoff_multiplier: 1.0,
            },
            std::time::Duration::from_secs(2),
        ),
        TransportMode::Socks5 => (
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(12),
            RetryConfig {
                max_attempts: 1,
                initial_backoff: std::time::Duration::from_millis(150),
                max_backoff: std::time::Duration::from_millis(150),
                backoff_multiplier: 1.0,
            },
            std::time::Duration::from_secs(3),
        ),
        TransportMode::Tor | TransportMode::I2p => (
            std::time::Duration::from_secs(20),
            std::time::Duration::from_secs(25),
            RetryConfig {
                max_attempts: 2,
                initial_backoff: std::time::Duration::from_millis(250),
                max_backoff: std::time::Duration::from_secs(1),
                backoff_multiplier: 2.0,
            },
            std::time::Duration::from_secs(6),
        ),
    };

    let config = endpoint::build_light_client_config(
        &endpoint_config,
        transport,
        socks5_url,
        allow_direct_fallback,
        retry,
        connect_timeout,
        request_timeout,
    );

    let client = LightClient::with_config(config);

    // Try to connect and get latest block
    tracing::info!(
        "test_node: Attempting to connect to {} (hostname: {})",
        endpoint,
        host
    );
    match client.connect().await {
        Ok(_) => {
            tracing::info!("test_node: Connection successful, fetching latest block...");
            match client.get_latest_block().await {
                Ok(height) => {
                    tracing::info!(
                        "test_node: Successfully retrieved latest block height: {}",
                        height
                    );
                    // Try to get server info if available
                    let (server_version, chain_name) =
                        match tokio::time::timeout(info_timeout, client.get_lightd_info()).await {
                            Ok(Ok(info)) => {
                                tracing::info!(
                                    "test_node: Server info - version: {}, chain: {}",
                                    info.version,
                                    info.chain_name
                                );
                                (Some(info.version), Some(info.chain_name))
                            }
                            Ok(Err(e)) => {
                                tracing::warn!("test_node: Failed to get server info: {}", e);
                                (None, None)
                            }
                            Err(_) => {
                                tracing::warn!(
                                    "test_node: Timed out fetching server info after {:?}",
                                    info_timeout
                                );
                                (None, None)
                            }
                        };

                    let response_time = start_time.elapsed().as_millis() as u64;

                    Ok(crate::models::NodeTestResult {
                        success: true,
                        latest_block_height: Some(height),
                        transport_mode: format!("{:?}", transport),
                        tls_enabled,
                        tls_pin_matched,
                        expected_pin: tls_pin,
                        actual_pin,
                        error_message: None,
                        response_time_ms: response_time,
                        server_version,
                        chain_name,
                    })
                }
                Err(e) => {
                    let response_time = start_time.elapsed().as_millis() as u64;
                    // Clean up error message - remove duplicate "transport error" if present
                    let error_msg = format!("{}", e);
                    let cleaned_error = if error_msg.contains("transport error: transport error") {
                        error_msg.replace("transport error: transport error", "transport error")
                    } else {
                        error_msg
                    };

                    Ok(crate::models::NodeTestResult {
                        success: false,
                        latest_block_height: None,
                        transport_mode: format!("{:?}", transport),
                        tls_enabled,
                        tls_pin_matched,
                        expected_pin: tls_pin,
                        actual_pin,
                        error_message: Some(format!(
                            "Failed to get latest block: {}",
                            cleaned_error
                        )),
                        response_time_ms: response_time,
                        server_version: None,
                        chain_name: None,
                    })
                }
            }
        }
        Err(e) => {
            let response_time = start_time.elapsed().as_millis() as u64;
            tracing::error!(
                "test_node: Connection failed after {}ms: {}",
                response_time,
                e
            );

            // Clean up error message - remove duplicate "transport error" if present
            let error_msg = format!("{}", e);
            let cleaned_error = if error_msg.contains("transport error: transport error") {
                error_msg.replace("transport error: transport error", "transport error")
            } else if error_msg.starts_with("Transport error: ") {
                // Remove redundant "Transport error: " prefix if the inner error already says "transport error"
                let inner = error_msg
                    .strip_prefix("Transport error: ")
                    .unwrap_or(&error_msg);
                if inner.contains("transport error") {
                    inner.to_string()
                } else {
                    error_msg
                }
            } else {
                error_msg
            };

            // Provide more helpful error message
            let final_error = if cleaned_error.contains("dns")
                || cleaned_error.contains("name resolution")
                || cleaned_error.contains("failed to lookup")
                || cleaned_error.contains("Name or service not known")
            {
                format!("DNS resolution failed: {}. The hostname '{}' cannot be resolved to an IP address. This may be a DNS configuration issue on your network. Try using a known good host such as 64.23.167.130:9067 or check your DNS settings.", cleaned_error, host)
            } else if cleaned_error.contains("transport error") {
                format!("Connection failed: {}. The connection attempt failed before we could query the latest block height. This could be due to DNS resolution failure, TLS/certificate issues, or network connectivity problems. Check your network connection, DNS settings, and endpoint URL.", cleaned_error)
            } else {
                format!("Connection failed: {}. Latest block height not retrieved because connection failed.", cleaned_error)
            };

            Ok(crate::models::NodeTestResult {
                success: false,
                latest_block_height: None, // No block height retrieved because connection failed
                transport_mode: format!("{:?}", transport),
                tls_enabled,
                tls_pin_matched,
                expected_pin: tls_pin,
                actual_pin,
                error_message: Some(final_error),
                response_time_ms: response_time,
                server_version: None,
                chain_name: None,
            })
        }
    }
}
