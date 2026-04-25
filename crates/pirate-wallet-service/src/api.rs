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
use anyhow::{anyhow, Result};
use directories::ProjectDirs;
use hex;
use orchard::note_encryption::OrchardDomain;
use parking_lot::RwLock;
use pirate_core::keys::{
    orchard_extsk_hrp_for_network, ExtendedFullViewingKey, ExtendedSpendingKey,
    OrchardExtendedFullViewingKey, OrchardExtendedSpendingKey, OrchardPaymentAddress,
    PaymentAddress,
};
use pirate_core::transaction::PirateNetwork;
use pirate_core::wallet::Wallet;
use pirate_core::{
    inspect_mnemonic as inspect_mnemonic_core, mnemonic::canonicalize_mnemonic,
    mnemonic::convert_mnemonic_language as convert_mnemonic_language_core, MnemonicInspection,
    MnemonicLanguage,
};
use pirate_params::{Network, NetworkType};
use pirate_storage_sqlite::{
    address_book::ColorTag as DbColorTag,
    passphrase_store, platform_keystore,
    security::{generate_salt, AppPassphrase, EncryptionAlgorithm, MasterKey, SealedKey},
    Account, AccountKey, AddressType, Database, EncryptionKey, KeyScope, KeyType, KeystoreResult,
    Repository, ScanQueueStorage, SpendabilityStateStorage, WalletSecret,
};
use pirate_sync_lightd::client::{LightClient, RetryConfig};
use pirate_sync_lightd::SyncEngine;
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;
use zcash_note_encryption::try_output_recovery_with_ovk;
use zcash_primitives::consensus::{BlockHeight, BranchId};
use zcash_primitives::merkle_tree::{read_commitment_tree, read_frontier_v0, read_frontier_v1};
use zcash_primitives::sapling::keys::OutgoingViewingKey as SaplingOutgoingViewingKey;
use zcash_primitives::sapling::note_encryption::try_sapling_output_recovery;
use zcash_primitives::transaction::Transaction;

pub(crate) mod address_book;
pub(crate) mod addresses;
pub(crate) mod background_sync;
pub(crate) mod diagnostics;
pub(crate) mod encrypted_db;
pub(crate) mod endpoint;
pub(crate) mod key_management;
pub(crate) mod panic_duress;
pub(crate) mod payment_disclosure;
pub(crate) mod provisioning;
pub(crate) mod qortal_p2sh;
pub(crate) mod seed_export;
pub(crate) mod sync_control;
pub(crate) mod tunnel;
pub(crate) mod tx_flow;
pub(crate) mod wallet_registry;

pub use self::diagnostics::CheckpointInfo;
pub use self::endpoint::{
    LightdEndpoint, DEFAULT_LIGHTD_HOST, DEFAULT_LIGHTD_PORT, DEFAULT_LIGHTD_USE_TLS,
};
use self::panic_duress::{ensure_not_decoy, is_decoy_mode_active};
pub use self::payment_disclosure::{
    export_orchard_payment_disclosure, export_payment_disclosures,
    export_sapling_payment_disclosure, verify_payment_disclosure,
};
pub use self::qortal_p2sh::{QortalP2shRedeemRequest, QortalP2shSendRequest};
pub use self::seed_export::SeedExportWarnings;
use self::wallet_registry::{
    auto_consolidation_enabled, ensure_wallet_registry_loaded, get_wallet_meta,
    load_wallet_registry_activity, load_wallet_registry_state, persist_wallet_meta,
    set_active_wallet_registry, touch_wallet_last_synced, touch_wallet_last_used,
};
use encrypted_db::{
    app_passphrase, get_registry_setting, open_wallet_db_for, open_wallet_db_with_passphrase,
    open_wallet_registry, set_registry_setting, wallet_db_key_path, wallet_db_keys,
    wallet_db_path_for, wallet_db_salt_path, wallet_registry_key_path, wallet_registry_path,
    wallet_registry_salt_path,
};
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

#[derive(Default)]
struct WalletDbCacheState {
    epoch: u64,
    entries: HashMap<String, Rc<Database>>,
}

thread_local! {
    // Keep one opened Database handle per wallet per thread.
    // Entries are tied to a global cache epoch so auth and registry changes
    // invalidate stale handles across all threads on next access.
    static WALLET_DB_CACHE: RefCell<WalletDbCacheState> = RefCell::new(WalletDbCacheState::default());
}

static REGISTRY_LOADED: AtomicBool = AtomicBool::new(false);
static WALLET_DB_CACHE_EPOCH: AtomicU64 = AtomicU64::new(1);
static PANIC_HOOK_ONCE: Once = Once::new();
static RUNTIME_DIAGNOSTICS_ONCE: Once = Once::new();
static RUNTIME_DIAGNOSTICS_STOP: AtomicBool = AtomicBool::new(false);
static RUNTIME_LAST_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
static RUNTIME_LAST_FD_PRESSURE_LOG_MS: AtomicU64 = AtomicU64::new(0);
const REGISTRY_APP_PASSPHRASE_KEY: &str = "app_passphrase_hash";
const REGISTRY_TUNNEL_MODE_KEY: &str = "tunnel_mode";
const REGISTRY_TUNNEL_SOCKS5_URL_KEY: &str = "tunnel_socks5_url";
const SPENDABILITY_REASON_ERR_RESCAN_REQUIRED: &str = "ERR_RESCAN_REQUIRED";
const SPENDABILITY_REASON_ERR_SYNC_FINALIZING: &str = "ERR_SYNC_FINALIZING";
const SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED: &str = "ERR_WITNESS_REPAIR_QUEUED";
const RUNTIME_MARKER_FILE: &str = "runtime_session.marker";

fn recover_outgoing_memo_from_raw_tx(
    raw_tx_bytes: &[u8],
    tx_height: Option<u32>,
    sapling_ovks: &[SaplingOutgoingViewingKey],
    orchard_ovks: &[orchard::keys::OutgoingViewingKey],
) -> Option<Vec<u8>> {
    if sapling_ovks.is_empty() && orchard_ovks.is_empty() {
        return None;
    }

    let tx = Transaction::read(raw_tx_bytes, BranchId::Nu5)
        .or_else(|_| Transaction::read(raw_tx_bytes, BranchId::Canopy))
        .ok()?;
    let block_height = BlockHeight::from_u32(tx_height.unwrap_or(0));

    if let Some(bundle) = tx.sapling_bundle() {
        for ovk in sapling_ovks {
            for output in bundle.shielded_outputs() {
                if let Some((_note, _address, memo)) = try_sapling_output_recovery(
                    &PirateNetwork::default(),
                    block_height,
                    ovk,
                    output,
                ) {
                    if !memo.as_array().iter().all(|b| *b == 0) {
                        return Some(memo.as_array().to_vec());
                    }
                }
            }
        }
    }

    if let Some(bundle) = tx.orchard_bundle() {
        for ovk in orchard_ovks {
            for action in bundle.actions() {
                let domain = OrchardDomain::for_action(action);
                if let Some((_note, _address, memo)) = try_output_recovery_with_ovk(
                    &domain,
                    ovk,
                    action,
                    action.cv_net(),
                    &action.encrypted_note().out_ciphertext,
                ) {
                    if !memo.iter().all(|b| *b == 0) {
                        return Some(memo.to_vec());
                    }
                }
            }
        }
    }

    None
}

fn collect_tx_recovery_context(wallet_id: &WalletId, txid: &str) -> Result<TxRecoveryContext> {
    let (db, repo) = open_wallet_db_for(wallet_id)?;
    let secret = repo
        .get_wallet_secret(wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    let parsed_txid = hex::decode(txid).map_err(|e| anyhow!("Invalid txid hex: {}", e))?;
    if parsed_txid.len() != 32 {
        return Err(anyhow!(
            "Invalid txid length: {} (expected 32 bytes)",
            parsed_txid.len()
        ));
    }

    let mut reversed_txid = parsed_txid.clone();
    reversed_txid.reverse();

    let mut tx_hash_candidates: Vec<[u8; 32]> = Vec::new();
    let mut push_tx_hash_candidate = |bytes: &[u8]| {
        if bytes.len() != 32 {
            return;
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        if !tx_hash_candidates.contains(&arr) {
            tx_hash_candidates.push(arr);
        }
    };
    push_tx_hash_candidate(&parsed_txid);
    push_tx_hash_candidate(&reversed_txid);

    let mut sapling_ovk_candidates: Vec<SaplingOutgoingViewingKey> = Vec::new();
    let mut seen_sapling_ovks: HashSet<[u8; 32]> = HashSet::new();
    let mut push_sapling_ovk = |ovk: SaplingOutgoingViewingKey| {
        if seen_sapling_ovks.insert(ovk.0) {
            sapling_ovk_candidates.push(ovk);
        }
    };

    let mut orchard_ovk_candidates: Vec<orchard::keys::OutgoingViewingKey> = Vec::new();
    let mut push_orchard_ovk = |ovk: orchard::keys::OutgoingViewingKey| {
        orchard_ovk_candidates.push(ovk);
    };

    if !secret.extsk.is_empty() {
        if let Ok(extsk) = ExtendedSpendingKey::from_bytes(&secret.extsk) {
            push_sapling_ovk(extsk.to_extended_fvk().outgoing_viewing_key());
        }
    } else if let Some(ref dfvk_bytes) = secret.dfvk {
        if let Some(dfvk) = ExtendedFullViewingKey::from_bytes(dfvk_bytes) {
            push_sapling_ovk(dfvk.outgoing_viewing_key());
        }
    }

    if let Some(ref orchard_extsk) = secret.orchard_extsk {
        if let Ok(extsk) = OrchardExtendedSpendingKey::from_bytes(orchard_extsk) {
            push_orchard_ovk(extsk.to_extended_fvk().to_ovk());
        }
    } else if let Some(ref orchard_ivk) = secret.orchard_ivk {
        if orchard_ivk.len() == 137 {
            if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bytes(orchard_ivk) {
                push_orchard_ovk(fvk.to_ovk());
            }
        }
    }

    for key in repo.get_account_keys(secret.account_id)? {
        if let Some(ref extsk_bytes) = key.sapling_extsk {
            if let Ok(extsk) = ExtendedSpendingKey::from_bytes(extsk_bytes) {
                push_sapling_ovk(extsk.to_extended_fvk().outgoing_viewing_key());
            }
        } else if let Some(ref dfvk_bytes) = key.sapling_dfvk {
            if let Some(dfvk) = ExtendedFullViewingKey::from_bytes(dfvk_bytes) {
                push_sapling_ovk(dfvk.outgoing_viewing_key());
            }
        }

        if let Some(ref extsk_bytes) = key.orchard_extsk {
            if let Ok(extsk) = OrchardExtendedSpendingKey::from_bytes(extsk_bytes) {
                push_orchard_ovk(extsk.to_extended_fvk().to_ovk());
            }
        } else if let Some(ref fvk_bytes) = key.orchard_fvk {
            if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bytes(fvk_bytes) {
                push_orchard_ovk(fvk.to_ovk());
            }
        }
    }

    let notes_direct = repo.get_notes_by_txid(secret.account_id, &parsed_txid)?;
    let notes = if notes_direct.is_empty() {
        repo.get_notes_by_txid(secret.account_id, &reversed_txid)?
    } else {
        notes_direct
    };
    let mut tx_height_hint = notes
        .iter()
        .map(|note| note.height)
        .filter(|height| *height > 0)
        .max()
        .and_then(|height| u32::try_from(height).ok());

    if tx_height_hint.is_none() {
        for candidate in [hex::encode(&parsed_txid), hex::encode(&reversed_txid)] {
            let mut stmt = db.conn().prepare(
                "SELECT height FROM transactions WHERE txid = ?1 AND height > 0 ORDER BY height DESC LIMIT 1",
            )?;
            let mut rows = stmt.query(params![candidate])?;
            if let Some(row) = rows.next()? {
                let height: i64 = row.get(0)?;
                if let Ok(parsed_height) = u32::try_from(height) {
                    tx_height_hint = Some(parsed_height);
                    break;
                }
            }
        }
    }

    Ok((
        get_lightd_endpoint_config(wallet_id.clone())?,
        tx_hash_candidates,
        sapling_ovk_candidates,
        orchard_ovk_candidates,
        tx_height_hint,
    ))
}

type TxRecoveryContext = (
    endpoint::LightdEndpoint,
    Vec<[u8; 32]>,
    Vec<SaplingOutgoingViewingKey>,
    Vec<orchard::keys::OutgoingViewingKey>,
    Option<u32>,
);

fn format_branch_id_hex(branch_id: BranchId) -> String {
    format!("{:08x}", u32::from(branch_id))
}

fn parse_branch_id_hex(hex_value: &str) -> Option<BranchId> {
    let trimmed = hex_value.trim();
    let normalized = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    u32::from_str_radix(normalized, 16)
        .ok()
        .and_then(|value| BranchId::try_from(value).ok())
}

fn debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        ProjectDirs::from("com", "Pirate", "PirateWallet")
            .map(|dirs| dirs.data_local_dir().join("logs").join("debug.log"))
            .unwrap_or_else(|| {
                env::current_dir()
                    .map(|dir| dir.join(".cursor").join("debug.log"))
                    .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
            })
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
}

fn unix_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn truncate_for_log(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (count, ch) in input.chars().enumerate() {
        if count >= max_chars {
            out.push_str("...<truncated>");
            return out;
        }
        out.push(ch);
    }
    out
}

fn runtime_marker_path() -> PathBuf {
    let log_path = debug_log_path();
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
    let path = runtime_marker_path();
    let mut marker = read_runtime_marker(&path);
    mutator(&mut marker);
    write_runtime_marker(&path, &marker);
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

fn mark_runtime_clean_shutdown(reason: &str) {
    RUNTIME_DIAGNOSTICS_STOP.store(true, Ordering::SeqCst);
    let ts = unix_timestamp_millis();
    RUNTIME_LAST_HEARTBEAT_MS.store(ts, Ordering::SeqCst);
    update_runtime_marker(|m| {
        m.insert("pid".to_string(), std::process::id().to_string());
        m.insert("last_heartbeat_ms".to_string(), ts.to_string());
        m.insert("clean_shutdown".to_string(), "1".to_string());
        m.insert("reason".to_string(), reason.to_string());
        if let Some(fd_count) = current_linux_fd_count() {
            m.insert("fd_count".to_string(), fd_count.to_string());
        }
    });
    write_runtime_debug_event(
        "log_runtime_shutdown_marked",
        "runtime marked clean shutdown",
        &format!(
            r#"{{"pid":{},"reason":"{}","timestamp":{}}}"#,
            std::process::id(),
            escape_json(reason),
            ts
        ),
    );
}

fn install_debug_panic_hook() {
    PANIC_HOOK_ONCE.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let payload = truncate_for_log(&panic_info.to_string(), 4_096);
                let payload = payload.replace('\"', "\\\"");
                let thread_name = std::thread::current()
                    .name()
                    .unwrap_or("unnamed")
                    .replace('\"', "\\\"");
                let panic_location = panic_info
                    .location()
                    .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
                    .unwrap_or_else(|| "unknown".to_string())
                    .replace('\"', "\\\"");
                let backtrace =
                    truncate_for_log(&format!("{:?}", std::backtrace::Backtrace::force_capture()), 8_192)
                        .replace('\"', "\\\"");
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rust_panic","timestamp":{},"location":"api.rs","message":"unhandled rust panic","data":{{"panic":"{}","thread":"{}","panic_location":"{}","backtrace":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, payload, thread_name, panic_location, backtrace
                );
            });
            update_runtime_marker(|m| {
                m.insert("pid".to_string(), std::process::id().to_string());
                m.insert("last_heartbeat_ms".to_string(), unix_timestamp_millis().to_string());
                m.insert("clean_shutdown".to_string(), "0".to_string());
                m.insert("reason".to_string(), "panic".to_string());
            });
            default_hook(panic_info);
        }));
    });
}

