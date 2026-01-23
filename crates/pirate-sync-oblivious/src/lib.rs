//! Oblivious sync (future feature)
//!
//! Trait-compatible shell for future oblivious sync implementation.
//! Enable with feature flag: `oblivious_sync`

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod provider;

pub use provider::ObliviousProvider;

/// Error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Not implemented
    #[error("Oblivious sync not yet implemented")]
    NotImplemented,
}

/// Result type
pub type Result<T> = std::result::Result<T, Error>;
