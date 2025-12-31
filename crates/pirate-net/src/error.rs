//! Error types

/// Network errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Tor error
    #[error("Tor error: {0}")]
    Tor(String),
    
    /// DNS error
    #[error("DNS error: {0}")]
    Dns(String),
    
    /// TLS error
    #[error("TLS error: {0}")]
    Tls(String),
    
    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),
    
    /// Network error
    #[error("Network error: {0}")]
    Network(String),
    
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type
pub type Result<T> = std::result::Result<T, Error>;