// ============================================================================
// Wallet Lifecycle
// ============================================================================

fn log_orchard_address_samples(wallet_id: &WalletId) {
    let (_db, repo) = match open_wallet_db_for(wallet_id) {
        Ok(result) => result,
        Err(_) => return,
    };
    let secret = match repo.get_wallet_secret(wallet_id) {
        Ok(Some(secret)) => secret,
        _ => return,
    };
    let orchard_extsk_bytes = match secret.orchard_extsk.as_ref() {
        Some(bytes) => bytes,
        None => return,
    };
    let orchard_extsk = match OrchardExtendedSpendingKey::from_bytes(orchard_extsk_bytes) {
        Ok(key) => key,
        Err(_) => return,
    };
    let orchard_fvk = orchard_extsk.to_extended_fvk();

    pirate_core::debug_log::with_locked_file(|file| {
        let ts = chrono::Utc::now().timestamp_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_orchard_address_samples","timestamp":{},"location":"api.rs:log_orchard_address_samples","message":"orchard address sample header","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
            ts, wallet_id
        );
    });

    for index in 0u32..10u32 {
        let address = orchard_fvk.address_at(index);
        let addr_mainnet = address
            .encode_for_network(NetworkType::Mainnet)
            .unwrap_or_default();
        let addr_testnet = address
            .encode_for_network(NetworkType::Testnet)
            .unwrap_or_default();
        let addr_regtest = address
            .encode_for_network(NetworkType::Regtest)
            .unwrap_or_default();

        pirate_core::debug_log::with_locked_file(|file| {
            let ts = chrono::Utc::now().timestamp_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_orchard_address_sample","timestamp":{},"location":"api.rs:log_orchard_address_samples","message":"orchard address sample","data":{{"wallet_id":"{}","index":{},"mainnet":"{}","testnet":"{}","regtest":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, index, addr_mainnet, addr_testnet, addr_regtest
            );
        });
    }
}

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
    provisioning::create_wallet(name, _entropy_len, birthday_opt, mnemonic_language)
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
    provisioning::restore_wallet(name, mnemonic, birthday_opt, mnemonic_language)
}

/// Check if wallet registry database file exists (without opening it)
///
/// This allows checking if wallets exist before the database is created or opened.
pub fn wallet_registry_exists() -> Result<bool> {
    wallet_registry::wallet_registry_exists()
}

/// List all wallets
///
/// Returns empty list if database can't be opened (e.g., passphrase not set)
/// NOTE: This will CREATE the database file if it doesn't exist (via open_wallet_registry)
pub fn list_wallets() -> Result<Vec<WalletMeta>> {
    wallet_registry::list_wallets()
}

/// Switch active wallet
pub fn switch_wallet(wallet_id: WalletId) -> Result<()> {
    wallet_registry::switch_wallet(wallet_id)
}

async fn run_sync_engine_task<F, T>(sync: Arc<tokio::sync::Mutex<SyncEngine>>, task: F) -> Result<T>
where
    F: for<'a> FnOnce(&'a mut SyncEngine) -> Pin<Box<dyn Future<Output = Result<T>> + 'a>>
        + Send
        + 'static,
    T: Send + 'static,
{
    let run_task = move || -> Result<T> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| anyhow!("Failed to build sync runtime: {}", e))?;
        runtime.block_on(async move {
            let mut engine = sync.lock().await;
            let cancel = engine.cancel_flag();
            tokio::select! {
                _ = cancel.cancelled() => Err(anyhow!("Sync cancelled")),
                result = task(&mut engine) => result,
            }
        })
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let join = handle.spawn_blocking(run_task);
        join.await
            .map_err(|e| anyhow!("Sync task join error: {}", e))?
    } else {
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_task());
        });
        rx.await
            .map_err(|e| anyhow!("Sync task thread join error: {}", e))?
    }
}

async fn run_on_runtime<F, Fut, T>(task: F) -> Result<T>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = (|| -> Result<T> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow!("Failed to build runtime: {}", e))?;
            runtime.block_on(task())
        })();
        let _ = tx.send(result);
    });

    rx.await
        .map_err(|e| anyhow!("Runtime task join error: {}", e))?
}

fn run_on_runtime_blocking<F, Fut, T>(task: F) -> Result<T>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T>> + 'static,
    T: Send + 'static,
{
    futures::executor::block_on(run_on_runtime(task))
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
    encrypted_db::set_app_passphrase(passphrase)
}

/// Check if app passphrase is configured
pub fn has_app_passphrase() -> Result<bool> {
    encrypted_db::has_app_passphrase()
}

/// Verify app passphrase by attempting to open the database with it
pub fn verify_app_passphrase(passphrase: String) -> Result<bool> {
    encrypted_db::verify_app_passphrase(passphrase)
}

/// Unlock app with passphrase (caches passphrase in memory for wallet access)
/// This allows wallets to be decrypted using the passphrase
pub fn unlock_app(passphrase: String) -> Result<()> {
    encrypted_db::unlock_app(passphrase)
}

/// Change app passphrase and re-encrypt all wallet data with the new keys.
pub fn change_app_passphrase(current_passphrase: String, new_passphrase: String) -> Result<()> {
    encrypted_db::change_app_passphrase(current_passphrase, new_passphrase)
}

/// Change passphrase using the cached passphrase from the current session.
pub fn change_app_passphrase_with_cached(new_passphrase: String) -> Result<()> {
    encrypted_db::change_app_passphrase_with_cached(new_passphrase)
}

/// Reseal registry + wallet DB keys using current platform keystore mode.
///
/// This is used when biometrics are enabled/disabled to rewrap the DB keys
/// under the appropriate keystore policy without changing the passphrase.
pub fn reseal_db_keys_for_biometrics() -> Result<()> {
    encrypted_db::reseal_db_keys_for_biometrics()
}

/// Get auto-consolidation setting for a wallet.
pub fn get_auto_consolidation_enabled(wallet_id: WalletId) -> Result<bool> {
    wallet_registry::get_auto_consolidation_enabled(wallet_id)
}

/// Enable or disable auto-consolidation for a wallet.
pub fn set_auto_consolidation_enabled(wallet_id: WalletId, enabled: bool) -> Result<()> {
    wallet_registry::set_auto_consolidation_enabled(wallet_id, enabled)
}

/// Get the note count threshold that triggers auto-consolidation prompts.
pub fn get_auto_consolidation_threshold() -> Result<u32> {
    Ok(AUTO_CONSOLIDATION_THRESHOLD as u32)
}

/// Count selectable notes eligible for auto-consolidation.
pub fn get_auto_consolidation_candidate_count(wallet_id: WalletId) -> Result<u32> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let selectable_notes =
        repo.get_unspent_selectable_notes_filtered(secret.account_id, None, None)?;
    let count = selectable_notes
        .iter()
        .filter(|note| note.auto_consolidation_eligible)
        .count();
    Ok(count as u32)
}

/// Return deterministic spendability status for the wallet.
pub fn get_spendability_status(wallet_id: WalletId) -> Result<SpendabilityStatus> {
    sync_control::get_spendability_status(wallet_id)
}

fn ensure_primary_account_key(
    repo: &Repository,
    wallet_id: &str,
    secret: &WalletSecret,
) -> Result<i64> {
    let keys = repo.get_account_keys(secret.account_id)?;
    let meta = get_wallet_meta(wallet_id)?;
    if let Some(existing) = keys
        .iter()
        .find(|k| k.key_type == KeyType::Seed && k.key_scope == KeyScope::Account)
    {
        if let Some(id) = existing.id {
            if existing.birthday_height != meta.birthday_height as i64 {
                let mut updated = existing.clone();
                updated.birthday_height = meta.birthday_height as i64;
                let encrypted = repo.encrypt_account_key_fields(&updated)?;
                let _ = repo.upsert_account_key(&encrypted);
            }
            let _ = repo.backfill_address_key_id(secret.account_id, id);
            let _ = repo.backfill_note_key_id(id);
            return Ok(id);
        }
    }

    let sapling_extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)?;
    let dfvk_bytes = match secret.dfvk.as_ref() {
        Some(bytes) => Some(bytes.clone()),
        None => Some(sapling_extsk.to_extended_fvk().to_bytes()),
    };

    let orchard_fvk_bytes = match secret.orchard_extsk.as_ref() {
        Some(bytes) => {
            let extsk = OrchardExtendedSpendingKey::from_bytes(bytes)
                .map_err(|e| anyhow!("Invalid Orchard spending key bytes: {}", e))?;
            Some(extsk.to_extended_fvk().to_bytes())
        }
        None => None,
    };

    let key = AccountKey {
        id: None,
        account_id: secret.account_id,
        key_type: KeyType::Seed,
        key_scope: KeyScope::Account,
        label: Some("Seed".to_string()),
        birthday_height: meta.birthday_height as i64,
        created_at: chrono::Utc::now().timestamp(),
        spendable: true,
        sapling_extsk: Some(secret.extsk.clone()),
        sapling_dfvk: dfvk_bytes,
        orchard_extsk: secret.orchard_extsk.clone(),
        orchard_fvk: orchard_fvk_bytes,
        encrypted_mnemonic: secret.encrypted_mnemonic.clone(),
    };

    let encrypted_key = repo.encrypt_account_key_fields(&key)?;
    let key_id = repo
        .upsert_account_key(&encrypted_key)
        .map_err(|e| anyhow!(e.to_string()))?;
    let _ = repo.backfill_address_key_id(secret.account_id, key_id);
    let _ = repo.backfill_note_key_id(key_id);
    Ok(key_id)
}

