//! Encrypted SQLite storage for Pirate Wallet
//!
//! Provides encrypted-at-rest database with WAL mode, migrations,
//! and full schema for accounts, addresses, notes, transactions.
//!
//! ## Security Features
//!
//! - **Database Encryption**: AES-256-GCM or ChaCha20-Poly1305 page encryption
//! - **Master Key Sealing**: Platform keystores (Android Keystore, iOS Keychain, DPAPI, libsecret)
//! - **Passphrase KDF**: Argon2id with 64 MiB memory, 3 iterations, 4 lanes
//! - **Biometric Unlock**: Optional fingerprint/face authentication
//! - **Secure Clipboard**: Auto-clear after 10-60s depending on data type
//! - **Screenshot Blocking**: FLAG_SECURE on Android, secure text field on iOS
//! - **Panic PIN**: Decoy vault for plausible deniability under duress
//! - **Seed Export**: Gated flow with warning → biometric → passphrase
//! - **Watch-Only**: IVK import/export for incoming-only wallets

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod address_book;
pub mod checkpoints;
pub mod database;
pub mod decoy_vault;
pub mod encryption;
pub mod error;
pub mod frontier;
pub mod keystore;
pub mod migrations;
pub mod models;
/// In-memory app passphrase storage.
pub mod passphrase_store;
pub mod repository;
pub mod screenshot_guard;
pub mod secure_clipboard;
pub mod security;
pub mod seed_export;
pub mod sync_state;
pub mod watch_only;

pub use address_book::{
    AddressBookEntry, AddressBookStorage, ColorTag,
    MAX_LABEL_LENGTH, MAX_NOTES_LENGTH,
};
pub use checkpoints::{Checkpoint, CheckpointManager};
pub use database::Database;
pub use decoy_vault::{
    DecoyVaultManager, DecoyVaultConfig, DecoyVaultStorage, VaultMode,
    DecoyBalance, DecoyTransactionList, DecoyWalletMeta,
};
pub use encryption::EncryptionKey;
pub use error::{Error, Result};
pub use frontier::{FrontierStorage, FrontierSnapshotRow};
pub use keystore::{
    PlatformKeystore, MockKeystore, KeystoreManager, KeystoreCapabilities, KeystoreResult,
    Platform, BiometricType, BiometricConfig, BiometricManager, BiometricState,
    clear_platform_keystore, platform_keystore, set_platform_keystore,
};
pub use models::*;
pub use repository::Repository;
pub use passphrase_store::{clear_passphrase, get_passphrase, is_passphrase_set, set_passphrase};
pub use screenshot_guard::{
    ScreenshotGuard, ScreenshotProtectionGuard, ScreenshotProtectionStatus,
    ProtectionState, ProtectionReason,
};
pub use secure_clipboard::{
    SecureClipboard, ClipboardTimer, ClipboardDataType, ClipboardPlatform, MockClipboard,
    DEFAULT_CLEAR_TIMEOUT_SECS, SEED_CLEAR_TIMEOUT_SECS, ADDRESS_CLEAR_TIMEOUT_SECS,
};
pub use security::{MasterKey, AppPassphrase, SealedKey, EncryptionAlgorithm, PanicPin, PassphraseStrength, generate_salt, hash_sha256};
pub use seed_export::{
    SeedExportManager, SeedExportRequest, SeedExportResult, ExportFlowState, ExportAuditEntry,
    warnings as seed_warnings,
};
pub use sync_state::{
    SyncStateStorage, SyncStateRow, atomic_sync_update, truncate_above_height,
    MAX_BUSY_RETRIES, BASE_BACKOFF_MS, MAX_BACKOFF_MS,
};
pub use watch_only::{
    WatchOnlyManager, WatchOnlyCapabilities, IvkExportResult, IvkImportRequest,
    WatchOnlyWalletMeta, WatchOnlyBanner, WatchOnlyBannerType,
    messages as watch_only_messages,
};

