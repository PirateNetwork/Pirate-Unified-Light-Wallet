//! Name resolution for crypto addresses
//!
//! Supports Unstoppable Domains and OpenAlias.
//! Feature flag: `names` (default off)

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod openalias;
pub mod unstoppable;

pub use openalias::OpenAliasResolver;
pub use unstoppable::UnstoppableResolver;

/// Error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Resolution error
    #[error("Resolution error: {0}")]
    Resolution(String),
    
    /// Feature not enabled
    #[error("Feature 'names' not enabled")]
    FeatureDisabled,
    
    /// Invalid name
    #[error("Invalid name: {0}")]
    InvalidName(String),
}

/// Result type
pub type Result<T> = std::result::Result<T, Error>;