/// Get active wallet ID
pub fn get_active_wallet() -> Result<Option<WalletId>> {
    wallet_registry::get_active_wallet()
}

/// Rename wallet
pub fn rename_wallet(wallet_id: WalletId, new_name: String) -> Result<()> {
    wallet_registry::rename_wallet(wallet_id, new_name)
}

/// Update wallet birthday height
pub fn set_wallet_birthday_height(wallet_id: WalletId, birthday_height: u32) -> Result<()> {
    wallet_registry::set_wallet_birthday_height(wallet_id, birthday_height)
}

/// Delete wallet and its local database
pub fn delete_wallet(wallet_id: WalletId) -> Result<()> {
    wallet_registry::delete_wallet(wallet_id)
}

// ============================================================================
// Addresses
// ============================================================================

/// Helper: Determine if Orchard addresses should be generated based on network and height
fn orchard_activation_override(wallet_id: &WalletId) -> Result<Option<u32>> {
    let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;
    Ok(endpoint::orchard_activation_override_height(&endpoint))
}

fn wallet_network_type(wallet_id: &WalletId) -> Result<NetworkType> {
    let wallet = get_wallet_meta(wallet_id)?;
    let network_type = match wallet.network_type.as_deref().unwrap_or("mainnet") {
        "testnet" => NetworkType::Testnet,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Mainnet,
    };
    Ok(network_type)
}

fn address_prefix_network_type(wallet_id: &WalletId) -> Result<NetworkType> {
    let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;
    let default_network = wallet_network_type(wallet_id)?;
    Ok(endpoint::address_prefix_network_type_for_endpoint(
        &endpoint,
        default_network,
    ))
}

fn should_generate_orchard(wallet_id: &WalletId) -> Result<bool> {
    let wallet = get_wallet_meta(wallet_id)?;
    let network = Network::from_type(wallet_network_type(wallet_id)?);

    // Get current block height from sync state
    let (_db, _repo) = open_wallet_db_for(wallet_id)?;
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&_db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height as u32;
    let effective_height = if current_height == 0 {
        wallet.birthday_height
    } else {
        current_height
    };

    // Check if Orchard is activated at current height
    if let Some(override_height) = orchard_activation_override(wallet_id)? {
        return Ok(effective_height >= override_height);
    }

    Ok(network.is_orchard_active(effective_height))
}

/// Get current receive address for wallet
///
/// Returns the current diversified Sapling address from storage.
/// If no address exists, generates and stores the first address (index 0).
/// Call `next_receive_address` to rotate to a new unlinkable address.
pub fn current_receive_address(wallet_id: WalletId) -> Result<String> {
    addresses::current_receive_address(wallet_id)
}

/// Generate next receive address (diversifier rotation)
///
/// Increments the diversifier index to generate a fresh, unlinkable address.
/// Address type (Sapling or Orchard) is determined by network and current block height.
/// Previous addresses remain valid for receiving funds.
pub fn next_receive_address(wallet_id: WalletId) -> Result<String> {
    addresses::next_receive_address(wallet_id)
}

/// Label an address for address book
pub fn label_address(wallet_id: WalletId, addr: String, label: String) -> Result<()> {
    ensure_not_decoy("Label address")?;
    // Open encrypted wallet DB
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Update address label (empty string means remove label)
    let label_opt = if label.is_empty() {
        None
    } else {
        Some(label.clone())
    };

    repo.update_address_label(secret.account_id, &addr, label_opt)?;

    tracing::info!("Labeled address {} as '{}'", addr, label);
    Ok(())
}

/// Set color tag for a wallet address
pub fn set_address_color_tag(
    wallet_id: WalletId,
    addr: String,
    color_tag: AddressBookColorTag,
) -> Result<()> {
    ensure_not_decoy("Update address color")?;
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;

    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let db_tag = address_book_color_from_ffi(color_tag);
    repo.update_address_color_tag(secret.account_id, &addr, db_tag)?;

    tracing::info!("Updated address color tag for {}", addr);
    Ok(())
}

/// Get all addresses for wallet with labels
pub fn list_addresses(wallet_id: WalletId) -> Result<Vec<AddressInfo>> {
    addresses::list_addresses(wallet_id)
}

/// Get per-address balances for a wallet (optionally filtered by key group).
pub fn list_address_balances(
    wallet_id: WalletId,
    key_id: Option<i64>,
) -> Result<Vec<AddressBalanceInfo>> {
    addresses::list_address_balances(wallet_id, key_id)
}

fn address_matches_expected_network_prefix(
    address: &str,
    address_type: AddressType,
    network_type: NetworkType,
) -> bool {
    match (address_type, network_type) {
        (AddressType::Sapling, NetworkType::Mainnet) => address.starts_with("zs1"),
        (AddressType::Sapling, NetworkType::Testnet) => address.starts_with("ztestsapling1"),
        (AddressType::Sapling, NetworkType::Regtest) => address.starts_with("zregtestsapling1"),
        (AddressType::Orchard, NetworkType::Mainnet) => address.starts_with("pirate1"),
        (AddressType::Orchard, NetworkType::Testnet) => address.starts_with("pirate-test1"),
        (AddressType::Orchard, NetworkType::Regtest) => address.starts_with("pirate-regtest1"),
    }
}

// ============================================================================
// Address Book
// ============================================================================

pub(super) fn address_book_color_from_ffi(tag: AddressBookColorTag) -> DbColorTag {
    match tag {
        AddressBookColorTag::None => DbColorTag::None,
        AddressBookColorTag::Red => DbColorTag::Red,
        AddressBookColorTag::Orange => DbColorTag::Orange,
        AddressBookColorTag::Yellow => DbColorTag::Yellow,
        AddressBookColorTag::Green => DbColorTag::Green,
        AddressBookColorTag::Blue => DbColorTag::Blue,
        AddressBookColorTag::Purple => DbColorTag::Purple,
        AddressBookColorTag::Pink => DbColorTag::Pink,
        AddressBookColorTag::Gray => DbColorTag::Gray,
    }
}

pub(super) fn address_book_color_to_ffi(tag: DbColorTag) -> AddressBookColorTag {
    match tag {
        DbColorTag::None => AddressBookColorTag::None,
        DbColorTag::Red => AddressBookColorTag::Red,
        DbColorTag::Orange => AddressBookColorTag::Orange,
        DbColorTag::Yellow => AddressBookColorTag::Yellow,
        DbColorTag::Green => AddressBookColorTag::Green,
        DbColorTag::Blue => AddressBookColorTag::Blue,
        DbColorTag::Purple => AddressBookColorTag::Purple,
        DbColorTag::Pink => AddressBookColorTag::Pink,
        DbColorTag::Gray => AddressBookColorTag::Gray,
    }
}

/// List address book entries for a wallet
pub fn list_address_book(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    address_book::list_address_book(wallet_id)
}

/// Add an address book entry
pub fn add_address_book_entry(
    wallet_id: WalletId,
    address: String,
    label: String,
    notes: Option<String>,
    color_tag: AddressBookColorTag,
) -> Result<AddressBookEntryFfi> {
    address_book::add_address_book_entry(wallet_id, address, label, notes, color_tag)
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
    address_book::update_address_book_entry(wallet_id, id, label, notes, color_tag, is_favorite)
}

/// Delete an address book entry
pub fn delete_address_book_entry(wallet_id: WalletId, id: i64) -> Result<()> {
    address_book::delete_address_book_entry(wallet_id, id)
}

/// Toggle favorite status for an entry
pub fn toggle_address_book_favorite(wallet_id: WalletId, id: i64) -> Result<bool> {
    address_book::toggle_address_book_favorite(wallet_id, id)
}

/// Mark an address as used
pub fn mark_address_used(wallet_id: WalletId, address: String) -> Result<()> {
    address_book::mark_address_used(wallet_id, address)
}

/// Get label for an address
pub fn get_label_for_address(wallet_id: WalletId, address: String) -> Result<Option<String>> {
    address_book::get_label_for_address(wallet_id, address)
}

/// Check if an address exists in the book
pub fn address_exists_in_book(wallet_id: WalletId, address: String) -> Result<bool> {
    address_book::address_exists_in_book(wallet_id, address)
}

/// Count address book entries
pub fn get_address_book_count(wallet_id: WalletId) -> Result<u32> {
    address_book::get_address_book_count(wallet_id)
}

/// Get entry by ID
pub fn get_address_book_entry(wallet_id: WalletId, id: i64) -> Result<Option<AddressBookEntryFfi>> {
    address_book::get_address_book_entry(wallet_id, id)
}

/// Get entry by address
pub fn get_address_book_entry_by_address(
    wallet_id: WalletId,
    address: String,
) -> Result<Option<AddressBookEntryFfi>> {
    address_book::get_address_book_entry_by_address(wallet_id, address)
}

/// Search entries by query
pub fn search_address_book(wallet_id: WalletId, query: String) -> Result<Vec<AddressBookEntryFfi>> {
    address_book::search_address_book(wallet_id, query)
}

/// List favorites
pub fn get_address_book_favorites(wallet_id: WalletId) -> Result<Vec<AddressBookEntryFfi>> {
    address_book::get_address_book_favorites(wallet_id)
}

/// List recently used addresses
pub fn get_recently_used_addresses(
    wallet_id: WalletId,
    limit: u32,
) -> Result<Vec<AddressBookEntryFfi>> {
    address_book::get_recently_used_addresses(wallet_id, limit)
}

/// Returns true when the address is a valid shielded address supported by this wallet.
pub fn is_valid_shielded_addr(address: String) -> Result<bool> {
    Ok(validate_address(address)?.is_valid)
}

/// Validate a shielded recipient address and return its supported type or an error reason.
pub fn validate_address(address: String) -> Result<AddressValidation> {
    if PaymentAddress::decode_any_network(&address).is_ok() {
        return Ok(AddressValidation {
            is_valid: true,
            address_type: Some(ShieldedAddressType::Sapling),
            reason: None,
        });
    }

    if OrchardPaymentAddress::decode_any_network(&address).is_ok() {
        return Ok(AddressValidation {
            is_valid: true,
            address_type: Some(ShieldedAddressType::Orchard),
            reason: None,
        });
    }

    Ok(AddressValidation {
        is_valid: false,
        address_type: None,
        reason: Some(
            "Invalid shielded address. Supported formats start with \"zs1\" or \"pirate1\"."
                .to_string(),
        ),
    })
}

// ============================================================================
// Watch-Only
// ============================================================================

/// Export Sapling viewing key from full wallet.
///
/// Uses the zxviews... Bech32 format for watch-only wallets.
pub fn export_sapling_viewing_key(wallet_id: WalletId) -> Result<String> {
    key_management::export_sapling_viewing_key(wallet_id)
}

/// Export Orchard Extended Full Viewing Key as Bech32 (for watch-only wallets)
///
/// Returns Bech32-encoded string with the network-specific HRP.
/// Uses the standard Orchard viewing key export format.
/// Use export_sapling_viewing_key() for Sapling viewing keys (zxviews... format).
pub fn export_orchard_viewing_key(wallet_id: WalletId) -> Result<String> {
    key_management::export_orchard_viewing_key(wallet_id)
}

/// Import viewing keys (watch-only wallet).
///
/// Supports Sapling viewing keys (zxviews...) and Orchard extended viewing keys (bech32).
/// If both are provided, creates a watch-only wallet that can view both Sapling and Orchard transactions.
pub fn import_viewing_wallet(
    name: String,
    sapling_viewing_key: Option<String>,
    orchard_viewing_key: Option<String>,
    birthday: u32,
) -> Result<WalletId> {
    provisioning::import_viewing_wallet(name, sapling_viewing_key, orchard_viewing_key, birthday)
}

// ============================================================================
// Key Management
// ============================================================================

/// List key groups for the active wallet account.
pub fn list_key_groups(wallet_id: WalletId) -> Result<Vec<KeyGroupInfo>> {
    key_management::list_key_groups(wallet_id)
}

