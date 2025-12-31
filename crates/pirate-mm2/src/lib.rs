//! AtomicDEX MM2 integration for buy_arrr feature
//!
//! Provides non-custodial atomic swaps for acquiring ARRR.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod binary;
pub mod client;
pub mod config;
pub mod error;
pub mod manager;
pub mod swap;

pub use binary::Mm2Binary;
pub use client::Mm2Client;
pub use config::Mm2Config;
pub use error::{Error, Result};
pub use manager::{Mm2Manager, Mm2State, Mm2Health};
pub use swap::{SwapStage, SwapProgress, SwapManager, BuyArrrQuote};
