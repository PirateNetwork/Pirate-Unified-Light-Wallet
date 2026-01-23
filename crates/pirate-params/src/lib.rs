//! Pirate Chain network parameters and constants
//!
//! This crate provides network-specific constants, consensus parameters,
//! checkpoint data, and birthday height resolution for Pirate Chain.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod checkpoints;
pub mod consensus;
pub mod network;

pub use checkpoints::{Checkpoint, CheckpointList};
pub use consensus::ConsensusParams;
pub use network::{Network, NetworkType};

/// Error types for parameter operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Invalid network specified
    #[error("Invalid network: {0}")]
    InvalidNetwork(String),

    /// Invalid block height
    #[error("Invalid block height: {0}")]
    InvalidHeight(u32),

    /// Checkpoint not found
    #[error("No checkpoint found for height {0}")]
    CheckpointNotFound(u32),
}

/// Result type for parameter operations
pub type Result<T> = std::result::Result<T, Error>;