/// Export viewing/spending keys for a specific key group.
pub fn export_key_group_keys(wallet_id: WalletId, key_id: i64) -> Result<KeyExportInfo> {
    key_management::export_key_group_keys(wallet_id, key_id)
}

/// List addresses for a specific key group.
pub fn list_addresses_for_key(wallet_id: WalletId, key_id: i64) -> Result<Vec<KeyAddressInfo>> {
    key_management::list_addresses_for_key(wallet_id, key_id)
}

/// Generate a new address for a specific key group.
pub fn generate_address_for_key(
    wallet_id: WalletId,
    key_id: i64,
    use_orchard: bool,
) -> Result<String> {
    key_management::generate_address_for_key(wallet_id, key_id, use_orchard)
}

/// Import a spending key into an existing wallet.
pub fn import_spending_key(
    wallet_id: WalletId,
    sapling_key: Option<String>,
    orchard_key: Option<String>,
    label: Option<String>,
    birthday_height: u32,
) -> Result<i64> {
    key_management::import_spending_key(wallet_id, sapling_key, orchard_key, label, birthday_height)
}

/// Export mnemonic seed through the raw advanced path.
///
/// This path is intended for advanced callers such as CLIs or external wallet
/// integrations that implement their own local authorization UX.
///
/// It does not use the app-gated seed export state machine.
///
/// Note: Only works for wallets created/restored from seed.
/// Wallets imported from private key or watch-only wallets cannot export seed.
pub fn export_seed_raw(
    wallet_id: WalletId,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<String> {
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export seed from watch-only wallet"));
    }

    // Load wallet secret from encrypted storage
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Check if mnemonic is stored (wallet was created/restored from seed)
    let mnemonic_bytes = secret.encrypted_mnemonic.clone().ok_or_else(|| {
        anyhow!("Seed not available. This wallet was imported from private key or is watch-only.")
    })?;

    // Decrypt mnemonic (database encryption handles decryption)
    let mnemonic = String::from_utf8(mnemonic_bytes)
        .map_err(|e| anyhow!("Failed to decode mnemonic: {}", e))?;

    let original_language = wallet_secret_mnemonic_language(&secret, &mnemonic)?;
    let display_language = mnemonic_language.unwrap_or(original_language);
    let mnemonic = if display_language == original_language {
        canonicalize_mnemonic(&mnemonic, Some(original_language))?.0
    } else {
        convert_mnemonic_language_core(&mnemonic, Some(original_language), display_language)?
    };

    tracing::info!("Raw seed export completed for wallet {}", wallet_id);
    Ok(mnemonic)
}

pub(super) fn wallet_secret_mnemonic_language(
    secret: &WalletSecret,
    mnemonic: &str,
) -> Result<MnemonicLanguage> {
    if let Some(language_key) = secret.mnemonic_language.as_deref() {
        if let Some(language) = MnemonicLanguage::from_key(language_key) {
            return Ok(language);
        }
    }

    if let Some(language) = inspect_mnemonic_core(mnemonic).detected_language {
        return Ok(language);
    }

    Ok(MnemonicLanguage::English)
}

// ============================================================================
// Send (Send-to-Many with per-output memos)
// ============================================================================

use pirate_core::{
    apply_dust_policy_add_to_fee, FeeCalculator, FeePolicy, NoteSelector, SelectionStrategy,
    CHANGE_DUST_THRESHOLD, DEFAULT_FEE, MAX_FEE, MAX_MEMO_LENGTH, MIN_FEE,
};

/// Maximum number of outputs per transaction
pub const MAX_OUTPUTS_PER_TX: usize = 50;
const AUTO_CONSOLIDATION_THRESHOLD: usize = 30;
const AUTO_CONSOLIDATION_MAX_EXTRA_NOTES: usize = 20;
const SPENDABILITY_MIN_CONFIRMATIONS: u32 = 1;

/// Build transaction with note selection, fee calculation, and change.
pub fn build_tx(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    ensure_not_decoy("Build transaction")?;
    tx_flow::build_tx(wallet_id, outputs, fee_opt)
}

/// Build transaction using notes from a specific key group.
pub fn build_tx_for_key(
    wallet_id: WalletId,
    key_id: i64,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    ensure_not_decoy("Build transaction")?;
    tx_flow::build_tx_for_key(wallet_id, key_id, outputs, fee_opt)
}

/// Build transaction using selected key groups or addresses.
pub fn build_tx_filtered(
    wallet_id: WalletId,
    outputs: Vec<Output>,
    fee_opt: Option<u64>,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<PendingTx> {
    ensure_not_decoy("Build transaction")?;
    tx_flow::build_tx_filtered(
        wallet_id,
        outputs,
        fee_opt,
        key_ids_filter,
        address_ids_filter,
    )
}

/// Build a consolidation transaction for a key group.
pub fn build_consolidation_tx(
    wallet_id: WalletId,
    key_id: i64,
    target_address: String,
    fee_opt: Option<u64>,
) -> Result<PendingTx> {
    tx_flow::build_consolidation_tx(wallet_id, key_id, target_address, fee_opt)
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
    tx_flow::build_sweep_tx(
        wallet_id,
        target_address,
        fee_opt,
        key_ids_filter,
        address_ids_filter,
    )
}

/// Sign pending transaction (all spendable notes in the wallet)
pub fn sign_tx(wallet_id: WalletId, pending: PendingTx) -> Result<SignedTx> {
    ensure_not_decoy("Sign transaction")?;
    tx_flow::sign_tx(wallet_id, pending)
}

/// Sign pending transaction using notes from a specific key group
pub fn sign_tx_for_key(wallet_id: WalletId, pending: PendingTx, key_id: i64) -> Result<SignedTx> {
    ensure_not_decoy("Sign transaction")?;
    tx_flow::sign_tx_for_key(wallet_id, pending, key_id)
}

/// Sign pending transaction using selected key groups or addresses.
pub fn sign_tx_filtered(
    wallet_id: WalletId,
    pending: PendingTx,
    key_ids_filter: Option<Vec<i64>>,
    address_ids_filter: Option<Vec<i64>>,
) -> Result<SignedTx> {
    ensure_not_decoy("Sign transaction")?;
    tx_flow::sign_tx_filtered(wallet_id, pending, key_ids_filter, address_ids_filter)
}

/// Broadcast signed transaction to the network
///
/// Sends transaction via lightwalletd gRPC SendTransaction.
/// Returns TxId on success, or error with details.
pub async fn broadcast_tx(signed: SignedTx) -> Result<TxId> {
    ensure_not_decoy("Broadcast transaction")?;
    run_on_runtime(move || tx_flow::broadcast_tx(signed)).await
}

/// Estimate fee for transaction without building it
pub fn estimate_fee(num_outputs: usize, has_memo: bool, fee_policy: Option<String>) -> Result<u64> {
    let calculator = FeeCalculator::new();
    let estimated_inputs = num_outputs.div_ceil(2);

    let base_fee = calculator
        .calculate_fee(estimated_inputs, num_outputs, has_memo)
        .map_err(|e| anyhow!("Fee calculation error: {}", e))?;

    // Apply fee policy
    let policy = match fee_policy.as_deref() {
        Some("low") => FeePolicy::Low,
        Some("high") => FeePolicy::High,
        Some("standard") | None => FeePolicy::Standard,
        Some(custom) => {
            let fee: u64 = custom
                .parse()
                .map_err(|_| anyhow!("Invalid fee: {}", custom))?;
            FeePolicy::Custom(fee)
        }
    };

    let fee = policy.apply(base_fee);
    Ok(fee.clamp(MIN_FEE, MAX_FEE))
}

/// Get fee information
pub fn get_fee_info() -> Result<FeeInfo> {
    Ok(FeeInfo {
        default_fee: DEFAULT_FEE,
        min_fee: MIN_FEE,
        max_fee: MAX_FEE,
        fee_per_output: 0,
        memo_fee_multiplier: 1.0,
    })
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
    sync_control::start_sync(wallet_id, mode).await
}

/// Get sync status for a wallet with full performance metrics
pub fn sync_status(wallet_id: WalletId) -> Result<SyncStatus> {
    sync_control::sync_status(wallet_id)
}

/// Get last checkpoint info for diagnostics
pub fn get_last_checkpoint(wallet_id: WalletId) -> Result<Option<CheckpointInfo>> {
    sync_control::get_last_checkpoint(wallet_id)
}

/// Rescan wallet from specific height
pub async fn rescan(wallet_id: WalletId, from_height: u32) -> Result<()> {
    sync_control::rescan(wallet_id, from_height).await
}

/// Cancel ongoing sync for a wallet.
pub async fn cancel_sync(wallet_id: WalletId) -> Result<()> {
    sync_control::cancel_sync(wallet_id).await
}

/// Check if sync is running for a wallet
pub fn is_sync_running(wallet_id: WalletId) -> Result<bool> {
    sync_control::is_sync_running(wallet_id)
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
    pub orchard_requested: usize,
    pub orchard_updated: usize,
    pub orchard_missing: usize,
    pub orchard_errors: usize,
}

/// Set lightwalletd endpoint
pub fn set_lightd_endpoint(
    wallet_id: WalletId,
    url: String,
    tls_pin_opt: Option<String>,
) -> Result<()> {
    ensure_wallet_registry_loaded()?;
    let was_running = sync_control::is_sync_running(wallet_id.clone()).unwrap_or(false);
    let endpoint =
        endpoint::endpoint_from_url(&url, DEFAULT_LIGHTD_USE_TLS, tls_pin_opt.clone(), None)?;
    // Detect network type from endpoint (best effort).
    // Unknown endpoints keep current wallet network instead of forcing mainnet.
    let detected_network_type =
        endpoint::detect_network_from_endpoint(&endpoint.host, endpoint.port);

    let endpoint_url = endpoint.url();

    tracing::info!(
        "Set lightd endpoint for wallet {}: {} (detected network: {:?})",
        wallet_id,
        endpoint.url(),
        detected_network_type
    );

    endpoint::cache_lightd_endpoint(wallet_id.clone(), endpoint.clone());

    // Update wallet network type
    let mut wallets = WALLETS.write();
    if let Some(wallet) = wallets.iter_mut().find(|w| w.id == wallet_id) {
        let old_network_type = match wallet.network_type.as_deref().unwrap_or("mainnet") {
            "testnet" => NetworkType::Testnet,
            "regtest" => NetworkType::Regtest,
            _ => NetworkType::Mainnet,
        };
        let new_network_type = detected_network_type.unwrap_or(old_network_type);
        wallet.network_type = Some(format!("{:?}", new_network_type).to_lowercase());
        let registry_db = open_wallet_registry()?;
        persist_wallet_meta(&registry_db, wallet)?;
        tracing::info!(
            "Updated wallet {} network type to {:?}",
            wallet_id,
            new_network_type
        );

        {
            let ts = chrono::Utc::now().timestamp_millis();
            pirate_core::debug_log::with_locked_file(|file| {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_set_lightd_endpoint","timestamp":{},"location":"api.rs:set_lightd_endpoint","message":"set_lightd_endpoint","data":{{"wallet_id":"{}","endpoint":"{}","old_network":"{:?}","new_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id, endpoint_url, old_network_type, new_network_type
                );
            });
        }

        if old_network_type != new_network_type {
            if let Ok((_db, repo)) = open_wallet_db_for(&wallet_id) {
                if let Err(err) = repo.clear_chain_state() {
                    tracing::warn!(
                        "Failed to clear chain state for wallet {} after network change: {:?}",
                        wallet_id,
                        err
                    );
                }
            }
        }

        if old_network_type != new_network_type {
            if let Err(err) =
                rederive_wallet_keys_for_network(&wallet_id, old_network_type, new_network_type)
            {
                tracing::warn!(
                    "Failed to re-derive keys for wallet {}: {:?}",
                    wallet_id,
                    err
                );
            }
        } else {
            let ts = chrono::Utc::now().timestamp_millis();
            pirate_core::debug_log::with_locked_file(|file| {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rederive_skip","timestamp":{},"location":"api.rs:set_lightd_endpoint","message":"rederive skipped (same network)","data":{{"wallet_id":"{}","network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id, new_network_type
                );
            });
        }

        // Persist endpoint per wallet so it survives restarts.
        let endpoint_key = format!("lightd_endpoint_{}", wallet_id);
        let pin_key = format!("lightd_tls_pin_{}", wallet_id);
        set_registry_setting(&registry_db, &endpoint_key, Some(&endpoint_url))?;
        set_registry_setting(&registry_db, &pin_key, tls_pin_opt.as_deref())?;
    }

    if let Err(err) = run_on_runtime_blocking({
        let wallet_id = wallet_id.clone();
        move || async move { sync_control::cancel_sync_internal(wallet_id, true).await }
    }) {
        tracing::warn!(
            "Failed to cancel stale sync session after endpoint change for {}: {}",
            wallet_id,
            err
        );
    }

    sync_control::clear_wallet_sync_state(&wallet_id);

    if was_running {
        sync_control::maybe_trigger_compact_sync(wallet_id.clone());
    }

    Ok(())
}

