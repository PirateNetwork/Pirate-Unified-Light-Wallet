//! Public API exposed to Flutter via flutter_rust_bridge
//!
//! This module defines the complete FFI surface for the Pirate Unified Wallet.
//! All functions are designed to be called from Flutter through FRB-generated bindings.
//!
//! ## Architecture
//!
//! - **Wallet Management**: Create, restore, list, switch wallets
//! - **Addresses**: Generate, label, list Sapling addresses
//! - **Transactions**: Build, sign, broadcast transactions
//! - **Sync**: Start/stop sync, rescan, progress tracking
//! - **Security**: Panic PIN, seed export, viewing key export
//! - **Network**: Endpoint management, tunnel configuration
//!
//! ## State Management
//!
//! Global state is managed via `lazy_static` RwLocks. This is suitable for
//! single-process mobile/desktop apps. State is persisted to encrypted SQLite.

use crate::models::*;
use anyhow::Result;
use parking_lot::RwLock;
use pirate_storage_sqlite::Database;
use serde::{de::DeserializeOwned, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;

use pirate_wallet_service as service;

pub(crate) mod background_sync;
pub(crate) mod diagnostics;
pub(crate) mod endpoint;
pub(crate) mod seed_export;
pub(crate) mod tunnel;

pub use self::diagnostics::CheckpointInfo;
pub use self::endpoint::{
    LightdEndpoint, DEFAULT_LIGHTD_HOST, DEFAULT_LIGHTD_PORT, DEFAULT_LIGHTD_USE_TLS,
};
pub use self::seed_export::SeedExportWarnings;
// Global state with thread-safe access
lazy_static::lazy_static! {
    /// Active wallet metadata (persisted to encrypted storage)
    static ref WALLETS: Arc<RwLock<Vec<WalletMeta>>> = Arc::new(RwLock::new(Vec::new()));
    /// Currently active wallet ID
    static ref ACTIVE_WALLET: Arc<RwLock<Option<WalletId>>> = Arc::new(RwLock::new(None));
    /// Network tunnel configuration (Tor default)
    static ref TUNNEL_MODE: Arc<RwLock<TunnelMode>> = Arc::new(RwLock::new(TunnelMode::Tor));
    /// Pending tunnel mode to persist once registry is available.
    static ref PENDING_TUNNEL_MODE: Arc<RwLock<Option<TunnelMode>>> = Arc::new(RwLock::new(None));
}

thread_local! {
    // Keep one opened Database handle per wallet per thread.
    // Unlike the previous leaked static-pointer cache, entries are dropped when
    // the thread exits, so file descriptors are reclaimed.
    static WALLET_DB_CACHE: RefCell<HashMap<String, Box<Database>>> = RefCell::new(HashMap::new());
}

static RUNTIME_DIAGNOSTICS_ONCE: Once = Once::new();
static RUNTIME_DIAGNOSTICS_STOP: AtomicBool = AtomicBool::new(false);
static RUNTIME_LAST_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
static RUNTIME_LAST_FD_PRESSURE_LOG_MS: AtomicU64 = AtomicU64::new(0);
const RUNTIME_MARKER_FILE: &str = "runtime_session.marker";

pub(super) fn convert_from_service<T, U>(value: U) -> Result<T>
where
    T: DeserializeOwned,
    U: Serialize,
{
    let mut value = serde_json::to_value(value)?;
    normalize_service_amount_strings_for_typed_bridge(&mut value);
    Ok(serde_json::from_value(value)?)
}

pub(super) fn convert_into_service<T, U>(value: T) -> Result<U>
where
    T: serde::Serialize,
    U: DeserializeOwned,
{
    convert_from_service(value)
}

// The service JSON ABI emits amount-like integers as decimal strings for
// JavaScript clients. FRB is a typed bridge, so convert those fields back before
// serde deserializes into the Flutter-facing Rust models.
fn normalize_service_amount_strings_for_typed_bridge(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(entries) => {
            for entry in entries {
                normalize_service_amount_strings_for_typed_bridge(entry);
            }
        }
        serde_json::Value::Object(entries) => {
            for (key, entry) in entries {
                if is_service_amount_key(key) {
                    normalize_decimal_integer_string(entry);
                }
                normalize_service_amount_strings_for_typed_bridge(entry);
            }
        }
        _ => {}
    }
}

fn normalize_decimal_integer_string(value: &mut serde_json::Value) {
    let Some(text) = value.as_str().map(str::trim) else {
        return;
    };

    if text.starts_with('-') {
        if let Ok(parsed) = text.parse::<i64>() {
            *value = serde_json::Value::Number(parsed.into());
        }
    } else if let Ok(parsed) = text.parse::<u64>() {
        *value = serde_json::Value::Number(parsed.into());
    }
}

fn is_service_amount_key(key: &str) -> bool {
    matches!(
        key,
        "amount"
            | "available"
            | "balance"
            | "change"
            | "default_fee"
            | "fee"
            | "fee_opt"
            | "fee_per_output"
            | "input_total"
            | "max_fee"
            | "min_fee"
            | "new_balance"
            | "pending"
            | "required"
            | "spendable"
            | "total"
            | "total_amount"
            | "value"
    )
}

fn unix_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn runtime_marker_path() -> PathBuf {
    let log_path = pirate_core::debug_log::debug_log_path();
    let dir = log_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join(RUNTIME_MARKER_FILE)
}

fn read_runtime_marker(path: &Path) -> BTreeMap<String, String> {
    let mut marker = BTreeMap::new();
    let raw = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return marker,
    };
    for line in raw.lines() {
        if let Some((k, v)) = line.split_once('=') {
            marker.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    marker
}

fn write_runtime_marker(path: &Path, marker: &BTreeMap<String, String>) {
    if !pirate_core::debug_log::is_enabled() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut serialized = String::new();
    for (k, v) in marker {
        serialized.push_str(k);
        serialized.push('=');
        serialized.push_str(v);
        serialized.push('\n');
    }
    let _ = fs::write(path, serialized);
}

fn update_runtime_marker<F>(mutator: F)
where
    F: FnOnce(&mut BTreeMap<String, String>),
{
    if !pirate_core::debug_log::is_enabled() {
        return;
    }
    let path = runtime_marker_path();
    let mut marker = read_runtime_marker(&path);
    mutator(&mut marker);
    write_runtime_marker(&path, &marker);
}

fn clear_runtime_marker() {
    let _ = fs::remove_file(runtime_marker_path());
}

fn write_runtime_debug_event(id: &str, message: &str, data_json: &str) {
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = unix_timestamp_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"{}","timestamp":{},"location":"api.rs:runtime","message":"{}","data":{},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
            id,
            ts,
            escape_json(message),
            data_json
        );
    });
}

