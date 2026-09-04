use super::*;

pub const DEFAULT_LIGHTD_HOST: &str = service::DEFAULT_LIGHTD_HOST;
pub const DEFAULT_LIGHTD_PORT: u16 = service::DEFAULT_LIGHTD_PORT;
pub const DEFAULT_LIGHTD_USE_TLS: bool = service::DEFAULT_LIGHTD_USE_TLS;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LightdEndpoint {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub tls_pin: Option<String>,
    pub label: Option<String>,
    pub automatic_failover: bool,
    pub failover_endpoints: Vec<String>,
    pub is_configured: bool,
}

impl Default for LightdEndpoint {
    fn default() -> Self {
        Self {
            host: DEFAULT_LIGHTD_HOST.to_string(),
            port: DEFAULT_LIGHTD_PORT,
            use_tls: DEFAULT_LIGHTD_USE_TLS,
            tls_pin: None,
            label: None,
            automatic_failover: false,
            failover_endpoints: Vec::new(),
            is_configured: false,
        }
    }
}

impl LightdEndpoint {
    pub fn url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", scheme, self.host, self.port)
    }

    pub fn display_string(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_endpoint_uses_the_official_tls_server() {
        let endpoint = LightdEndpoint::default();
        assert_eq!(endpoint.url(), "https://lightd1.pirate.black:443");
    }
}