/// Get lightwalletd endpoint
pub fn get_lightd_endpoint(wallet_id: WalletId) -> Result<String> {
    endpoint::get_lightd_endpoint(wallet_id)
}

/// Get full endpoint configuration
pub fn get_lightd_endpoint_config(wallet_id: WalletId) -> Result<LightdEndpoint> {
    endpoint::get_lightd_endpoint_config(wallet_id)
}

fn infer_key_network_type_from_addresses(
    mnemonic: &str,
    mnemonic_language: MnemonicLanguage,
    account_id: i64,
    repo: &Repository,
    endpoint: &LightdEndpoint,
) -> Result<Option<(NetworkType, usize, usize)>> {
    let addresses = repo.get_all_addresses(account_id)?;
    let address_count = addresses.len();
    if addresses.is_empty() {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = chrono::Utc::now().timestamp_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_address_count","timestamp":{},"location":"api.rs:infer_key_network_type_from_addresses","message":"no stored addresses","data":{{"account_id":{},"count":0}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, account_id
            );
        });
        return Ok(None);
    }

    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic_in_language(
        mnemonic,
        Some(mnemonic_language),
    )?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;
    let candidates = [
        NetworkType::Mainnet,
        NetworkType::Testnet,
        NetworkType::Regtest,
    ];

    let mut best_network = None;
    let mut best_matches = 0usize;
    let mut match_counts = Vec::new();

    for candidate in candidates {
        let candidate_network = Network::from_type(candidate);
        let sapling_extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
            mnemonic,
            candidate_network.network_type,
            0,
            Some(mnemonic_language),
        )?;
        let sapling_fvk = sapling_extsk.to_extended_fvk();
        let orchard_extsk = orchard_master.derive_account(candidate_network.coin_type, 0)?;
        let orchard_fvk = orchard_extsk.to_extended_fvk();
        let prefix_network =
            endpoint::address_prefix_network_type_for_endpoint(endpoint, candidate);

        let mut matches = 0usize;
        for addr in &addresses {
            let derived = match addr.address_type {
                AddressType::Orchard => {
                    let orchard_addr = orchard_fvk.address_at(addr.diversifier_index);
                    orchard_addr.encode_for_network(prefix_network)?
                }
                AddressType::Sapling => {
                    let payment_addr = sapling_fvk.derive_address(addr.diversifier_index);
                    payment_addr.encode_for_network(prefix_network)
                }
            };
            if derived == addr.address {
                matches += 1;
            }
        }

        match_counts.push((candidate, matches));
        if matches > best_matches {
            best_matches = matches;
            best_network = Some(candidate);
        }
    }

    pirate_core::debug_log::with_locked_file(|file| {
        let ts = chrono::Utc::now().timestamp_millis();
        let mut summary = String::new();
        for (idx, (candidate, matches)) in match_counts.iter().enumerate() {
            if idx > 0 {
                summary.push(',');
            }
            summary.push_str(&format!(
                r#"{{"network":"{:?}","matches":{}}}"#,
                candidate, matches
            ));
        }
        let sample = addresses.first().map(|addr| {
            let prefix_len = addr.address.chars().take(8).count();
            let sample = addr.address.chars().take(prefix_len).collect::<String>();
            (sample, addr.address_type)
        });
        if let Some((sample_prefix, sample_type)) = sample {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_address_match","timestamp":{},"location":"api.rs:infer_key_network_type_from_addresses","message":"address match summary","data":{{"account_id":{},"count":{},"sample_prefix":"{}","sample_type":"{:?}","matches":[{}]}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, account_id, address_count, sample_prefix, sample_type, summary
            );
        }
    });

    if best_matches == 0 {
        return Ok(None);
    }

    Ok(best_network.map(|network| (network, best_matches, addresses.len())))
}

fn rederive_wallet_keys_for_network(
    wallet_id: &WalletId,
    old_network_type: NetworkType,
    new_network_type: NetworkType,
) -> Result<()> {
    {
        let ts = chrono::Utc::now().timestamp_millis();
        pirate_core::debug_log::with_locked_file(|file| {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_start","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive start","data":{{"wallet_id":"{}","old_network":"{:?}","new_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, old_network_type, new_network_type
            );
        });
    }

    let (_db, repo) = open_wallet_db_for(wallet_id)?;
    let mut secret = repo
        .get_wallet_secret(wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let mnemonic_bytes = match secret.encrypted_mnemonic.as_ref() {
        Some(bytes) => bytes,
        None => {
            tracing::warn!(
                "Wallet {} has no mnemonic stored; skipping key re-derive",
                wallet_id
            );
            let ts = chrono::Utc::now().timestamp_millis();
            pirate_core::debug_log::with_locked_file(|file| {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rederive_skip","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive skipped (no mnemonic)","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id
                );
            });
            return Ok(());
        }
    };

    let mnemonic = String::from_utf8(mnemonic_bytes.clone())
        .map_err(|_| anyhow!("Stored mnemonic is not valid UTF-8"))?;
    let mnemonic_language = wallet_secret_mnemonic_language(&secret, &mnemonic)?;

    let old_network = Network::from_type(old_network_type);
    let current_extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
        &mnemonic,
        old_network.network_type,
        0,
        Some(mnemonic_language),
    )?;

    let mut matches_any = current_extsk.to_bytes() == secret.extsk;
    if !matches_any {
        let candidates = [
            NetworkType::Mainnet,
            NetworkType::Testnet,
            NetworkType::Regtest,
        ];
        for candidate in candidates {
            if candidate == old_network_type {
                continue;
            }
            let candidate_net = Network::from_type(candidate);
            let candidate_extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
                &mnemonic,
                candidate_net.network_type,
                0,
                Some(mnemonic_language),
            )?;
            if candidate_extsk.to_bytes() == secret.extsk {
                matches_any = true;
                break;
            }
        }
    }

    if !matches_any {
        tracing::warn!(
            "Wallet {} appears to use a non-empty BIP-39 passphrase; skipping key re-derive",
            wallet_id
        );
        let ts = chrono::Utc::now().timestamp_millis();
        pirate_core::debug_log::with_locked_file(|file| {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_skip","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive skipped (passphrase mismatch)","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id
            );
        });
        return Ok(());
    }

    let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;
    let inferred_network = infer_key_network_type_from_addresses(
        &mnemonic,
        mnemonic_language,
        secret.account_id,
        &repo,
        &endpoint,
    )?;
    let key_network_type = if let Some((network_type, matched, total)) = inferred_network {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = chrono::Utc::now().timestamp_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_infer","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive inferred key network","data":{{"wallet_id":"{}","inferred_network":"{:?}","matched":{},"total":{},"endpoint_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, network_type, matched, total, new_network_type
            );
        });
        network_type
    } else {
        let prefix_network =
            endpoint::address_prefix_network_type_for_endpoint(&endpoint, new_network_type);
        if prefix_network != new_network_type {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = chrono::Utc::now().timestamp_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rederive_prefix_fallback","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive using prefix network fallback","data":{{"wallet_id":"{}","endpoint_network":"{:?}","prefix_network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                    ts, wallet_id, new_network_type, prefix_network
                );
            });
        }
        prefix_network
    };

    let new_network = Network::from_type(key_network_type);
    let new_extsk = ExtendedSpendingKey::from_mnemonic_with_account_and_language(
        &mnemonic,
        new_network.network_type,
        0,
        Some(mnemonic_language),
    )?;
    let seed_bytes = ExtendedSpendingKey::seed_bytes_from_mnemonic_in_language(
        &mnemonic,
        Some(mnemonic_language),
    )?;
    let orchard_master = OrchardExtendedSpendingKey::master(&seed_bytes)?;
    let orchard_extsk = orchard_master.derive_account(new_network.coin_type, 0)?;

    secret.extsk = new_extsk.to_bytes();
    secret.dfvk = Some(new_extsk.to_extended_fvk().to_bytes());
    secret.orchard_extsk = Some(orchard_extsk.to_bytes());
    secret.sapling_ivk = None;
    secret.orchard_ivk = None;

    let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret)?;
    repo.upsert_wallet_secret(&encrypted_secret)?;
    repo.clear_chain_state()?;

    tracing::info!(
        "Re-derived wallet {} keys for network {:?} and cleared chain state",
        wallet_id,
        key_network_type
    );
    {
        let ts = chrono::Utc::now().timestamp_millis();
        pirate_core::debug_log::with_locked_file(|file| {
            let _ = writeln!(
                file,
                r#"{{"id":"log_rederive_ok","timestamp":{},"location":"api.rs:rederive_wallet_keys_for_network","message":"rederive ok","data":{{"wallet_id":"{}","network":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                ts, wallet_id, key_network_type
            );
        });
    }

    Ok(())
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
    tunnel::fetch_external_text(url, accept, user_agent).await
}

/// Fetch arbitrary bytes over the currently selected network tunnel.
pub async fn fetch_external_bytes(
    url: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<Vec<u8>> {
    tunnel::fetch_external_bytes(url, accept, user_agent).await
}

/// Download an external resource to a local file over the currently selected network tunnel.
pub async fn download_external_to_file(
    url: String,
    destination_path: String,
    accept: Option<String>,
    user_agent: Option<String>,
) -> Result<()> {
    tunnel::download_external_to_file(url, destination_path, accept, user_agent).await
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
    if is_decoy_mode_active() {
        return Ok(Balance {
            total: 0,
            spendable: 0,
            pending: 0,
        });
    }
    tracing::info!("Getting balance for wallet {}", wallet_id);

    let suppress_live_reads = sync_control::should_suppress_live_tx_reads(&wallet_id);
    if suppress_live_reads {
        if let Some(cached) = sync_control::get_cached_balance(&wallet_id) {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_get_balance_cached","timestamp":{},"location":"api.rs:get_balance","message":"returning cached balance during active sync mutation","data":{{"wallet_id":"{}","total":{},"spendable":{},"pending":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                    ts, wallet_id, cached.total, cached.spendable, cached.pending
                );
            });
            return Ok(cached);
        }
    }

    // Open encrypted wallet DB
    let (db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    // Get current height from sync state
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
    let sync_state = sync_storage.load_sync_state()?;
    // Confirmation math should be derived from locally-scanned chain state.
    // During active sync mutation (including FoundNote replay), callers should use the stable
    // cached balance above to avoid transient dips.
    let current_height = sync_state.local_height;

    // #region agent log
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_get_balance","timestamp":{},"location":"api.rs:4186","message":"get_balance start","data":{{"wallet_id":"{}","account_id":{},"current_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, wallet_id, secret.account_id, current_height
            );
        });
    }
    // #endregion

    // Standard confirmation depth for wallet spendability.
    const MIN_DEPTH: u64 = 1;

    let unspent = repo.get_unspent_notes(secret.account_id)?;

    // #region agent log
    {
        let (count, sum_value, min_h, max_h) = if unspent.is_empty() {
            (0usize, 0i64, None, None)
        } else {
            let mut sum = 0i64;
            let mut min_height = i64::MAX;
            let mut max_height = i64::MIN;
            for n in &unspent {
                sum = sum.saturating_add(n.value);
                min_height = min_height.min(n.height);
                max_height = max_height.max(n.height);
            }
            (unspent.len(), sum, Some(min_height), Some(max_height))
        };
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_get_balance","timestamp":{},"location":"api.rs:4196","message":"get_balance unspent","data":{{"wallet_id":"{}","unspent_count":{},"unspent_sum":{},"min_height":{},"max_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts,
                wallet_id,
                count,
                sum_value,
                min_h
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                max_h
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            );
        });
    }
    // #endregion

    // Match wallet-summary behavior for displayed balances:
    // - spendable/pending are confirmation-depth based wallet balances
    // - send gating remains controlled by spendability status checks
    let (spendable, mut pending, mut total) =
        repo.calculate_balance(secret.account_id, current_height, MIN_DEPTH)?;

    // Include change from recently-broadcast TXs whose change note hasn't been
    // mined and detected by sync yet. Without this, balance drops to zero between
    // broadcast and the sync engine trial-decrypting the mined change output.
    {
        let known_txids: HashSet<String> = unspent
            .iter()
            .flat_map(|note| tx_flow::txid_hex_variants_from_bytes(&note.txid))
            .collect();
        let unseen_change = tx_flow::resolve_pending_change(&wallet_id, &known_txids);
        if unseen_change > 0 {
            pending = pending.saturating_add(unseen_change);
            total = total.saturating_add(unseen_change);
        }
    }

    // #region agent log
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_get_balance","timestamp":{},"location":"api.rs:4204","message":"get_balance result","data":{{"wallet_id":"{}","total":{},"spendable":{},"pending":{},"min_depth":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, wallet_id, total, spendable, pending, MIN_DEPTH
            );
        });
    }
    // #endregion

    tracing::debug!(
        "Balance for wallet {}: total={}, spendable={}, pending={} (height={})",
        wallet_id,
        total,
        spendable,
        pending,
        current_height
    );

    let balance = Balance {
        total,
        spendable,
        pending,
    };
    // Always refresh the cache, even during mutation mode. This lets the first
    // fallback read populate a stable snapshot and avoids repeated heavy DB reads
    // while sync is actively mutating state.
    sync_control::put_cached_balance(&wallet_id, &balance);
    Ok(balance)
}