fn current_linux_fd_count() -> Option<usize> {
    #[cfg(target_os = "linux")]
    {
        fs::read_dir("/proc/self/fd")
            .ok()
            .map(|entries| entries.filter_map(|e| e.ok()).count())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn install_runtime_diagnostics() {
    if !pirate_core::debug_log::is_enabled() {
        return;
    }
    RUNTIME_DIAGNOSTICS_ONCE.call_once(|| {
        let marker_path = runtime_marker_path();
        let previous = read_runtime_marker(&marker_path);
        if !previous.is_empty() {
            let clean_shutdown = previous
                .get("clean_shutdown")
                .map(|v| v == "1")
                .unwrap_or(false);
            if !clean_shutdown {
                let prev_pid = previous
                    .get("pid")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let prev_hb = previous
                    .get("last_heartbeat_ms")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let prev_reason = previous
                    .get("reason")
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                write_runtime_debug_event(
                    "log_runtime_unclean_exit",
                    "previous run did not shut down cleanly",
                    &format!(
                        r#"{{"prev_pid":"{}","prev_last_heartbeat_ms":"{}","prev_reason":"{}","marker":"{}"}}"#,
                        escape_json(&prev_pid),
                        escape_json(&prev_hb),
                        escape_json(&prev_reason),
                        escape_json(&marker_path.display().to_string())
                    ),
                );
            }
        }

        let pid = std::process::id();
        let start_ms = unix_timestamp_millis();
        let mut marker = BTreeMap::new();
        marker.insert("pid".to_string(), pid.to_string());
        marker.insert("started_ms".to_string(), start_ms.to_string());
        marker.insert("last_heartbeat_ms".to_string(), start_ms.to_string());
        marker.insert("clean_shutdown".to_string(), "0".to_string());
        marker.insert("reason".to_string(), "running".to_string());
        if let Some(fd_count) = current_linux_fd_count() {
            marker.insert("fd_count".to_string(), fd_count.to_string());
        }
        write_runtime_marker(&marker_path, &marker);
        RUNTIME_LAST_HEARTBEAT_MS.store(start_ms, Ordering::SeqCst);
        RUNTIME_DIAGNOSTICS_STOP.store(false, Ordering::SeqCst);

        let fd_json = current_linux_fd_count()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string());
        write_runtime_debug_event(
            "log_runtime_start",
            "runtime diagnostics started",
            &format!(
                r#"{{"pid":{},"os":"{}","arch":"{}","started_ms":{},"fd_count":{},"marker":"{}"}}"#,
                pid,
                escape_json(std::env::consts::OS),
                escape_json(std::env::consts::ARCH),
                start_ms,
                fd_json,
                escape_json(&marker_path.display().to_string())
            ),
        );

        let start_ms_for_thread = start_ms;
        let _ = std::thread::Builder::new()
            .name("runtime-diagnostics".to_string())
            .spawn(move || {
                loop {
                    if RUNTIME_DIAGNOSTICS_STOP.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(15));
                    let heartbeat_ms = unix_timestamp_millis();
                    RUNTIME_LAST_HEARTBEAT_MS.store(heartbeat_ms, Ordering::SeqCst);
                    update_runtime_marker(|m| {
                        m.insert("pid".to_string(), pid.to_string());
                        m.insert("started_ms".to_string(), start_ms_for_thread.to_string());
                        m.insert("last_heartbeat_ms".to_string(), heartbeat_ms.to_string());
                        m.insert("clean_shutdown".to_string(), "0".to_string());
                        m.insert("reason".to_string(), "running".to_string());
                        if let Some(fd_count) = current_linux_fd_count() {
                            m.insert("fd_count".to_string(), fd_count.to_string());
                            if fd_count >= 512 {
                                let last_log = RUNTIME_LAST_FD_PRESSURE_LOG_MS
                                    .load(Ordering::SeqCst);
                                if heartbeat_ms.saturating_sub(last_log) >= 60_000 {
                                    RUNTIME_LAST_FD_PRESSURE_LOG_MS
                                        .store(heartbeat_ms, Ordering::SeqCst);
                                    write_runtime_debug_event(
                                        "log_runtime_fd_pressure",
                                        "file descriptor usage is high",
                                        &format!(
                                            r#"{{"pid":{},"fd_count":{},"threshold":512}}"#,
                                            pid, fd_count
                                        ),
                                    );
                                }
                            }
                        }
                    });
                }
            });
    });
}

// ============================================================================
// Wallet Lifecycle
// ============================================================================

