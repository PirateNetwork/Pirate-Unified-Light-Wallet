//! SOCKS5 proxy configuration

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Proxy host
    pub host: String,
    /// Proxy port
    pub port: u16,
    /// Username (optional)
    pub username: Option<String>,
    /// Password (optional)
    pub password: Option<String>,
}

impl ProxyConfig {
    /// Create new proxy config
    pub fn new(host: String, port: u16) -> Self {
        Self {
            host,
            port,
            username: None,
            password: None,
        }
    }

    /// With authentication
    pub fn with_auth(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }
}