/// Get optional advanced split balances for the Sapling and Orchard pools.
pub fn get_shielded_pool_balances(wallet_id: WalletId) -> Result<ShieldedPoolBalances> {
    if is_decoy_mode_active() {
        let zero = Balance {
            total: 0,
            spendable: 0,
            pending: 0,
        };
        return Ok(ShieldedPoolBalances {
            sapling: zero.clone(),
            orchard: zero,
        });
    }

    let (db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
    let sync_state = sync_storage.load_sync_state()?;
    let current_height = sync_state.local_height;
    const MIN_DEPTH: u64 = 1;

    let mut sapling = Balance {
        total: 0,
        spendable: 0,
        pending: 0,
    };
    let mut orchard = Balance {
        total: 0,
        spendable: 0,
        pending: 0,
    };

    for note in repo.get_unspent_notes(secret.account_id)? {
        if note.value <= 0 {
            continue;
        }
        let value = note.value as u64;
        let is_spendable =
            note.height > 0 && current_height.saturating_sub(note.height as u64) + 1 >= MIN_DEPTH;
        let target = match note.note_type {
            pirate_storage_sqlite::models::NoteType::Sapling => &mut sapling,
            pirate_storage_sqlite::models::NoteType::Orchard => &mut orchard,
        };
        target.total = target.total.saturating_add(value);
        if is_spendable {
            target.spendable = target.spendable.saturating_add(value);
        } else {
            target.pending = target.pending.saturating_add(value);
        }
    }

    Ok(ShieldedPoolBalances { sapling, orchard })
}

/// List transactions
///
/// Returns transaction history from the database, aggregated by transaction ID.
/// Transactions are sorted by height descending (newest first).
pub fn list_transactions(wallet_id: WalletId, limit: Option<u32>) -> Result<Vec<TxInfo>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let suppress_live_reads = sync_control::should_suppress_live_tx_reads(&wallet_id);
    if suppress_live_reads {
        if let Some(cached) = sync_control::get_cached_transactions(&wallet_id, limit) {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_list_transactions_cached","timestamp":{},"location":"api.rs:list_transactions","message":"returning cached tx list during active sync mutation","data":{{"wallet_id":"{}","limit":"{:?}","cached_count":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    ts,
                    wallet_id,
                    limit,
                    cached.len()
                );
            });
            return Ok(cached);
        }
    }
    tracing::info!(
        "Listing transactions for wallet {} (limit: {:?})",
        wallet_id,
        limit
    );

    // Open encrypted wallet DB
    let (db, repo) = open_wallet_db_for(&wallet_id)?;

    // Get wallet secret to find account_id
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

    let spendable =
        !secret.extsk.is_empty() || secret.orchard_extsk.as_ref().is_some_and(|k| !k.is_empty());
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = format!("{:08x}", ts);
        let _ = writeln!(
            file,
            r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:list_transactions","message":"list_transactions flags","data":{{"wallet_id":"{}","spendable":{},"extsk_len":{},"orchard_extsk_len":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
            id,
            ts,
            wallet_id,
            spendable,
            secret.extsk.len(),
            secret.orchard_extsk.as_ref().map(|k| k.len()).unwrap_or(0)
        );
    });

    // Get current height from sync state
    let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
    let sync_state = sync_storage.load_sync_state()?;
    // Use the best known synced height for confirmation display stability.
    let current_height = sync_state.local_height.max(sync_state.target_height);

    // Confirmation thresholds for transaction display.
    const RECEIVE_MIN_DEPTH: u64 = 1;
    const SEND_MIN_DEPTH: u64 = 1;

    // Get transactions from database
    let split_transfers = spendable;
    let tx_records = repo.get_transactions_with_options(
        secret.account_id,
        limit,
        current_height,
        RECEIVE_MIN_DEPTH,
        split_transfers,
    )?;

    // Convert to TxInfo format
    let transactions: Vec<TxInfo> = tx_records
        .into_iter()
        .map(|tx| {
            // Determine confirmed status
            let confirmed = if tx.height > 0 {
                let height = tx.height as u64;
                let confirmations = if current_height >= height {
                    current_height.saturating_sub(height).saturating_add(1)
                } else {
                    0
                };
                let min_depth = if tx.amount < 0 {
                    SEND_MIN_DEPTH
                } else {
                    RECEIVE_MIN_DEPTH
                };
                confirmations >= min_depth
            } else {
                false
            };

            // Decode memo from bytes to string (if present).
            // Use protocol-aware decoder so both Sapling and Orchard memo bytes
            // render consistently in the transaction list.
            let memo_str = tx.memo.and_then(|memo_bytes| {
                pirate_sync_lightd::sapling::full_decrypt::decode_memo(&memo_bytes)
            });

            TxInfo {
                txid: tx.txid,
                height: if tx.height > 0 {
                    Some(tx.height as u32)
                } else {
                    None
                },
                timestamp: tx.timestamp,
                amount: tx.amount,
                fee: tx.fee,
                memo: memo_str,
                confirmed,
            }
        })
        .collect();

    tracing::debug!(
        "Found {} transactions for wallet {}",
        transactions.len(),
        wallet_id
    );

    // Always refresh the cache, even during mutation mode. This lets the first
    // fallback read populate a stable snapshot and avoids repeated heavy DB reads
    // while sync is actively mutating state.
    sync_control::put_cached_transactions(&wallet_id, limit, &transactions);

    Ok(transactions)
}

/// List wallet notes for inspection.
pub fn list_notes(wallet_id: WalletId, all_notes: bool) -> Result<Vec<crate::models::NoteInfo>> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let notes = if all_notes {
        repo.get_spend_reconciliation_notes(secret.account_id)?
    } else {
        repo.get_unspent_notes(secret.account_id)?
    };

    Ok(notes
        .into_iter()
        .map(|note| crate::models::NoteInfo {
            id: note.id,
            note_type: format!("{:?}", note.note_type),
            value: note.value,
            spent: note.spent,
            height: note.height,
            txid: hex::encode(note.txid),
            output_index: note.output_index,
            key_id: note.key_id,
            address_id: note.address_id,
            memo: note
                .memo
                .as_ref()
                .and_then(|memo| pirate_sync_lightd::sapling::full_decrypt::decode_memo(memo)),
        })
        .collect())
}

/// Clear wallet chain-derived state.
pub fn clear_wallet_state(wallet_id: WalletId) -> Result<()> {
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    repo.clear_chain_state()?;
    sync_control::clear_wallet_sync_state(&wallet_id);
    Ok(())
}