/// Create new wallet
///
/// Always generates a 24-word mnemonic seed phrase for new wallets.
/// For restoring wallets with 12 or 18 word seeds, use `restore_wallet()`.
pub fn create_wallet(
    name: String,
    _entropy_len: Option<u32>, // Deprecated: always generates 24-word seed
    birthday_opt: Option<u32>,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<WalletId> {
    let mnemonic_language = match mnemonic_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    service::create_wallet(name, _entropy_len, birthday_opt, mnemonic_language)
}

/// Restore wallet from mnemonic
///
/// Supports restoring wallets with 12, 18, or 24 word mnemonic seeds
/// (for backward compatibility with old wallets that used 12 or 18 word seeds).
/// New wallets created with `create_wallet()` always use 24-word seeds.
pub fn restore_wallet(
    name: String,
    mnemonic: String,
    birthday_opt: Option<u32>,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<WalletId> {
    let mnemonic_language = match mnemonic_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    service::restore_wallet(name, mnemonic, birthday_opt, mnemonic_language)
}

/// Check if wallet registry database file exists (without opening it)
///
/// This allows checking if wallets exist before the database is created or opened.
pub fn wallet_registry_exists() -> Result<bool> {
    service::wallet_registry_exists()
}

/// List all wallets
///
/// Returns empty list if database can't be opened (e.g., passphrase not set)
/// NOTE: This will CREATE the database file if it doesn't exist (via open_wallet_registry)
pub fn list_wallets() -> Result<Vec<WalletMeta>> {
    convert_from_service(service::list_wallets()?)
}

/// Switch active wallet
pub fn switch_wallet(wallet_id: WalletId) -> Result<()> {
    service::switch_wallet(wallet_id)
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Store app passphrase hash for local verification
///
/// IMPORTANT: This function opens/creates the database with the passphrase,
/// then stores the hash and caches the passphrase in memory for this session.
pub fn set_app_passphrase(passphrase: String) -> Result<()> {
    service::set_app_passphrase(passphrase)
}

/// Check if app passphrase is configured
pub fn has_app_passphrase() -> Result<bool> {
    service::has_app_passphrase()
}

/// Verify app passphrase by attempting to open the database with it
pub fn verify_app_passphrase(passphrase: String) -> Result<bool> {
    service::verify_app_passphrase(passphrase)
}

/// Unlock app with passphrase (caches passphrase in memory for wallet access)
/// This allows wallets to be decrypted using the passphrase
pub fn unlock_app(passphrase: String) -> Result<()> {
    service::unlock_app(passphrase)
}

/// Change app passphrase and re-encrypt all wallet data with the new keys.
pub fn change_app_passphrase(current_passphrase: String, new_passphrase: String) -> Result<()> {
    service::change_app_passphrase(current_passphrase, new_passphrase)
}

/// Change passphrase using the cached passphrase from the current session.
pub fn change_app_passphrase_with_cached(new_passphrase: String) -> Result<()> {
    service::change_app_passphrase_with_cached(new_passphrase)
}

/// Reseal registry + wallet DB keys using current platform keystore mode.
///
/// This is used when biometrics are enabled/disabled to rewrap the DB keys
/// under the appropriate keystore policy without changing the passphrase.
pub fn reseal_db_keys_for_biometrics() -> Result<()> {
    service::reseal_db_keys_for_biometrics()
}

/// Get auto-consolidation setting for a wallet.
pub fn get_auto_consolidation_enabled(wallet_id: WalletId) -> Result<bool> {
    service::get_auto_consolidation_enabled(wallet_id)
}

/// Enable or disable auto-consolidation for a wallet.
pub fn set_auto_consolidation_enabled(wallet_id: WalletId, enabled: bool) -> Result<()> {
    service::set_auto_consolidation_enabled(wallet_id, enabled)
}

/// Get the note count threshold that triggers auto-consolidation prompts.
pub fn get_auto_consolidation_threshold() -> Result<u32> {
    service::get_auto_consolidation_threshold()
}

/// Count selectable notes eligible for auto-consolidation.
pub fn get_auto_consolidation_candidate_count(wallet_id: WalletId) -> Result<u32> {
    service::get_auto_consolidation_candidate_count(wallet_id)
}

/// Return deterministic spendability status for the wallet.
pub fn get_spendability_status(wallet_id: WalletId) -> Result<SpendabilityStatus> {
    convert_from_service(service::get_spendability_status(wallet_id)?)
}

/// Get active wallet ID
pub fn get_active_wallet() -> Result<Option<WalletId>> {
    service::get_active_wallet()
}

/// Rename wallet
pub fn rename_wallet(wallet_id: WalletId, new_name: String) -> Result<()> {
    service::rename_wallet(wallet_id, new_name)
}

/// Update wallet birthday height
pub fn set_wallet_birthday_height(wallet_id: WalletId, birthday_height: u32) -> Result<()> {
    service::set_wallet_birthday_height(wallet_id, birthday_height)
}

/// Delete wallet and its local database
pub fn delete_wallet(wallet_id: WalletId) -> Result<()> {
    service::delete_wallet(wallet_id)
}

// ============================================================================
// Addresses
// ============================================================================

/// Get current receive address for wallet
///
/// Returns the current diversified Sapling address from storage.
/// If no address exists, generates and stores the first address (index 0).
/// Call `next_receive_address` to rotate to a new unlinkable address.
pub fn current_receive_address(wallet_id: WalletId) -> Result<String> {
    service::current_receive_address(wallet_id)
}

/// Generate next receive address (diversifier rotation)
///
/// Increments the diversifier index to generate a fresh, unlinkable address.
/// Address type (Sapling or Ironwood) is determined by network and current block height.
/// Previous addresses remain valid for receiving funds.
pub fn next_receive_address(wallet_id: WalletId) -> Result<String> {
    service::next_receive_address(wallet_id)
}

/// Label an address for address book
pub fn label_address(wallet_id: WalletId, addr: String, label: String) -> Result<()> {
    service::label_address(wallet_id, addr, label)
}

/// Set color tag for a wallet address
pub fn set_address_color_tag(
    wallet_id: WalletId,
    addr: String,
    color_tag: AddressBookColorTag,
) -> Result<()> {
    service::set_address_color_tag(
        convert_into_service(wallet_id)?,
        addr,
        convert_into_service(color_tag)?,
    )
}

/// Get user-managed display preferences for wallet addresses.
pub fn list_address_display_preferences(
    wallet_id: WalletId,
) -> Result<Vec<AddressDisplayPreferenceInfo>> {
    convert_from_service(service::list_address_display_preferences(wallet_id)?)
}

/// Pin or unpin a wallet address in user interfaces.
pub fn set_address_pinned(wallet_id: WalletId, address_id: i64, is_pinned: bool) -> Result<()> {
    service::set_address_pinned(wallet_id, address_id, is_pinned)
}

/// Archive or restore a wallet address in user interfaces.
pub fn set_address_archived(wallet_id: WalletId, address_id: i64, is_archived: bool) -> Result<()> {
    service::set_address_archived(wallet_id, address_id, is_archived)
}

/// Get all addresses for wallet with labels
pub fn list_addresses(wallet_id: WalletId) -> Result<Vec<AddressInfo>> {
    convert_from_service(service::list_addresses(wallet_id)?)
}

/// Get per-address balances for a wallet (optionally filtered by key group).
pub fn list_address_balances(
    wallet_id: WalletId,
    key_id: Option<i64>,
) -> Result<Vec<AddressBalanceInfo>> {
    convert_from_service(service::list_address_balances(wallet_id, key_id)?)
}

// ============================================================================
// Address Book
// ============================================================================

/// List address book entries for a wallet
pub fn list_address_book(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    convert_from_service(service::list_address_book(wallet_id)?)
}

/// Add an address book entry
pub fn add_address_book_entry(
    wallet_id: WalletId,
    address: String,
    label: String,
    notes: Option<String>,
    color_tag: AddressBookColorTag,
) -> Result<AddressBookEntryFfi> {
    convert_from_service(service::add_address_book_entry(
        wallet_id,
        address,
        label,
        notes,
        convert_into_service(color_tag)?,
    )?)
}

/// Update an address book entry
pub fn update_address_book_entry(
    wallet_id: WalletId,
    id: i64,
    label: Option<String>,
    notes: Option<String>,
    color_tag: Option<AddressBookColorTag>,
    is_favorite: Option<bool>,
) -> Result<AddressBookEntryFfi> {
    convert_from_service(service::update_address_book_entry(
        wallet_id,
        id,
        label,
        notes,
        match color_tag {
            Some(value) => Some(convert_into_service(value)?),
            None => None,
        },
        is_favorite,
    )?)
}

/// Delete an address book entry
pub fn delete_address_book_entry(wallet_id: WalletId, id: i64) -> Result<()> {
    service::delete_address_book_entry(wallet_id, id)
}

/// Toggle favorite status for an entry
pub fn toggle_address_book_favorite(wallet_id: WalletId, id: i64) -> Result<bool> {
    service::toggle_address_book_favorite(wallet_id, id)
}

/// Mark an address as used
pub fn mark_address_used(wallet_id: WalletId, address: String) -> Result<()> {
    service::mark_address_used(wallet_id, address)
}

/// Get label for an address
pub fn get_label_for_address(wallet_id: WalletId, address: String) -> Result<Option<String>> {
    service::get_label_for_address(wallet_id, address)
}

/// Check if an address exists in the book
pub fn address_exists_in_book(wallet_id: WalletId, address: String) -> Result<bool> {
    service::address_exists_in_book(wallet_id, address)
}

/// Count address book entries
pub fn get_address_book_count(wallet_id: WalletId) -> Result<u32> {
    service::get_address_book_count(wallet_id)
}

/// Get entry by ID
pub fn get_address_book_entry(wallet_id: WalletId, id: i64) -> Result<Option<AddressBookEntryFfi>> {
    convert_from_service(service::get_address_book_entry(wallet_id, id)?)
}

/// Get entry by address
pub fn get_address_book_entry_by_address(
    wallet_id: WalletId,
    address: String,
) -> Result<Option<AddressBookEntryFfi>> {
    convert_from_service(service::get_address_book_entry_by_address(
        wallet_id, address,
    )?)
}

/// Search entries by query
pub fn search_address_book(wallet_id: WalletId, query: String) -> Result<Vec<AddressBookEntryFfi>> {
    convert_from_service(service::search_address_book(wallet_id, query)?)
}

/// List favorites
pub fn get_address_book_favorites(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    convert_from_service(service::get_address_book_favorites(wallet_id)?)
}

/// List recently used addresses
pub fn get_recently_used_addresses(
    wallet_id: WalletId,
    limit: u32,
) -> Result<Vec<AddressBookEntryFfi>> {
    convert_from_service(service::get_recently_used_addresses(wallet_id, limit)?)
}

// ============================================================================
// Watch-Only
// ============================================================================

/// Export Sapling viewing key from full wallet.
///
/// Uses the zxviews... Bech32 format for watch-only wallets.
pub fn export_sapling_viewing_key(wallet_id: WalletId) -> Result<String> {
    service::export_sapling_viewing_key(wallet_id)
}

/// Export Ironwood Extended Full Viewing Key as Bech32 (for watch-only wallets)
///
/// Returns Bech32-encoded string with the network-specific HRP.
/// Uses the standard Ironwood viewing key export format.
/// Use export_sapling_viewing_key() for Sapling viewing keys (zxviews... format).
pub fn export_ironwood_viewing_key(wallet_id: WalletId) -> Result<String> {
    service::export_ironwood_viewing_key(wallet_id)
}

/// Import viewing keys (watch-only wallet).
///
/// Supports Sapling viewing keys (zxviews...) and Ironwood extended viewing keys (bech32).
/// If both are provided, creates a watch-only wallet that can view both Sapling and Ironwood transactions.
pub fn import_viewing_wallet(
    name: String,
    sapling_viewing_key: Option<String>,
    ironwood_viewing_key: Option<String>,
    birthday: u32,
) -> Result<WalletId> {
    service::import_viewing_wallet(name, sapling_viewing_key, ironwood_viewing_key, birthday)
}

// ============================================================================
// Key Management
// ============================================================================

/// List key groups for the active wallet account.
pub fn list_key_groups(wallet_id: WalletId) -> Result<Vec<KeyGroupInfo>> {
    convert_from_service(service::list_key_groups(wallet_id)?)
}

/// Add the next one or five durable ZIP-32 accounts derived from the wallet seed.
pub fn add_next_seed_accounts(wallet_id: WalletId, count: u32) -> Result<Vec<u32>> {
    service::add_next_seed_accounts(wallet_id, count)
}

/// Export viewing/spending keys for a specific key group.
pub fn export_key_group_keys(wallet_id: WalletId, key_id: i64) -> Result<KeyExportInfo> {
    convert_from_service(service::export_key_group_keys(wallet_id, key_id)?)
}

/// List addresses for a specific key group.
pub fn list_addresses_for_key(wallet_id: WalletId, key_id: i64) -> Result<Vec<KeyAddressInfo>> {
    convert_from_service(service::list_addresses_for_key(wallet_id, key_id)?)
}

/// Generate a new address for a specific key group.
pub fn generate_address_for_key(
    wallet_id: WalletId,
    key_id: i64,
    use_ironwood: bool,
) -> Result<String> {
    service::generate_address_for_key(wallet_id, key_id, use_ironwood)
}

/// Import a spending key into an existing wallet.
pub fn import_spending_key(
    wallet_id: WalletId,
    sapling_key: Option<String>,
    ironwood_key: Option<String>,
    label: Option<String>,
    birthday_height: u32,
) -> Result<i64> {
    service::import_spending_key(wallet_id, sapling_key, ironwood_key, label, birthday_height)
}

/// Export mnemonic seed through the raw advanced path.
///
/// This path is intended for advanced callers that implement their own local
/// authorization UX. It does not use the app-gated seed export flow.
///
/// Note: Only works for wallets created/restored from seed.
/// Wallets imported from private key or watch-only wallets cannot export seed.
pub fn export_seed_raw(
    wallet_id: WalletId,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<String> {
    let mnemonic_language = match mnemonic_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    service::export_seed_raw(wallet_id, mnemonic_language)
}

/// Export the active wallet seed for immediate KDF swap-engine startup.
///
/// This uses the Rust service's KDF-specific guardrails and rejects decoy,
/// locked, watch-only, and seedless/private-key-import wallet states. The
/// mnemonic is always rendered as English BIP39 for KDF/Komodo compatibility.
pub fn export_seed_for_kdf(wallet_id: WalletId) -> Result<String> {
    service::export_seed_for_kdf(wallet_id)
}

// ============================================================================
// Send (Send-to-Many with per-output memos)
// ============================================================================

/// Maximum number of outputs per transaction
pub const MAX_OUTPUTS_PER_TX: usize = 50;

/// Build transaction with note selection, fee calculation, and change.
pub fn build_tx(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    let outputs = convert_into_service(outputs)?;
    convert_from_service(service::build_tx(wallet_id, outputs, fee_opt)?)
}

/// Build transaction using notes from a specific key group.
pub fn build_tx_for_key(
    wallet_id: WalletId,
    key_id: i64,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    let outputs = convert_into_service(outputs)?;
    convert_from_service(service::build_tx_for_key(
        wallet_id, key_id, outputs, fee_opt,
    )?)
}

/// Build transaction using selected key groups or addresses.
pub fn build_tx_filtered(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    let outputs = convert_into_service(outputs)?;
    convert_from_service(service::build_tx_filtered(
        wallet_id,
        outputs,
        fee_opt,
        key_ids_filter,
        address_ids_filter,
    )?)
}

/// Build a consolidation transaction for a key group.
pub fn build_consolidation_tx(
    wallet_id: WalletId,
    key_id: i64,
    target_address: String,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    convert_from_service(service::build_consolidation_tx(
        wallet_id,
        key_id,
        target_address,
        fee_opt,
    )?)
}

/// Build a sweep transaction from selected key groups or addresses.
/// Sends the full available balance minus fee to the target address.
pub fn build_sweep_tx(
    wallet_id: WalletId,
    target_address: String,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    convert_from_service(service::build_sweep_tx(
        wallet_id,
        target_address,
        fee_opt,
        key_ids_filter,
        address_ids_filter,
    )?)
}

/// Sign pending transaction (all spendable notes in the wallet)
pub fn sign_tx(wallet_id: WalletId, pending: PendingTx) -> Result<SignedTx> {
    let pending = convert_into_service(pending)?;
    convert_from_service(service::sign_tx(wallet_id, pending)?)
}

/// Sign pending transaction using notes from a specific key group
pub fn sign_tx_for_key(wallet_id: WalletId, pending: PendingTx, key_id: i64) -> Result<SignedTx> {
    let pending = convert_into_service(pending)?;
    convert_from_service(service::sign_tx_for_key(wallet_id, pending, key_id)?)
}

/// Sign pending transaction using selected key groups or addresses.
pub fn sign_tx_filtered(
    wallet_id: WalletId,
    pending: PendingTx,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<SignedTx> {
    let pending = convert_into_service(pending)?;
    convert_from_service(service::sign_tx_filtered(
        wallet_id,
        pending,
        key_ids_filter,
        address_ids_filter,
    )?)
}

/// Broadcast signed transaction to the network
///
/// Sends transaction via lightwalletd gRPC SendTransaction.
/// Returns TxId on success, or error with details.
pub async fn broadcast_tx(signed: SignedTx) -> Result<TxId> {
    let signed = convert_into_service(signed)?;
    service::broadcast_tx(signed).await
}

/// Estimate fee for transaction without building it
pub fn estimate_fee(num_outputs: usize, has_memo: bool, fee_policy: Option<String>) -> Result<u64> {
    service::estimate_fee(num_outputs, has_memo, fee_policy)
}

/// Get fee information
pub fn get_fee_info() -> Result<FeeInfo> {
    convert_from_service(service::get_fee_info()?)
}

/// Fee information for UI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeeInfo {
    /// Default fee (fixed)
    pub default_fee: u64,
    /// Minimum allowed fee
    pub min_fee: u64,
    /// Maximum allowed fee
    pub max_fee: u64,
    /// Additional fee per output (fixed fee uses 0)
    pub fee_per_output: u64,
    /// Fee multiplier when memo is included (fixed fee uses 1.0)
    pub memo_fee_multiplier: f64,
}

// ============================================================================
// Sync
// ============================================================================
pub async fn start_sync(wallet_id: WalletId, mode: SyncMode) -> Result<()> {
    service::start_sync(wallet_id, convert_into_service(mode)?).await
}

/// Get sync status for a wallet with full performance metrics
pub fn sync_status(wallet_id: WalletId) -> Result<SyncStatus> {
    convert_from_service(service::sync_status(wallet_id)?)
}

/// Get last checkpoint info for diagnostics
pub fn get_last_checkpoint(wallet_id: WalletId) -> Result<Option<CheckpointInfo>> {
    convert_from_service(service::get_last_checkpoint(wallet_id)?)
}

/// Rescan wallet from specific height
pub async fn rescan(wallet_id: WalletId, from_height: u32) -> Result<()> {
    service::rescan(wallet_id, from_height).await
}

/// Cancel ongoing sync for a wallet.
pub async fn cancel_sync(wallet_id: WalletId) -> Result<()> {
    service::cancel_sync(wallet_id).await
}

/// Check if sync is running for a wallet
pub fn is_sync_running(wallet_id: WalletId) -> Result<bool> {
    service::is_sync_running(wallet_id)
}

// ============================================================================
// Background Sync
// ============================================================================

/// Start background sync for a wallet
///
/// This should be called from iOS BGAppRefreshTask or Android WorkManager.
/// The sync will run with time limits and battery constraints.
///
/// Note: This creates a new SyncEngine instance for background sync to avoid
/// conflicts with foreground sync. The background sync will use the same
/// wallet database and storage.
pub async fn start_background_sync(
    wallet_id: WalletId,
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::BackgroundSyncResult> {
    background_sync::start_background_sync(wallet_id, mode, max_duration_secs, max_blocks).await
}

/// Start background sync using round-robin scheduling with warm-wallet priority.
///
/// Chooses the next wallet to sync based on recent usage and rotates fairly
/// across wallets over successive runs.
pub async fn start_background_sync_round_robin(
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::WalletBackgroundSyncResult> {
    background_sync::start_background_sync_round_robin(mode, max_duration_secs, max_blocks).await
}

/// Check if background sync is needed for a wallet
pub async fn is_background_sync_needed(wallet_id: WalletId) -> Result<bool> {
    background_sync::is_background_sync_needed(wallet_id).await
}

/// Get recommended background sync mode based on time since last sync
pub fn get_recommended_background_sync_mode(
    wallet_id: WalletId,
    minutes_since_last: u32,
) -> Result<String> {
    background_sync::get_recommended_background_sync_mode(wallet_id, minutes_since_last)
}

// ============================================================================
// Nodes & Endpoints
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct WitnessRefreshOutcome {
    pub source: String,
    pub sapling_requested: usize,
    pub sapling_updated: usize,
    pub sapling_missing: usize,
    pub sapling_errors: usize,
    pub ironwood_requested: usize,
    pub ironwood_updated: usize,
    pub ironwood_missing: usize,
    pub ironwood_errors: usize,
}

/// Set lightwalletd endpoint
pub fn set_lightd_endpoint(
    wallet_id: WalletId,
    url: String,
    tls_pin_opt: Option<String>,
) -> Result<()> {
    service::set_lightd_endpoint(wallet_id, url, tls_pin_opt)
}

/// Set a lightwalletd endpoint with explicit same-network alternates.
pub fn set_lightd_endpoint_pool(
    wallet_id: WalletId,
    url: String,
    tls_pin_opt: Option<String>,
    failover_endpoints: Vec<String>,
) -> Result<()> {
    service::set_lightd_endpoint_pool(wallet_id, url, tls_pin_opt, failover_endpoints)
}

/// Get lightwalletd endpoint
pub fn get_lightd_endpoint(wallet_id: WalletId) -> Result<String> {
    service::get_lightd_endpoint(wallet_id)
}

/// Get full endpoint configuration
pub fn get_lightd_endpoint_config(wallet_id: WalletId) -> Result<LightdEndpoint> {
    convert_from_service(service::get_lightd_endpoint_config(wallet_id)?)
}

// ============================================================================
// Network Tunnel
// ============================================================================

/// Set network tunnel mode
pub fn set_tunnel(mode: TunnelMode) -> Result<()> {
    tunnel::set_tunnel(mode)
}

/// Get current tunnel mode
pub fn get_tunnel() -> Result<TunnelMode> {
    tunnel::get_tunnel()
}

/// Bootstrap tunnel transport early (Tor/I2P/SOCKS5) without unlocking wallets.
pub async fn bootstrap_tunnel(mode: TunnelMode) -> Result<()> {
    tunnel::bootstrap_tunnel(mode).await
}

/// Shutdown any active transport manager (Tor/I2P/SOCKS5).
pub async fn shutdown_transport() -> Result<()> {
    tunnel::shutdown_transport().await
}

/// Configure Tor bridge settings (Snowflake/obfs4/custom) for censorship circumvention.
pub async fn set_tor_bridge_settings(
    use_bridges: bool,
    fallback_to_bridges: bool,
    transport: String,
    bridge_lines: Vec<String>,
    transport_path: Option<String>,
) -> Result<()> {
    tunnel::set_tor_bridge_settings(
        use_bridges,
        fallback_to_bridges,
        transport,
        bridge_lines,
        transport_path,
    )
    .await
}

/// Get current Tor bootstrap status for UI.
pub async fn get_tor_status() -> Result<String> {
    tunnel::get_tor_status().await
}

/// Rotate Tor exit circuits for new streams and reconnect sync channels.
pub async fn rotate_tor_exit() -> Result<()> {
    tunnel::rotate_tor_exit().await
}

/// Fetch arbitrary text over the currently selected network tunnel.
pub async fn fetch_external_text(
    url: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<String> {
    service::fetch_external_text(url, accept, user_agent).await
}

/// Fetch arbitrary bytes over the currently selected network tunnel.
pub async fn fetch_external_bytes(
    url: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<Vec<u8>> {
    service::fetch_external_bytes(url, accept, user_agent).await
}

/// Download an external resource to a local file over the currently selected network tunnel.
pub async fn download_external_to_file(
    url: String,
    destination_path: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<()> {
    service::download_external_to_file(url, destination_path, accept, user_agent).await
}

// ============================================================================
// Balance & Transactions
// ============================================================================

/// Get wallet balance
///
/// Calculates balance from unspent notes in the database.
/// - spendable: Confirmed unspent notes (with 1+ confirmation)
/// - pending: Unconfirmed unspent notes
/// - total: spendable + pending
pub fn get_balance(wallet_id: WalletId) -> Result<Balance> {
    convert_from_service(service::get_balance(wallet_id)?)
}

/// List transactions
///
/// Returns transaction history from the database, aggregated by transaction ID.
/// Pending transactions are returned first, followed by confirmed transactions
/// in descending block-height order.
pub fn list_transactions(wallet_id: WalletId, limit: Option<u32>) -> Result<Vec<TxInfo>> {
    convert_from_service(service::list_transactions(wallet_id, limit)?)
}

/// List one stable page of transaction history.
pub fn list_transactions_page(
    wallet_id: WalletId,
    cursor: Option<TransactionCursor>,
    page_size: u32,
) -> Result<TransactionPage> {
    convert_from_service(service::list_transactions_page(
        wallet_id,
        convert_into_service(cursor)?,
        page_size,
    )?)
}

/// Fetch and decrypt memo for a specific transaction (lazy memo decoding)
///
/// This function implements lazy memo decoding:
/// 1. Checks if memo already exists in database
/// 2. If exists, validates it by re-decrypting to ensure it's correct
/// 3. If missing or corrupted, fetches full transaction and decrypts memo
/// 4. Stores memo in database for future use
///
/// # Arguments
/// * `wallet_id` - Wallet ID
/// * `txid` - Transaction ID (hex string)
/// * `output_index` - Optional output index (if None, returns first memo found)
///
/// # Returns
/// Decoded memo string, or None if no memo exists or decryption fails
pub async fn fetch_transaction_memo(
    wallet_id: WalletId,
    txid: String,
    output_index: Option<u32>,
) -> Result<Option<String>> {
    service::fetch_transaction_memo(wallet_id, txid, output_index).await
}

/// Export all payment disclosures recoverable by this wallet for an outgoing transaction.
pub async fn export_payment_disclosures(
    wallet_id: WalletId,
    txid: String,
) -> Result<Vec<PaymentDisclosure>> {
    convert_from_service(service::export_payment_disclosures(wallet_id, txid).await?)
}

/// Export a Sapling payment disclosure for a specific output index.
pub async fn export_sapling_payment_disclosure(
    wallet_id: WalletId,
    txid: String,
    output_index: u32,
) -> Result<String> {
    service::export_sapling_payment_disclosure(wallet_id, txid, output_index).await
}

/// Export an Ironwood payment disclosure for a specific action index.
pub async fn export_ironwood_payment_disclosure(
    wallet_id: WalletId,
    txid: String,
    action_index: u32,
) -> Result<String> {
    service::export_ironwood_payment_disclosure(wallet_id, txid, action_index).await
}

/// Verify and decrypt a Sapling or Ironwood payment disclosure.
pub async fn verify_payment_disclosure(
    wallet_id: WalletId,
    disclosure: String,
) -> Result<PaymentDisclosureVerification> {
    convert_from_service(service::verify_payment_disclosure(wallet_id, disclosure).await?)
}

// ============================================================================
// Utilities
// ============================================================================

/// Generate new mnemonic (utility function for testing/development)
///
/// **Note**: New wallets always use 24-word seeds. This function is provided
/// for testing/utilities. For wallet creation, use `create_wallet()` which
/// always generates 24-word seeds.
///
/// # Arguments
/// * `word_count` - Number of words in mnemonic (12, 18, or 24). Defaults to 24 if None.
///
/// # Returns
/// BIP39 mnemonic phrase with the specified number of words
pub fn generate_mnemonic(
    word_count: Option<u32>,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<String> {
    let mnemonic_language = match mnemonic_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    service::generate_mnemonic(word_count, mnemonic_language)
}

/// Validate mnemonic
pub fn validate_mnemonic(
    mnemonic: String,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<bool> {
    let mnemonic_language = match mnemonic_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    service::validate_mnemonic(mnemonic, mnemonic_language)
}

/// Inspect mnemonic validity, language, and ambiguity.
pub fn inspect_mnemonic(mnemonic: String) -> Result<MnemonicInspection> {
    convert_from_service(service::inspect_mnemonic(mnemonic)?)
}

/// Convert a mnemonic phrase to a different display language while preserving seed entropy.
pub fn convert_mnemonic_language(
    mnemonic: String,
    source_language: Option<MnemonicLanguage>,
    target_language: MnemonicLanguage,
) -> Result<String> {
    let source_language = match source_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    let target_language = convert_into_service(target_language)?;
    service::convert_mnemonic_language(mnemonic, source_language, target_language)
}

/// Get network info
pub fn get_network_info() -> Result<NetworkInfo> {
    convert_from_service(service::get_network_info()?)
}

/// Format amount (arrrtoshis to ARRR)
pub fn format_amount(arrrtoshis: u64) -> Result<String> {
    service::format_amount(arrrtoshis)
}

/// Parse amount (ARRR to arrrtoshis)
pub fn parse_amount(arrr: String) -> Result<u64> {
    service::parse_amount(arrr)
}

// ============================================================================
// Security Features
// ============================================================================

use pirate_storage_sqlite::WatchOnlyManager;

lazy_static::lazy_static! {
    /// Global watch-only manager
    static ref WATCH_ONLY: Arc<RwLock<WatchOnlyManager>> =
        Arc::new(RwLock::new(WatchOnlyManager::new()));
}

// ============================================================================
// Panic PIN / Decoy Vault
// ============================================================================

/// Set panic PIN for decoy vault
pub fn set_panic_pin(pin: String) -> Result<()> {
    service::set_panic_pin(pin)
}

/// Check if panic PIN is configured
pub fn has_panic_pin() -> Result<bool> {
    service::has_panic_pin()
}

/// Verify panic PIN (returns true if PIN matches and activates decoy mode)
pub fn verify_panic_pin(pin: String) -> Result<bool> {
    service::verify_panic_pin(pin)
}

/// Check if currently in decoy mode
pub fn is_decoy_mode() -> Result<bool> {
    service::is_decoy_mode()
}

/// Get current vault mode
pub fn get_vault_mode() -> Result<String> {
    service::get_vault_mode()
}

/// Clear panic PIN and disable decoy vault
pub fn clear_panic_pin() -> Result<()> {
    service::clear_panic_pin()
}

/// Set duress passphrase for decoy vault.
pub fn set_duress_passphrase(custom_passphrase: Option<String>) -> Result<()> {
    service::set_duress_passphrase(custom_passphrase)
}

/// Check if a duress passphrase is configured
pub fn has_duress_passphrase() -> Result<bool> {
    service::has_duress_passphrase()
}

/// Clear duress passphrase configuration
pub fn clear_duress_passphrase() -> Result<()> {
    service::clear_duress_passphrase()
}

/// Verify duress passphrase (activates decoy mode if correct)
pub fn verify_duress_passphrase(passphrase: String) -> Result<bool> {
    service::verify_duress_passphrase(passphrase)
}

/// Set decoy wallet name
pub fn set_decoy_wallet_name(name: String) -> Result<()> {
    service::set_decoy_wallet_name(name)
}

/// Exit decoy mode (requires real passphrase re-authentication).
pub fn exit_decoy_mode(passphrase: String) -> Result<()> {
    service::exit_decoy_mode(passphrase)
}

// ============================================================================
// Debug Logging
// ============================================================================

pub fn set_debug_logging_enabled(enabled: bool) -> Result<()> {
    pirate_core::debug_log::set_enabled(enabled);
    if enabled {
        RUNTIME_DIAGNOSTICS_STOP.store(false, Ordering::SeqCst);
        install_runtime_diagnostics();
    } else {
        RUNTIME_DIAGNOSTICS_STOP.store(true, Ordering::SeqCst);
        clear_runtime_marker();
    }
    Ok(())
}

pub fn get_debug_logging_enabled() -> Result<bool> {
    Ok(pirate_core::debug_log::is_enabled())
}

pub fn clear_debug_logs() -> Result<()> {
    pirate_core::debug_log::clear_logs();
    clear_runtime_marker();
    Ok(())
}

// ============================================================================
// Seed Export (Gated Flow)
// ============================================================================

/// Start seed export flow (step 1: show warning)
pub fn start_seed_export(wallet_id: WalletId) -> Result<String> {
    seed_export::start_seed_export(wallet_id)
}

/// Acknowledge seed export warning (step 2)
pub fn acknowledge_seed_warning() -> Result<String> {
    seed_export::acknowledge_seed_warning()
}

/// Complete biometric step (step 3)
pub fn complete_seed_biometric(success: bool) -> Result<String> {
    seed_export::complete_seed_biometric(success)
}

/// Skip biometric (when not available)
pub fn skip_seed_biometric() -> Result<String> {
    seed_export::skip_seed_biometric()
}

/// Verify passphrase and get seed (step 4 - final)
///
/// This is the final step of the gated seed export flow.
/// Verifies passphrase against stored Argon2id hash before returning the seed.
///
/// Note: Only works for wallets created/restored from seed.
/// Wallets imported from private key or watch-only wallets cannot export seed.
pub fn export_seed_with_passphrase(
    wallet_id: WalletId,
    passphrase: String,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<Vec<String>> {
    let mnemonic_language = match mnemonic_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    seed_export::export_seed_with_passphrase(wallet_id, passphrase, mnemonic_language)
}

/// Export seed using cached app passphrase (after biometric approval).
pub fn export_seed_with_cached_passphrase(
    wallet_id: WalletId,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<Vec<String>> {
    let mnemonic_language = match mnemonic_language {
        Some(value) => Some(convert_into_service(value)?),
        None => None,
    };
    seed_export::export_seed_with_cached_passphrase(wallet_id, mnemonic_language)
}

/// Cancel seed export flow
pub fn cancel_seed_export() -> Result<()> {
    seed_export::cancel_seed_export()
}

/// Get current seed export flow state
pub fn get_seed_export_state() -> Result<String> {
    seed_export::get_seed_export_state()
}

/// Check if screenshots are blocked during export
pub fn are_seed_screenshots_blocked() -> Result<bool> {
    seed_export::are_seed_screenshots_blocked()
}

/// Get clipboard auto-clear remaining seconds
pub fn get_seed_clipboard_remaining() -> Result<Option<u64>> {
    seed_export::get_seed_clipboard_remaining()
}

/// Get seed export warning messages
pub fn get_seed_export_warnings() -> Result<SeedExportWarnings> {
    seed_export::get_seed_export_warnings()
}

// ============================================================================
// Watch-Only / Viewing Key Export/Import
// ============================================================================

/// Export Sapling viewing key from full wallet (for creating watch-only on another device)
pub fn export_sapling_viewing_key_secure(wallet_id: WalletId) -> Result<String> {
    service::export_sapling_viewing_key_secure(wallet_id)
}

/// Import Sapling viewing key to create watch-only wallet
pub fn import_sapling_viewing_key_as_watch_only(
    name: String,
    sapling_viewing_key: String,
    birthday_height: u32,
) -> Result<WalletId> {
    service::import_sapling_viewing_key_as_watch_only(name, sapling_viewing_key, birthday_height)
}

/// Get watch-only capabilities for a wallet
pub fn get_watch_only_capabilities(wallet_id: WalletId) -> Result<WatchOnlyCapabilitiesInfo> {
    convert_from_service(service::get_watch_only_capabilities(wallet_id)?)
}

/// Watch-only capabilities for FFI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WatchOnlyCapabilitiesInfo {
    pub can_view_incoming: bool,
    pub can_view_outgoing: bool,
    pub can_spend: bool,
    pub can_export_seed: bool,
    pub can_generate_addresses: bool,
    pub is_watch_only: bool,
}

/// Get watch-only banner info for a wallet
pub fn get_watch_only_banner(wallet_id: WalletId) -> Result<Option<WatchOnlyBannerInfo>> {
    convert_from_service(service::get_watch_only_banner(wallet_id)?)
}

/// Watch-only banner info for FFI
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WatchOnlyBannerInfo {
    pub banner_type: String,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
}

/// Check if viewing key clipboard should be cleared
pub fn get_ivk_clipboard_remaining() -> Result<Option<u64>> {
    service::get_ivk_clipboard_remaining()
}

/// Get build information for verification
pub fn get_build_info() -> Result<BuildInfo> {
    diagnostics::get_build_info()
}

/// Get sync logs for diagnostics
pub fn get_sync_logs(
    wallet_id: WalletId,
    limit: Option<u32>,
) -> Result<Vec<crate::models::SyncLogEntryFfi>> {
    diagnostics::get_sync_logs(wallet_id, limit)
}

/// Get checkpoint details at specific height
pub fn get_checkpoint_details(_wallet_id: WalletId, height: u32) -> Result<Option<CheckpointInfo>> {
    diagnostics::get_checkpoint_details(_wallet_id, height)
}

/// Test connection to a lightwalletd endpoint
pub async fn test_node(
    url: String,
    tls_pin: Option<String>,
) -> Result<crate::models::NodeTestResult> {
    tunnel::test_node(url, tls_pin).await
}
#[cfg(test)]
mod convert_from_service_tests;
