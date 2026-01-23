//! Error types

/// Storage errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// Encryption error
    #[error("Encryption error: {0}")]
    Encryption(String),

    /// Migration error
    #[error("Migration error: {0}")]
    Migration(String),

    /// Not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Security error
    #[error("Security error: {0}")]
    Security(String),

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Storage error (generic)
    #[error("Storage error: {0}")]
    Storage(String),
}

/// Result type
pub type Result<T> = std::result::Result<T, Error>;