/// Fetch detailed transaction information, including memo and recovered outgoing recipients.
pub async fn get_transaction_details(
    wallet_id: WalletId,
    txid: String,
) -> Result<Option<TransactionDetails>> {
    let tx_info = list_transactions(wallet_id.clone(), None)?
        .into_iter()
        .find(|tx| tx.txid == txid);
    let Some(tx_info) = tx_info else {
        return Ok(None);
    };

    let memo = fetch_transaction_memo(wallet_id.clone(), txid.clone(), None).await?;

    let recipients = if tx_info.amount < 0 {
        let (endpoint_config, tx_hash_candidates, sapling_ovks, orchard_ovks, tx_height_hint) =
            collect_tx_recovery_context(&wallet_id, &txid)?;
        let client_config = tunnel::light_client_config_for_endpoint(
            &endpoint_config,
            RetryConfig::default(),
            Duration::from_secs(30),
            Duration::from_secs(60),
        );
        let client = LightClient::with_config(client_config);
        client
            .connect()
            .await
            .map_err(|e| anyhow!("Failed to connect to lightwalletd: {}", e))?;

        let mut raw_tx_bytes: Option<Vec<u8>> = None;
        let mut last_fetch_err: Option<String> = None;
        for tx_hash in &tx_hash_candidates {
            match client.get_transaction(tx_hash).await {
                Ok(raw) => {
                    raw_tx_bytes = Some(raw);
                    break;
                }
                Err(e) => last_fetch_err = Some(e.to_string()),
            }
        }

        if let Some(raw_tx_bytes) = raw_tx_bytes {
            payment_disclosure::recover_outgoing_recipients_with_disclosures_from_raw_tx(
                &raw_tx_bytes,
                tx_height_hint,
                &sapling_ovks,
                &orchard_ovks,
                address_prefix_network_type(&wallet_id)?,
            )
        } else {
            tracing::warn!(
                "Failed to fetch raw transaction {} for recipient recovery: {}",
                txid,
                last_fetch_err.unwrap_or_else(|| "unknown error".to_string())
            );
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(Some(TransactionDetails {
        txid: tx_info.txid,
        height: tx_info.height,
        timestamp: tx_info.timestamp,
        amount: tx_info.amount,
        fee: tx_info.fee,
        confirmed: tx_info.confirmed,
        memo,
        recipients,
    }))
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
    run_on_runtime(move || fetch_transaction_memo_inner(wallet_id, txid, output_index)).await
}

async fn fetch_transaction_memo_inner(
    wallet_id: WalletId,
    txid: String,
    output_index: Option<u32>,
) -> Result<Option<String>> {
    tracing::info!(
        "Fetching memo for transaction {} (output_index: {:?})",
        txid,
        output_index
    );

    // Extract all data from DB in a block scope to ensure repo is dropped before async.
    let (
        endpoint_config,
        account_id,
        tx_hash_candidates,
        txid_bytes,
        sapling_candidates,
        orchard_candidates,
        sapling_ovk_candidates,
        orchard_ovk_candidates,
        tx_height_hint,
        stored_memo,
    ) = {
        // Open encrypted wallet DB
        let (db, repo) = open_wallet_db_for(&wallet_id)?;

        // Get wallet secret to find account_id
        let secret = repo
            .get_wallet_secret(&wallet_id)?
            .ok_or_else(|| anyhow!("No wallet secret found for {}", wallet_id))?;

        // Parse txid from hex and support either byte order.
        let parsed_txid = hex::decode(&txid).map_err(|e| anyhow!("Invalid txid hex: {}", e))?;
        if parsed_txid.len() != 32 {
            return Err(anyhow!(
                "Invalid txid length: {} (expected 32 bytes)",
                parsed_txid.len()
            ));
        }

        let mut reversed_txid = parsed_txid.clone();
        reversed_txid.reverse();
        let notes_direct = repo.get_notes_by_txid(secret.account_id, &parsed_txid)?;
        let (txid_bytes, notes) = if notes_direct.is_empty() {
            let notes_reversed = repo.get_notes_by_txid(secret.account_id, &reversed_txid)?;
            if notes_reversed.is_empty() {
                (parsed_txid.clone(), notes_direct)
            } else {
                (reversed_txid.clone(), notes_reversed)
            }
        } else {
            (parsed_txid.clone(), notes_direct)
        };

        let mut tx_hash_candidates: Vec<[u8; 32]> = Vec::new();
        let mut push_tx_hash_candidate = |bytes: &[u8]| {
            if bytes.len() != 32 {
                return;
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(bytes);
            if !tx_hash_candidates.contains(&arr) {
                tx_hash_candidates.push(arr);
            }
        };
        push_tx_hash_candidate(&txid_bytes);
        push_tx_hash_candidate(&parsed_txid);
        push_tx_hash_candidate(&reversed_txid);

        let default_sapling_ivk = if !secret.extsk.is_empty() {
            ExtendedSpendingKey::from_bytes(&secret.extsk)
                .map(|extsk| extsk.to_extended_fvk().to_ivk().to_sapling_ivk_bytes())
                .ok()
        } else if let Some(ref dfvk_bytes) = secret.dfvk {
            ExtendedFullViewingKey::from_bytes(dfvk_bytes)
                .map(|dfvk| dfvk.to_ivk().to_sapling_ivk_bytes())
        } else if let Some(ref ivk_bytes) = secret.sapling_ivk {
            if ivk_bytes.len() == 32 {
                let mut ivk = [0u8; 32];
                ivk.copy_from_slice(&ivk_bytes[..32]);
                Some(ivk)
            } else {
                None
            }
        } else {
            None
        };

        let default_orchard_ivk = if let Some(ref extsk_bytes) = secret.orchard_extsk {
            OrchardExtendedSpendingKey::from_bytes(extsk_bytes)
                .map(|extsk| extsk.to_extended_fvk().to_ivk_bytes())
                .ok()
        } else if let Some(ref orchard_ivk_bytes) = secret.orchard_ivk {
            if orchard_ivk_bytes.len() == 64 {
                let mut ivk = [0u8; 64];
                ivk.copy_from_slice(&orchard_ivk_bytes[..64]);
                Some(ivk)
            } else {
                OrchardExtendedFullViewingKey::from_bytes(orchard_ivk_bytes)
                    .ok()
                    .map(|fvk| fvk.to_ivk_bytes())
            }
        } else {
            None
        };
        let mut sapling_ovk_candidates: Vec<SaplingOutgoingViewingKey> = Vec::new();
        let mut seen_sapling_ovks: HashSet<[u8; 32]> = HashSet::new();
        let mut push_sapling_ovk = |ovk: SaplingOutgoingViewingKey| {
            if seen_sapling_ovks.insert(ovk.0) {
                sapling_ovk_candidates.push(ovk);
            }
        };

        let mut orchard_ovk_candidates: Vec<orchard::keys::OutgoingViewingKey> = Vec::new();
        let mut push_orchard_ovk = |ovk: orchard::keys::OutgoingViewingKey| {
            orchard_ovk_candidates.push(ovk);
        };

        if !secret.extsk.is_empty() {
            if let Ok(extsk) = ExtendedSpendingKey::from_bytes(&secret.extsk) {
                push_sapling_ovk(extsk.to_extended_fvk().outgoing_viewing_key());
            }
        } else if let Some(ref dfvk_bytes) = secret.dfvk {
            if let Some(dfvk) = ExtendedFullViewingKey::from_bytes(dfvk_bytes) {
                push_sapling_ovk(dfvk.outgoing_viewing_key());
            }
        }

        if let Some(ref orchard_extsk) = secret.orchard_extsk {
            if let Ok(extsk) = OrchardExtendedSpendingKey::from_bytes(orchard_extsk) {
                push_orchard_ovk(extsk.to_extended_fvk().to_ovk());
            }
        } else if let Some(ref orchard_ivk) = secret.orchard_ivk {
            if orchard_ivk.len() == 137 {
                if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bytes(orchard_ivk) {
                    push_orchard_ovk(fvk.to_ovk());
                }
            }
        }

        for key in repo.get_account_keys(secret.account_id)? {
            if let Some(ref extsk_bytes) = key.sapling_extsk {
                if let Ok(extsk) = ExtendedSpendingKey::from_bytes(extsk_bytes) {
                    push_sapling_ovk(extsk.to_extended_fvk().outgoing_viewing_key());
                }
            } else if let Some(ref dfvk_bytes) = key.sapling_dfvk {
                if let Some(dfvk) = ExtendedFullViewingKey::from_bytes(dfvk_bytes) {
                    push_sapling_ovk(dfvk.outgoing_viewing_key());
                }
            }

            if let Some(ref extsk_bytes) = key.orchard_extsk {
                if let Ok(extsk) = OrchardExtendedSpendingKey::from_bytes(extsk_bytes) {
                    push_orchard_ovk(extsk.to_extended_fvk().to_ovk());
                }
            } else if let Some(ref fvk_bytes) = key.orchard_fvk {
                if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bytes(fvk_bytes) {
                    push_orchard_ovk(fvk.to_ovk());
                }
            }
        }

        let mut sapling_ivk_by_key: HashMap<i64, [u8; 32]> = HashMap::new();
        let mut orchard_ivk_by_key: HashMap<i64, [u8; 64]> = HashMap::new();
        let mut sapling_candidates: Vec<(i64, [u8; 32], Option<[u8; 32]>)> = Vec::new();
        let mut orchard_candidates: Vec<(i64, [u8; 64], Option<[u8; 32]>)> = Vec::new();
        let mut seen_sapling_output_indices: HashSet<i64> = HashSet::new();
        let mut seen_orchard_output_indices: HashSet<i64> = HashSet::new();
        let mut stored_memo: Option<Vec<u8>> = if output_index.is_none() {
            repo.get_tx_memo(&txid)?
        } else {
            None
        };

        for note in &notes {
            if let Some(requested_idx) = output_index {
                if note.output_index != requested_idx as i64 {
                    continue;
                }
            }

            if stored_memo.is_none() {
                if let Some(memo) = note.memo.clone() {
                    stored_memo = Some(memo);
                }
            }

            let commitment = if note.commitment.len() == 32 {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&note.commitment[..32]);
                Some(bytes)
            } else {
                None
            };

            match note.note_type {
                pirate_storage_sqlite::models::NoteType::Sapling => {
                    if !seen_sapling_output_indices.insert(note.output_index) {
                        continue;
                    }
                    let ivk_opt = if let Some(key_id) = note.key_id {
                        if let Some(cached) = sapling_ivk_by_key.get(&key_id) {
                            Some(*cached)
                        } else {
                            let key = repo
                                .get_account_key_by_id(key_id)?
                                .ok_or_else(|| anyhow!("Key group not found"))?;
                            let ivk = if let Some(ref bytes) = key.sapling_extsk {
                                let extsk = ExtendedSpendingKey::from_bytes(bytes)?;
                                extsk.to_extended_fvk().to_ivk().to_sapling_ivk_bytes()
                            } else if let Some(ref bytes) = key.sapling_dfvk {
                                let dfvk = ExtendedFullViewingKey::from_bytes(bytes)
                                    .ok_or_else(|| anyhow!("Invalid Sapling viewing key bytes"))?;
                                dfvk.to_ivk().to_sapling_ivk_bytes()
                            } else {
                                continue;
                            };
                            sapling_ivk_by_key.insert(key_id, ivk);
                            Some(ivk)
                        }
                    } else {
                        default_sapling_ivk
                    };

                    if let Some(ivk) = ivk_opt {
                        sapling_candidates.push((note.output_index, ivk, commitment));
                    }
                }
                pirate_storage_sqlite::models::NoteType::Orchard => {
                    if !seen_orchard_output_indices.insert(note.output_index) {
                        continue;
                    }
                    let ivk_opt = if let Some(key_id) = note.key_id {
                        if let Some(cached) = orchard_ivk_by_key.get(&key_id) {
                            Some(*cached)
                        } else {
                            let key = repo
                                .get_account_key_by_id(key_id)?
                                .ok_or_else(|| anyhow!("Key group not found"))?;
                            let ivk = if let Some(ref bytes) = key.orchard_extsk {
                                OrchardExtendedSpendingKey::from_bytes(bytes)
                                    .map(|extsk| extsk.to_extended_fvk().to_ivk_bytes())
                                    .map_err(|e| anyhow!("Invalid Orchard spending key: {}", e))?
                            } else if let Some(ref bytes) = key.orchard_fvk {
                                OrchardExtendedFullViewingKey::from_bytes(bytes)
                                    .map(|fvk| fvk.to_ivk_bytes())
                                    .map_err(|e| anyhow!("Invalid Orchard viewing key: {}", e))?
                            } else {
                                continue;
                            };
                            orchard_ivk_by_key.insert(key_id, ivk);
                            Some(ivk)
                        }
                    } else {
                        default_orchard_ivk
                    };

                    if let Some(ivk) = ivk_opt {
                        orchard_candidates.push((note.output_index, ivk, commitment));
                    }
                }
            }
        }

        // If caller requested a specific output index and no matching local note was found,
        // still try decrypting that index with the wallet-default viewing keys.
        if let Some(requested_idx) = output_index {
            let idx = requested_idx as i64;
            if !seen_sapling_output_indices.contains(&idx) {
                if let Some(ivk) = default_sapling_ivk {
                    sapling_candidates.push((idx, ivk, None));
                }
            }
            if !seen_orchard_output_indices.contains(&idx) {
                if let Some(ivk) = default_orchard_ivk {
                    orchard_candidates.push((idx, ivk, None));
                }
            }
        }

        let mut tx_height_hint = notes
            .iter()
            .map(|note| note.height)
            .filter(|height| *height > 0)
            .max()
            .and_then(|height| u32::try_from(height).ok());

        if tx_height_hint.is_none() {
            let mut txid_candidates = vec![
                hex::encode(&txid_bytes),
                hex::encode(&parsed_txid),
                hex::encode(&reversed_txid),
            ];
            txid_candidates.sort_unstable();
            txid_candidates.dedup();

            for candidate in txid_candidates {
                let mut stmt = db.conn().prepare(
                    "SELECT height FROM transactions WHERE txid = ?1 AND height > 0 ORDER BY height DESC LIMIT 1",
                )?;
                let mut rows = stmt.query(params![candidate])?;
                if let Some(row) = rows.next()? {
                    let height: i64 = row.get(0)?;
                    if let Ok(parsed_height) = u32::try_from(height) {
                        tx_height_hint = Some(parsed_height);
                        break;
                    }
                }
            }
        }

        (
            get_lightd_endpoint_config(wallet_id.clone())?,
            secret.account_id,
            tx_hash_candidates,
            txid_bytes,
            sapling_candidates,
            orchard_candidates,
            sapling_ovk_candidates,
            orchard_ovk_candidates,
            tx_height_hint,
            stored_memo,
        )
    };

    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint_config,
        RetryConfig::default(),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(60),
    );

    if let Some(stored) = stored_memo {
        let memo = pirate_sync_lightd::sapling::full_decrypt::decode_memo(&stored);
        if memo.is_some() {
            return Ok(memo);
        }
    }

    // Memo not in database or validation failed, fetch and decrypt
    let client = pirate_sync_lightd::LightClient::with_config(client_config);
    client
        .connect()
        .await
        .map_err(|e| anyhow!("Failed to connect to lightwalletd: {}", e))?;

    let mut raw_tx_bytes: Option<Vec<u8>> = None;
    let mut last_fetch_err: Option<String> = None;
    for tx_hash in &tx_hash_candidates {
        match client.get_transaction(tx_hash).await {
            Ok(raw) => {
                raw_tx_bytes = Some(raw);
                break;
            }
            Err(e) => {
                last_fetch_err = Some(e.to_string());
            }
        }
    }
    let raw_tx_bytes = raw_tx_bytes.ok_or_else(|| {
        anyhow!(
            "Failed to fetch transaction: {}",
            last_fetch_err.unwrap_or_else(|| "unknown get_transaction error".to_string())
        )
    })?;

    // Decrypt Sapling memo candidates.
    for (idx, ivk_bytes, cmu_opt) in &sapling_candidates {
        match pirate_sync_lightd::sapling::full_decrypt::decrypt_memo_from_raw_tx_with_ivk_bytes(
            &raw_tx_bytes,
            *idx as usize,
            ivk_bytes,
            cmu_opt.as_ref(),
        ) {
            Ok(Some(decrypted)) => {
                // Store memo in database (re-open DB)
                let (_db3, repo3) = open_wallet_db_for(&wallet_id)?;
                repo3.update_note_memo_with_type(
                    account_id,
                    &txid_bytes,
                    *idx,
                    Some(pirate_storage_sqlite::models::NoteType::Sapling),
                    Some(&decrypted.memo),
                )?;

                // Decode and return
                let memo_str =
                    pirate_sync_lightd::sapling::full_decrypt::decode_memo(&decrypted.memo);
                tracing::info!(
                    "Fetched and stored Sapling memo for tx {} output {}",
                    txid,
                    idx
                );
                if memo_str.is_some() || output_index.is_some() {
                    return Ok(memo_str);
                }
            }
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("Failed to decrypt Sapling memo for output {}: {}", idx, e);
                continue;
            }
        }
    }

    // Decrypt Orchard memo candidates.
    for (idx, orchard_ivk, cmx_opt) in &orchard_candidates {
        match pirate_sync_lightd::orchard::full_decrypt::decrypt_orchard_memo_from_raw_tx_with_ivk_bytes(
            &raw_tx_bytes,
            *idx as usize,
            orchard_ivk,
            cmx_opt.as_ref(),
        ) {
            Ok(Some(decrypted)) => {
                let memo_bytes = decrypted.memo.to_vec();
                // Store memo in database (re-open DB)
                let (_db3, repo3) = open_wallet_db_for(&wallet_id)?;
                repo3.update_note_memo_with_type(
                    account_id,
                    &txid_bytes,
                    *idx,
                    Some(pirate_storage_sqlite::models::NoteType::Orchard),
                    Some(&memo_bytes),
                )?;

                // Decode and return
                let memo_str =
                    pirate_sync_lightd::sapling::full_decrypt::decode_memo(&memo_bytes);
                tracing::info!("Fetched and stored Orchard memo for tx {} output {}", txid, idx);
                if memo_str.is_some() || output_index.is_some() {
                    return Ok(memo_str);
                }
            }
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("Failed to decrypt Orchard memo for output {}: {}", idx, e);
                continue;
            }
        }
    }

    if output_index.is_none() {
        if let Some(memo_bytes) = recover_outgoing_memo_from_raw_tx(
            &raw_tx_bytes,
            tx_height_hint,
            &sapling_ovk_candidates,
            &orchard_ovk_candidates,
        ) {
            let (_db3, repo3) = open_wallet_db_for(&wallet_id)?;
            let txid_hex = hex::encode(&txid_bytes);
            if let Err(e) = repo3.upsert_tx_memo(&txid_hex, &memo_bytes) {
                tracing::warn!(
                    "Failed to persist recovered outgoing memo for {}: {}",
                    txid,
                    e
                );
            }
            let memo_str = pirate_sync_lightd::sapling::full_decrypt::decode_memo(&memo_bytes);
            if memo_str.is_some() {
                return Ok(memo_str);
            }
        }
    }

    // No memo found for any output
    Ok(None)
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
    // Validate word count (must be 12, 18, or 24)
    if let Some(count) = word_count {
        if count != 12 && count != 18 && count != 24 {
            return Err(anyhow!("Invalid word count: must be 12, 18, or 24"));
        }
    }

    Ok(ExtendedSpendingKey::generate_mnemonic_in_language(
        word_count,
        mnemonic_language,
    ))
}

