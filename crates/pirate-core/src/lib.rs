//! Pirate Chain wallet core
//!
//! This crate implements the Sapling wallet engine including key derivation,
//! note management, transaction building, and memo handling.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod address;
pub mod diversifier;
pub mod error;
pub mod fees;
pub mod keys;
pub mod memo;
pub mod params;
pub mod notes;
pub mod selection;
pub mod shielded_builder;
pub mod transaction;
pub mod wallet;

pub use address::{AddressManager, SaplingAddress, parse_sapling_address};
pub use diversifier::{
    DiversifierIndex, DiversifierRotationService, DiversifierState, AddressUsage,
    RotationPolicy, DEFAULT_GAP_LIMIT, MAX_DIVERSIFIER_INDEX,
};
pub use error::{Error, ErrorCategory, Result};
pub use fees::{FeeCalculator, FeePolicy, DEFAULT_FEE, ZIP317_MARGINAL_FEE, MIN_FEE, MAX_FEE};
pub use memo::{Memo, MAX_MEMO_LENGTH, MEMO_WARNING_LENGTH};
pub use selection::{NoteSelector, NoteType, SelectableNote, SelectionResult, SelectionStrategy};
pub use shielded_builder::{ShieldedBuilder, ShieldedOutput, PendingShieldedTransaction, SignedShieldedTransaction};
pub use transaction::{TransactionBuilder, TransactionOutput, PendingTransaction, SignedTransaction};
pub use params::{sapling_prover, sapling_params, orchard_params};
