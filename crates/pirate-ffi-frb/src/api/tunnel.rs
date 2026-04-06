use super::*;
use pirate_sync_lightd::client::{RetryConfig, TransportMode};
use std::time::Duration;

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

pub(super) fn load_registry_tunnel_mode(db: &Database) -> Result<Option<TunnelMode>> {
    let mode = get_registry_setting(db, REGISTRY_TUNNEL_MODE_KEY)?;
    let Some(mode_str) = mode else {
        return Ok(None);
    };
    let socks5_url = get_registry_setting(db, REGISTRY_TUNNEL_SOCKS5_URL_KEY)?;
    let mode = match mode_str.as_str() {
        "tor" => Some(TunnelMode::Tor),
        "i2p" => Some(TunnelMode::I2p),
        "socks5" => Some(TunnelMode::Socks5 {
            url: socks5_url.unwrap_or_else(|| "socks5h://localhost:1080".to_string()),
        }),
        "direct" => Some(TunnelMode::Direct),
        _ => None,
    };
    Ok(mode)
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

pub fn set_tunnel(mode: TunnelMode) -> Result<()> {
    service::set_tunnel(convert_into_service(mode)?)
}

pub fn get_tunnel() -> Result<TunnelMode> {
    convert_from_service(service::get_tunnel()?)
}

pub async fn bootstrap_tunnel(mode: TunnelMode) -> Result<()> {
    service::bootstrap_tunnel(convert_into_service(mode)?).await
}

pub async fn shutdown_transport() -> Result<()> {
    service::shutdown_transport().await
}

pub async fn set_tor_bridge_settings(
    use_bridges: bool,
    fallback_to_bridges: bool,
    transport: String,
    bridge_lines: Vec<String>,
    transport_path: Option<String>,
) -> Result<()> {
    service::set_tor_bridge_settings(
        use_bridges,
        fallback_to_bridges,
        transport,
        bridge_lines,
        transport_path,
    )
    .await
}

pub async fn get_tor_status() -> Result<String> {
    service::get_tor_status().await
}

pub async fn rotate_tor_exit() -> Result<()> {
    service::rotate_tor_exit().await
}

pub async fn test_node(
    url: String,
    tls_pin: Option<String>,
) -> Result<crate::models::NodeTestResult> {
    convert_from_service(service::test_node(url, tls_pin).await?)
}