/// Validate mnemonic
pub fn validate_mnemonic(
    mnemonic: String,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<bool> {
    match pirate_core::mnemonic::parse_mnemonic(&mnemonic, mnemonic_language) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Inspect mnemonic validity, language, and ambiguity.
pub fn inspect_mnemonic(mnemonic: String) -> Result<MnemonicInspection> {
    Ok(inspect_mnemonic_core(&mnemonic))
}

/// Convert a mnemonic phrase to a different display language while preserving seed entropy.
pub fn convert_mnemonic_language(
    mnemonic: String,
    source_language: Option<MnemonicLanguage>,
    target_language: MnemonicLanguage,
) -> Result<String> {
    Ok(pirate_core::mnemonic::convert_mnemonic_language(
        &mnemonic,
        source_language,
        target_language,
    )?)
}

/// Get network info
pub fn get_network_info() -> Result<NetworkInfo> {
    let net = pirate_params::Network::mainnet();

    Ok(NetworkInfo {
        name: net.name.to_string(),
        coin_type: net.coin_type,
        rpc_port: net.rpc_port,
        default_birthday: net.default_birthday_height,
    })
}

/// Format amount (arrrtoshis to ARRR)
pub fn format_amount(arrrtoshis: u64) -> Result<String> {
    let arrr = arrrtoshis as f64 / 100_000_000.0;
    Ok(format!("{:.8}", arrr))
}

/// Parse amount (ARRR to arrrtoshis)
pub fn parse_amount(arrr: String) -> Result<u64> {
    let value: f64 = arrr.parse().map_err(|_| anyhow!("Invalid amount"))?;
    Ok((value * 100_000_000.0) as u64)
}

// ============================================================================
// Security Features
// ============================================================================

use pirate_storage_sqlite::{
    SaplingViewingKeyImportRequest, WatchOnlyBanner, WatchOnlyCapabilities, WatchOnlyManager,
};

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
    panic_duress::set_panic_pin(pin)
}

/// Check if panic PIN is configured
pub fn has_panic_pin() -> Result<bool> {
    panic_duress::has_panic_pin()
}

/// Verify panic PIN (returns true if PIN matches and activates decoy mode)
pub fn verify_panic_pin(pin: String) -> Result<bool> {
    panic_duress::verify_panic_pin(pin)
}

/// Check if currently in decoy mode
pub fn is_decoy_mode() -> Result<bool> {
    panic_duress::is_decoy_mode()
}

/// Get current vault mode
pub fn get_vault_mode() -> Result<String> {
    panic_duress::get_vault_mode()
}

/// Clear panic PIN and disable decoy vault
pub fn clear_panic_pin() -> Result<()> {
    panic_duress::clear_panic_pin()
}

/// Set duress passphrase for decoy vault
pub fn set_duress_passphrase(custom_passphrase: Option<String>) -> Result<()> {
    panic_duress::set_duress_passphrase(custom_passphrase)
}

/// Check if a duress passphrase is configured
pub fn has_duress_passphrase() -> Result<bool> {
    panic_duress::has_duress_passphrase()
}

/// Clear duress passphrase configuration
pub fn clear_duress_passphrase() -> Result<()> {
    panic_duress::clear_duress_passphrase()
}

/// Verify duress passphrase (activates decoy mode if correct)
pub fn verify_duress_passphrase(passphrase: String) -> Result<bool> {
    panic_duress::verify_duress_passphrase(passphrase)
}

/// Set decoy wallet name
pub fn set_decoy_wallet_name(name: String) -> Result<()> {
    panic_duress::set_decoy_wallet_name(name)
}

/// Exit decoy mode (requires real passphrase re-authentication)
pub fn exit_decoy_mode(passphrase: String) -> Result<()> {
    panic_duress::exit_decoy_mode(passphrase)
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
    seed_export::export_seed_with_passphrase(wallet_id, passphrase, mnemonic_language)
}

/// Export seed using cached app passphrase (after biometric approval).
pub fn export_seed_with_cached_passphrase(
    wallet_id: WalletId,
    mnemonic_language: Option<MnemonicLanguage>,
) -> Result<Vec<String>> {
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
    let wallet = get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export viewing key from watch-only wallet"));
    }
    // Load wallet secret from encrypted storage and extract viewing key.
    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    // Derive xFVK from stored spending key
    let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
        .map_err(|e| anyhow!("Invalid spending key bytes: {}", e))?;
    let network_type_str = wallet.network_type.as_deref().unwrap_or("mainnet");
    let network_type = match network_type_str {
        "testnet" => NetworkType::Testnet,
        "regtest" => NetworkType::Regtest,
        _ => NetworkType::Mainnet,
    };
    let ivk = extsk.to_xfvk_bech32_for_network(network_type);

    let manager = WATCH_ONLY.read();
    let result = manager
        .export_sapling_viewing_key(&wallet_id, ivk)
        .map_err(|e| anyhow!("Failed to export viewing key: {}", e))?;

    tracing::info!("Viewing key exported for wallet {}", wallet_id);

    Ok(result.sapling_viewing_key().to_string())
}

/// Import Sapling viewing key to create watch-only wallet
pub fn import_sapling_viewing_key_as_watch_only(
    name: String,
    sapling_viewing_key: String,
    birthday_height: u32,
) -> Result<WalletId> {
    // Validate import request
    let request = SaplingViewingKeyImportRequest::new(
        name.clone(),
        sapling_viewing_key.clone(),
        birthday_height,
    );
    let manager = WATCH_ONLY.read();
    manager
        .validate_sapling_viewing_key_import(&request)
        .map_err(|e| anyhow!("Invalid viewing key import: {}", e))?;

    let wallet_id = import_viewing_wallet(name, Some(sapling_viewing_key), None, birthday_height)?;
    tracing::info!("Watch-only wallet created: {}", wallet_id);
    Ok(wallet_id)
}

/// Get watch-only capabilities for a wallet
pub fn get_watch_only_capabilities(wallet_id: WalletId) -> Result<WatchOnlyCapabilitiesInfo> {
    let wallet = get_wallet_meta(&wallet_id)?;

    let caps = if wallet.watch_only {
        WatchOnlyCapabilities::watch_only()
    } else {
        WatchOnlyCapabilities::full_wallet()
    };

    Ok(WatchOnlyCapabilitiesInfo {
        can_view_incoming: caps.can_view_incoming,
        can_view_outgoing: caps.can_view_outgoing,
        can_spend: caps.can_spend,
        can_export_seed: caps.can_export_seed,
        can_generate_addresses: caps.can_generate_addresses,
        is_watch_only: wallet.watch_only,
    })
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
    let wallet = get_wallet_meta(&wallet_id)?;

    if !wallet.watch_only {
        return Ok(None);
    }

    let banner = WatchOnlyBanner::incoming_only();

    Ok(Some(WatchOnlyBannerInfo {
        banner_type: format!("{:?}", banner.banner_type),
        title: banner.title,
        subtitle: banner.subtitle,
        icon: banner.icon,
    }))
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
    let manager = WATCH_ONLY.read();
    Ok(manager.clipboard_remaining_seconds())
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

/// Validate that the SDK and the configured lightwalletd endpoint agree on the consensus branch.
pub async fn validate_consensus_branch(wallet_id: WalletId) -> Result<ConsensusBranchValidation> {
    let endpoint = get_lightd_endpoint_config(wallet_id.clone())?;
    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint,
        RetryConfig::default(),
        Duration::from_secs(30),
        Duration::from_secs(60),
    );
    let client = LightClient::with_config(client_config);
    client
        .connect()
        .await
        .map_err(|e| anyhow!("Failed to connect to lightwalletd: {}", e))?;

    let info = client
        .get_lightd_info()
        .await
        .map_err(|e| anyhow!("Failed to fetch lightwalletd info: {}", e))?;

    let network = PirateNetwork::new(wallet_network_type(&wallet_id)?);
    let server_height_u32 = u32::try_from(info.block_height)
        .map_err(|_| anyhow!("Server height out of range: {}", info.block_height))?;
    let sdk_branch = BranchId::for_height(&network, BlockHeight::from_u32(server_height_u32));
    let server_branch = parse_branch_id_hex(&info.consensus_branch_id);

    let sdk_branch_hex = Some(format_branch_id_hex(sdk_branch));
    let server_branch_hex = server_branch.map(format_branch_id_hex);
    let has_server_branch = server_branch.is_some();
    let has_sdk_branch = true;
    let is_valid = server_branch == Some(sdk_branch);
    let server_u32 = server_branch.map(u32::from);
    let sdk_u32 = u32::from(sdk_branch);
    let is_server_newer = server_u32.map(|value| value > sdk_u32).unwrap_or(false);
    let is_sdk_newer = server_u32.map(|value| sdk_u32 > value).unwrap_or(false);
    let error_message = if is_valid {
        None
    } else if let Some(server_branch_hex) = server_branch_hex.clone() {
        Some(format!(
            "Incompatible consensus branch: SDK expects {} but server reports {}.",
            sdk_branch_hex
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            server_branch_hex
        ))
    } else {
        Some("Server did not provide a recognizable consensus branch id.".to_string())
    };

    Ok(ConsensusBranchValidation {
        sdk_branch_id: sdk_branch_hex,
        server_branch_id: server_branch_hex,
        is_valid,
        has_server_branch,
        has_sdk_branch,
        is_server_newer,
        is_sdk_newer,
        error_message,
    })
}

pub async fn qortal_send_p2sh(
    wallet_id: WalletId,
    request: qortal_p2sh::QortalP2shSendRequest,
) -> Result<String> {
    qortal_p2sh::qortal_send_p2sh(wallet_id, request).await
}

pub async fn qortal_redeem_p2sh(
    wallet_id: WalletId,
    request: qortal_p2sh::QortalP2shRedeemRequest,
) -> Result<String> {
    qortal_p2sh::qortal_redeem_p2sh(wallet_id, request).await
}
#[cfg(test)]
mod api_regression_tests;
