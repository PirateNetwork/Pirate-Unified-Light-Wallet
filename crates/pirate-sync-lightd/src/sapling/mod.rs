//! Sapling helpers adapted from librustzcash for trial decryption of compact outputs.
//! This module provides a minimal helper to trial-decrypt Sapling compact outputs
//! using an IVK derived from the wallet's spending/view key.
//! This is a focused subset and does not bring the full wallet state machine.

pub mod full_decrypt;
pub mod trial_decrypt;
