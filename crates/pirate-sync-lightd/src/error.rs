//! Error types for sync operations

/// Result type
pub type Result<T> = std::result::Result<T, Error>;

/// Error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Network error
    #[error("Network error: {0}")]
    Network(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Sync error
    #[error("Sync error: {0}")]
    Sync(String),

    /// Operation cancelled
    #[error("Cancelled")]
    Cancelled,

    /// Privacy error
    #[error("Privacy error: {0}")]
    Privacy(String),

    /// Transport error
    #[error("Transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    /// Status error
    #[error("Status error: {0}")]
    Status(#[from] tonic::Status),

    /// Storage error
    #[error("Storage error: {0}")]
    Storage(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<pirate_storage_sqlite::Error> for Error {
    fn from(e: pirate_storage_sqlite::Error) -> Self {
        Error::Storage(format!("{}", e))
    }
}
