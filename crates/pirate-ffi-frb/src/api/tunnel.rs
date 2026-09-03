use super::*;

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
