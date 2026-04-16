//! Pirate Chain wallet core
//!
//! This crate implements the Sapling wallet engine including key derivation,
//! note management, transaction building, and memo handling.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod address;
pub mod change_policy;
pub mod debug_log;
pub mod diversifier;
pub mod error;
pub mod fees;
pub mod keys;
pub mod memo;
pub mod mnemonic;
pub mod notes;
pub mod params;
pub mod qortal_p2sh;
pub mod selection;
pub mod shielded_builder;
pub mod transaction;
pub mod wallet;

pub use address::{parse_sapling_address, AddressManager, SaplingAddress};
pub use change_policy::sapling_internal_change_active;
pub use diversifier::{
    AddressUsage, DiversifierIndex, DiversifierRotationService, DiversifierState, RotationPolicy,
    DEFAULT_GAP_LIMIT, MAX_DIVERSIFIER_INDEX,
};
pub use error::{Error, ErrorCategory, Result};
pub use fees::{
    apply_dust_policy_add_to_fee, EffectiveFeeAndChange, FeeCalculator, FeePolicy,
    CHANGE_DUST_THRESHOLD, DEFAULT_FEE, MAX_FEE, MIN_FEE,
};
pub use memo::{Memo, MAX_MEMO_LENGTH, MEMO_WARNING_LENGTH};
pub use mnemonic::{inspect_mnemonic, MnemonicInspection, MnemonicLanguage};
pub use params::{orchard_params, sapling_params, sapling_prover};
pub use qortal_p2sh::{
    build_p2sh_script_sig, build_qortal_p2sh_funding_transaction,
    build_qortal_p2sh_redeem_transaction, build_script_pubkey, QortalP2shFundingPlan,
    QortalP2shRedeemPlan, QortalRecipient,
};
pub use selection::{NoteSelector, NoteType, SelectableNote, SelectionResult, SelectionStrategy};
pub use shielded_builder::{
    BuildAndSignMultiInputs, PendingShieldedTransaction, ShieldedBuilder, ShieldedOutput,
    SignedShieldedTransaction,
};
pub use transaction::{
    PendingTransaction, SignedTransaction, TransactionBuilder, TransactionOutput,
};
