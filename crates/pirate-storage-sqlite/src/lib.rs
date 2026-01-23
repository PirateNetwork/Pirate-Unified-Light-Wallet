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
    AddressBookEntry, AddressBookStorage, ColorTag, MAX_LABEL_LENGTH, MAX_NOTES_LENGTH,
};
pub use checkpoints::{Checkpoint, CheckpointManager};
pub use database::Database;
pub use decoy_vault::{
    DecoyBalance, DecoyTransactionList, DecoyVaultConfig, DecoyVaultManager, DecoyVaultStorage,
    DecoyWalletMeta, VaultMode,
};
pub use encryption::EncryptionKey;
pub use error::{Error, Result};
pub use frontier::{FrontierSnapshotRow, FrontierStorage};
pub use keystore::{
    clear_platform_keystore, platform_keystore, set_platform_keystore, BiometricConfig,
    BiometricManager, BiometricState, BiometricType, KeystoreCapabilities, KeystoreManager,
    KeystoreResult, MockKeystore, Platform, PlatformKeystore,
};
pub use models::*;
pub use passphrase_store::{clear_passphrase, get_passphrase, is_passphrase_set, set_passphrase};
pub use repository::Repository;
pub use screenshot_guard::{
    ProtectionReason, ProtectionState, ScreenshotGuard, ScreenshotProtectionGuard,
    ScreenshotProtectionStatus,
};
pub use secure_clipboard::{
    ClipboardDataType, ClipboardPlatform, ClipboardTimer, MockClipboard, SecureClipboard,
    ADDRESS_CLEAR_TIMEOUT_SECS, DEFAULT_CLEAR_TIMEOUT_SECS, SEED_CLEAR_TIMEOUT_SECS,
};
pub use security::{
    generate_salt, hash_sha256, AppPassphrase, EncryptionAlgorithm, MasterKey, PanicPin,
    PassphraseStrength, SealedKey,
};
pub use seed_export::{
    warnings as seed_warnings, ExportAuditEntry, ExportFlowState, SeedExportManager,
    SeedExportRequest, SeedExportResult,
};
pub use sync_state::{
    atomic_sync_update, truncate_above_height, SyncStateRow, SyncStateStorage, BASE_BACKOFF_MS,
    MAX_BACKOFF_MS, MAX_BUSY_RETRIES,
};
pub use watch_only::{
    messages as watch_only_messages, IvkExportResult, IvkImportRequest, WatchOnlyBanner,
    WatchOnlyBannerType, WatchOnlyCapabilities, WatchOnlyManager, WatchOnlyWalletMeta,
};
