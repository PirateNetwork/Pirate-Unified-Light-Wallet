//! Error types

/// MM2 errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Process error
    #[error("Process error: {0}")]
    Process(String),
    
    /// RPC error
    #[error("RPC error: {0}")]
    Rpc(String),
    
    /// Swap error
    #[error("Swap error: {0}")]
    Swap(String),
    
    /// Manager error
    #[error("Manager error: {0}")]
    Manager(String),
    
    /// Feature not enabled
    #[error("Feature 'buy_arrr' not enabled")]
    FeatureDisabled,
}

/// Result type
pub type Result<T> = std::result::Result<T, Error>;

