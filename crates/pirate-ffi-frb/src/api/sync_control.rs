use super::*;
use parking_lot::RwLock;
use pirate_sync_lightd::{
    begin_sync_profile_session, record_sync_profile_failure, record_sync_profile_success,
    CancelToken, PerfCounters, SyncEngine, SyncProgress, SyncWorkload,
};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const CANCEL_SYNC_ENGINE_REQUEST_TIMEOUT: Duration = Duration::from_millis(350);

lazy_static::lazy_static! {
    /// Active sync sessions per wallet
    ///
    /// IMPORTANT: `SyncEngine` is not `Send + Sync` (it holds a rusqlite-backed storage sink),
    /// so we store sessions in a `parking_lot::RwLock` and never move them across threads.
    /// FRB calls are handled on a single thread by default.
    static ref SYNC_SESSIONS: Arc<RwLock<HashMap<WalletId, Arc<tokio::sync::Mutex<SyncSession>>>>> =
        Arc::new(RwLock::new(HashMap::new()));
    /// Live runtime handles for sync status reads without locking `SyncSession`.
    static ref SYNC_RUNTIME_HANDLES: Arc<RwLock<HashMap<WalletId, SyncRuntimeHandles>>> =
        Arc::new(RwLock::new(HashMap::new()));
    /// Last computed sync status snapshot per wallet (used as lock-free fallback).
    static ref SYNC_STATUS_SNAPSHOT_CACHE: Arc<RwLock<HashMap<WalletId, SyncStatus>>> =
        Arc::new(RwLock::new(HashMap::new()));
    /// Stable transaction list cache used while sync is mutating notes/spends.
    static ref TX_LIST_CACHE: Arc<RwLock<TxListCacheMap>> =
        Arc::new(RwLock::new(HashMap::new()));
    /// Stable balance cache used while sync is mutating notes/spends.
    static ref BALANCE_CACHE: Arc<RwLock<HashMap<WalletId, Balance>>> =
        Arc::new(RwLock::new(HashMap::new()));
    /// Prevent overlapping rescan setup for the same wallet.
    static ref RESCAN_IN_FLIGHT: Arc<RwLock<HashSet<WalletId>>> =
        Arc::new(RwLock::new(HashSet::new()));
    /// Wallets currently running an active rescan task.
    static ref RESCAN_ACTIVE: Arc<RwLock<HashSet<WalletId>>> =
        Arc::new(RwLock::new(HashSet::new()));
}

#[derive(Clone)]
struct SyncRuntimeHandles {
    progress: Arc<tokio::sync::RwLock<SyncProgress>>,
    perf: Arc<PerfCounters>,
}

type TxListCacheMap = HashMap<WalletId, Vec<TxInfo>>;

fn map_stage(stage: pirate_sync_lightd::SyncStage) -> crate::models::SyncStage {
    match stage {
        pirate_sync_lightd::SyncStage::Headers => crate::models::SyncStage::Headers,
        pirate_sync_lightd::SyncStage::Notes => crate::models::SyncStage::Notes,
        pirate_sync_lightd::SyncStage::Witness => crate::models::SyncStage::Witness,
        pirate_sync_lightd::SyncStage::Verify => crate::models::SyncStage::Verify,
        pirate_sync_lightd::SyncStage::Complete => crate::models::SyncStage::Verify,
    }
}

pub(super) fn get_cached_transactions(
    wallet_id: &WalletId,
    limit: Option<u32>,
) -> Option<Vec<TxInfo>> {
    let cached = TX_LIST_CACHE.read().get(wallet_id).cloned()?;
    if let Some(limit) = limit {
        let limit = limit as usize;
        if cached.len() > limit {
            return Some(cached.into_iter().take(limit).collect());
        }
    }
    Some(cached)
}

pub(super) fn put_cached_transactions(wallet_id: &WalletId, limit: Option<u32>, txs: &[TxInfo]) {
    let mut cache = TX_LIST_CACHE.write();
    match cache.get_mut(wallet_id) {
        Some(existing) => {
            let limit_usize = limit.map(|v| v as usize);
            let likely_truncated = limit_usize.is_some_and(|v| txs.len() >= v);
            if likely_truncated {
                let mut merged = txs.to_vec();
                let mut seen_txids: HashSet<String> =
                    merged.iter().map(|tx| tx.txid.clone()).collect();
                for tx in existing.iter() {
                    if seen_txids.insert(tx.txid.clone()) {
                        merged.push(tx.clone());
                    }
                }
                *existing = merged;
            } else {
                *existing = txs.to_vec();
            }
        }
        None => {
            cache.insert(wallet_id.clone(), txs.to_vec());
        }
    }
}

pub(super) fn get_cached_balance(wallet_id: &WalletId) -> Option<Balance> {
    BALANCE_CACHE.read().get(wallet_id).cloned()
}

pub(super) fn put_cached_balance(wallet_id: &WalletId, balance: &Balance) {
    BALANCE_CACHE
        .write()
        .insert(wallet_id.clone(), balance.clone());
}

pub(super) fn should_suppress_live_tx_reads(wallet_id: &WalletId) -> bool {
    let (mutating, _snapshot) = sync_mutation_snapshot(wallet_id);
    mutating
}

fn cache_sync_status(wallet_id: &WalletId, status: &SyncStatus) {
    SYNC_STATUS_SNAPSHOT_CACHE
        .write()
        .insert(wallet_id.clone(), status.clone());
}

fn get_cached_sync_status(wallet_id: &WalletId) -> Option<SyncStatus> {
    SYNC_STATUS_SNAPSHOT_CACHE.read().get(wallet_id).cloned()
}

fn clear_sync_runtime_cache(wallet_id: &WalletId) {
    SYNC_RUNTIME_HANDLES.write().remove(wallet_id);
    SYNC_STATUS_SNAPSHOT_CACHE.write().remove(wallet_id);
    TX_LIST_CACHE.write().remove(wallet_id);
    BALANCE_CACHE.write().remove(wallet_id);
}

fn load_spendability_status_internal(wallet_id: &str) -> Result<SpendabilityStatus> {
    let (db, _repo) = open_wallet_db_for(wallet_id)?;
    let storage = SpendabilityStateStorage::new(db);
    let state = storage.load_state()?;
    let scan_queue = ScanQueueStorage::new(db);
    let queue_has_work = scan_queue.next_found_note_range()?.is_some();

    let epoch_ok = state.anchor_height != 0 && state.validated_anchor_height >= state.anchor_height;
    let spendable = !state.rescan_required && !queue_has_work && epoch_ok;
    let reason_code = if state.rescan_required {
        SPENDABILITY_REASON_ERR_RESCAN_REQUIRED.to_string()
    } else if queue_has_work {
        SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED.to_string()
    } else if spendable {
        "OK".to_string()
    } else {
        SPENDABILITY_REASON_ERR_SYNC_FINALIZING.to_string()
    };

    Ok(SpendabilityStatus {
        spendable,
        rescan_required: state.rescan_required,
        target_height: state.target_height,
        anchor_height: state.anchor_height,
        validated_anchor_height: state.validated_anchor_height,
        repair_queued: queue_has_work,
        reason_code,
    })
}

fn require_spendability_ready(wallet_id: &str) -> Result<SpendabilityStatus> {
    let spendability = load_spendability_status_internal(wallet_id)?;
    if spendability.rescan_required {
        return Err(anyhow!(
            "{}: Wallet requires a full rescan before spending.",
            SPENDABILITY_REASON_ERR_RESCAN_REQUIRED
        ));
    }
    if spendability.repair_queued {
        return Err(anyhow!(
            "{}: Witness repair is queued. Let sync complete and retry.",
            SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED
        ));
    }
    if !spendability.spendable {
        return Err(anyhow!(
            "{}: Wallet spend anchor is not available yet. Let sync complete and retry.",
            SPENDABILITY_REASON_ERR_SYNC_FINALIZING
        ));
    }
    Ok(spendability)
}

pub(super) fn require_spendability_ready_with_sync_trigger(
    wallet_id: &WalletId,
) -> Result<SpendabilityStatus> {
    match require_spendability_ready(wallet_id) {
        Ok(status) => Ok(status),
        Err(e) => {
            let msg = e.to_string();
            if msg.starts_with(SPENDABILITY_REASON_ERR_SYNC_FINALIZING)
                || msg.starts_with(SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED)
            {
                maybe_trigger_compact_sync(wallet_id.clone());
            }
            Err(e)
        }
    }
}

fn mark_spendability_rescan_required(wallet_id: &str, reason_code: &str) {
    if let Ok((db, _repo)) = open_wallet_db_for(wallet_id) {
        let storage = SpendabilityStateStorage::new(db);
        if let Err(e) = storage.mark_rescan_required(reason_code) {
            tracing::warn!(
                "Failed to mark spendability rescan-required for {}: {}",
                wallet_id,
                e
            );
        }
    }
}

fn mark_spendability_sync_finalizing(wallet_id: &str, target_height: u64, anchor_height: u64) {
    if let Ok((db, _repo)) = open_wallet_db_for(wallet_id) {
        let storage = SpendabilityStateStorage::new(db);
        if let Err(e) = storage.mark_sync_finalizing(target_height, anchor_height) {
            tracing::warn!(
                "Failed to mark spendability sync-finalizing for {}: {}",
                wallet_id,
                e
            );
        }
    }
}

#[derive(Debug, Clone)]
struct SyncMutationSnapshot {
    reason: &'static str,
}

fn sync_mutation_snapshot(wallet_id: &WalletId) -> (bool, SyncMutationSnapshot) {
    let rescan_in_flight = RESCAN_IN_FLIGHT.read().contains(wallet_id);
    let rescan_active = is_rescan_active(wallet_id);
    if rescan_in_flight || rescan_active {
        return (
            true,
            SyncMutationSnapshot {
                reason: if rescan_in_flight {
                    "rescan_in_flight"
                } else {
                    "rescan_active"
                },
            },
        );
    }

    if let Some(handles) = SYNC_RUNTIME_HANDLES.read().get(wallet_id).cloned() {
        match handles.progress.try_read() {
            Ok(progress) => {
                let local_height = progress.current_height();
                let target_height = progress.target_height();
                let stage = map_stage(progress.stage());
                let mutating = local_height < target_height
                    || !matches!(stage, crate::models::SyncStage::Verify);
                return (
                    mutating,
                    SyncMutationSnapshot {
                        reason: if mutating {
                            "runtime_progress_mutating"
                        } else {
                            "runtime_progress_idle"
                        },
                    },
                );
            }
            Err(_) => {
                return (
                    true,
                    SyncMutationSnapshot {
                        reason: "runtime_progress_lock_busy",
                    },
                );
            }
        }
    }

    let session_arc = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(wallet_id).cloned()
    };
    let Some(session_arc) = session_arc else {
        return (
            false,
            SyncMutationSnapshot {
                reason: "no_session",
            },
        );
    };

    let try_lock = session_arc.try_lock();
    match try_lock {
        Ok(session) => {
            let is_running = session.is_running;
            let has_task = session.task.is_some() || session.startup_in_progress;
            let mutating = is_running || has_task;
            (
                mutating,
                SyncMutationSnapshot {
                    reason: if mutating {
                        "session_running_no_runtime"
                    } else {
                        "session_idle_no_runtime"
                    },
                },
            )
        }
        Err(_) => (
            true,
            SyncMutationSnapshot {
                reason: "session_lock_busy",
            },
        ),
    }
}

pub(super) fn maybe_trigger_compact_sync(wallet_id: WalletId) {
    if RESCAN_IN_FLIGHT.read().contains(&wallet_id) || is_rescan_active(&wallet_id) {
        return;
    }

    let session_running = {
        let sessions = SYNC_SESSIONS.read();
        if let Some(session_arc) = sessions.get(&wallet_id) {
            match session_arc.try_lock() {
                Ok(session) => {
                    session.is_running || session.task.is_some() || session.startup_in_progress
                }
                Err(_) => true,
            }
        } else {
            false
        }
    };
    if session_running {
        return;
    }

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = start_sync(wallet_id, SyncMode::Compact).await;
        });
    } else {
        std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                let _ = runtime.block_on(start_sync(wallet_id, SyncMode::Compact));
            }
        });
    }
}

pub(super) fn get_spendability_status(wallet_id: WalletId) -> Result<SpendabilityStatus> {
    ensure_not_decoy("Get spendability status")?;
    let status = load_spendability_status_internal(&wallet_id)?;
    let (sync_mutating, sync_mutation_snapshot) = sync_mutation_snapshot(&wallet_id);
    let epoch_ok =
        status.anchor_height != 0 && status.validated_anchor_height >= status.anchor_height;

    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_spendability_status_call","timestamp":{},"location":"api.rs:get_spendability_status","message":"get_spendability_status call","data":{{"wallet_id":"{}","spendable":{},"rescan_required":{},"repair_queued":{},"target_height":{},"anchor_height":{},"validated_anchor_height":{},"reason_code":"{}","epoch_ok":{},"sync_mutating":{},"sync_mutation_reason":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"S"}}"#,
            ts,
            wallet_id,
            status.spendable,
            status.rescan_required,
            status.repair_queued,
            status.target_height,
            status.anchor_height,
            status.validated_anchor_height,
            status.reason_code,
            epoch_ok,
            sync_mutating,
            sync_mutation_snapshot.reason,
        );
    });

    if !status.spendable
        || status.rescan_required
        || status.repair_queued
        || status.reason_code != "OK"
        || !epoch_ok
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_spendability_status","timestamp":{},"location":"api.rs:get_spendability_status","message":"spendability status","data":{{"wallet_id":"{}","spendable":{},"rescan_required":{},"repair_queued":{},"target_height":{},"anchor_height":{},"validated_anchor_height":{},"reason_code":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"S"}}"#,
                ts,
                wallet_id,
                status.spendable,
                status.rescan_required,
                status.repair_queued,
                status.target_height,
                status.anchor_height,
                status.validated_anchor_height,
                status.reason_code
            );
        });
    }
    Ok(status)
}

pub(super) async fn disconnect_foreground_sync_channels(reason: &'static str) {
    let sessions: Vec<(WalletId, Arc<tokio::sync::Mutex<SyncSession>>)> = {
        let sessions = SYNC_SESSIONS.read();
        sessions
            .iter()
            .map(|(wallet_id, session)| (wallet_id.clone(), Arc::clone(session)))
            .collect()
    };

    write_runtime_debug_event(
        "log_transport_sync_disconnect_start",
        "disconnect active sync channels",
        &format!(
            r#"{{"reason":"{}","session_count":{}}}"#,
            escape_json(reason),
            sessions.len()
        ),
    );

    for (wallet_id, session_arc) in sessions {
        let sync_opt = { session_arc.lock().await.sync.clone() };
        if let Some(sync) = sync_opt {
            let wallet_id_for_log = wallet_id.clone();
            let result = run_sync_engine_task(sync.clone(), move |engine| {
                Box::pin(async move {
                    engine.disconnect().await;
                    Ok(())
                })
            })
            .await;
            if let Err(e) = result {
                tracing::warn!(
                    "Failed to disconnect sync engine for {} after {}: {}",
                    wallet_id_for_log,
                    reason,
                    e
                );
                write_runtime_debug_event(
                    "log_transport_sync_disconnect_error",
                    "disconnect active sync channels failed",
                    &format!(
                        r#"{{"reason":"{}","wallet_id":"{}","error":"{}"}}"#,
                        escape_json(reason),
                        wallet_id_for_log,
                        escape_json(&format!("{}", e))
                    ),
                );
            }
        }
    }
}

pub(super) async fn foreground_sync_needs_work(wallet_id: &WalletId) -> Option<bool> {
    let session_arc = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(wallet_id).map(Arc::clone)
    }?;

    let sync_opt = { session_arc.lock().await.sync.clone() };
    if let Some(sync) = sync_opt {
        let progress_arc = {
            let engine = sync.clone().lock_owned().await;
            engine.progress()
        };
        let progress = progress_arc.read().await;
        Some(progress.current_height() < progress.target_height())
    } else {
        Some(false)
    }
}

#[flutter_rust_bridge::frb(ignore)]
struct SyncSession {
    sync: Option<Arc<tokio::sync::Mutex<SyncEngine>>>,
    cancelled: Option<CancelToken>,
    progress: Option<Arc<tokio::sync::RwLock<SyncProgress>>>,
    perf: Option<Arc<PerfCounters>>,
    last_status: SyncStatus,
    is_running: bool,
    startup_in_progress: bool,
    task: Option<tokio::task::JoinHandle<()>>,
    last_target_height_update: Option<std::time::Instant>,
    last_recovery_attempt: Option<std::time::Instant>,
}

impl Default for SyncSession {
    fn default() -> Self {
        Self {
            sync: None,
            cancelled: None,
            progress: None,
            perf: None,
            last_status: SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            },
            is_running: false,
            startup_in_progress: false,
            task: None,
            last_target_height_update: None,
            last_recovery_attempt: None,
        }
    }
}

pub(super) async fn start_sync(wallet_id: WalletId, mode: SyncMode) -> Result<()> {
    ensure_not_decoy("Sync")?;
    tracing::info!("Starting sync for wallet {} in mode {:?}", wallet_id, mode);

    if RESCAN_IN_FLIGHT.read().contains(&wallet_id) || is_rescan_active(&wallet_id) {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_start_sync_skip_rescan","timestamp":{},"location":"api.rs:start_sync","message":"start_sync skipped; rescan active","data":{{"wallet_id":"{}","mode":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                ts, wallet_id, mode
            );
        });
        return Ok(());
    }

    let session_arc_opt = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    };
    if let Some(session_arc) = session_arc_opt {
        let (is_running, has_task) = {
            let session = session_arc.lock().await;
            (
                session.is_running,
                session.task.is_some() || session.startup_in_progress,
            )
        };
        if is_running && has_task {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_start_sync_skip_running","timestamp":{},"location":"api.rs:start_sync","message":"start_sync skipped; already running","data":{{"wallet_id":"{}","mode":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                    ts, wallet_id, mode
                );
            });
            return Ok(());
        } else if is_running && !has_task {
            let mut session = session_arc.lock().await;
            session.is_running = false;
            session.startup_in_progress = false;
        }
    }
    log_orchard_address_samples(&wallet_id);
    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = uuid::Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>();
        let _ = writeln!(
            file,
            r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:2306","message":"start_sync wallet","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
            id, ts, wallet_id
        );
    });

    let wallet = get_wallet_meta(&wallet_id)?;
    let birthday_height = wallet.birthday_height;
    let start_height = {
        let resume_height_opt = open_wallet_db_for(&wallet_id).ok().and_then(|(db, _repo)| {
            let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
            sync_storage
                .load_sync_state()
                .ok()
                .map(|state| state.local_height as u32)
        });
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = uuid::Uuid::new_v4()
                .to_string()
                .chars()
                .take(8)
                .collect::<String>();
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:2319","message":"start_sync resume_height","data":{{"wallet_id":"{}","resume_height":"{:?}","birthday_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                id, ts, wallet_id, resume_height_opt, birthday_height
            );
        });
        match resume_height_opt {
            Some(resume_height) if resume_height > 0 => resume_height,
            _ => birthday_height,
        }
    };

    let should_mark_finalizing = load_spendability_status_internal(&wallet_id)
        .map(|state| !state.spendable && !state.rescan_required && !state.repair_queued)
        .unwrap_or(true);
    if should_mark_finalizing {
        mark_spendability_sync_finalizing(&wallet_id, start_height as u64, start_height as u64);
    }

    let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
    let endpoint_url = endpoint_config.url();
    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint_config,
        RetryConfig::default(),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(60),
    );
    let tls_enabled = endpoint_config.use_tls;
    let host = endpoint_config.host.clone();
    let tls_server_name = endpoint::tls_server_name(&endpoint_config);

    tracing::info!(
        "start_sync: Using endpoint {} (TLS: {}, transport: {:?})",
        endpoint_url,
        tls_enabled,
        client_config.transport
    );

    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let id = uuid::Uuid::new_v4()
            .to_string()
            .chars()
            .take(8)
            .collect::<String>();
        let _ = writeln!(
            file,
            r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:1964","message":"start_sync config","data":{{"endpoint":"{}","tls_enabled":{},"transport":"{:?}","host":"{}","tls_server_name":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
            id, ts, endpoint_url, tls_enabled, client_config.transport, host, tls_server_name
        );
    });

    let network_type = wallet_network_type(&wallet_id)?;
    let address_network_type = address_prefix_network_type(&wallet_id)?;
    let workload = match mode {
        SyncMode::Compact => SyncWorkload::Compact,
        SyncMode::Deep => SyncWorkload::Deep,
    };

    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let session_arc = {
        let mut sessions = SYNC_SESSIONS.write();
        sessions
            .entry(wallet_id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(SyncSession::default())))
            .clone()
    };

    {
        let mut session = session_arc.lock().await;
        if session.is_running {
            if session.task.is_none() {
                if session.startup_in_progress {
                    pirate_core::debug_log::with_locked_file(|file| {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_start_sync_skip_running","timestamp":{},"location":"api.rs:start_sync","message":"start_sync skipped; startup in progress","data":{{"wallet_id":"{}","mode":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                            ts, wallet_id, mode
                        );
                    });
                    return Ok(());
                }
                session.is_running = false;
            } else {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_start_sync_skip_running","timestamp":{},"location":"api.rs:start_sync","message":"start_sync skipped; already running","data":{{"wallet_id":"{}","mode":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
                        ts, wallet_id, mode
                    );
                });
                return Ok(());
            }
        }

        session.is_running = true;
        session.startup_in_progress = true;
        session.sync = None;
        session.cancelled = None;
        session.progress = None;
        session.perf = None;
        session.task = None;
        session.last_status = SyncStatus {
            local_height: start_height as u64,
            target_height: 0,
            percent: 0.0,
            eta: None,
            stage: crate::models::SyncStage::Headers,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        };
        session.last_target_height_update = None;
        session.last_recovery_attempt = None;
        cache_sync_status(&wallet_id, &session.last_status);
    }

    let selection = begin_sync_profile_session(workload);
    let sync_profile = selection.profile;
    let config = selection.config;
    tracing::info!(
        "start_sync: selected local sync profile {} for {:?} (batch_size={}, max_batch_size={}, target_bytes={}, max_bytes={}, prefetch_depth={}, workers={}, crash_downgraded={}, downgrade_steps={})",
        sync_profile.as_str(),
        workload,
        config.batch_size,
        config.max_batch_size,
        config.target_batch_bytes,
        config.max_batch_bytes,
        config.prefetch_queue_depth,
        config.max_parallel_decrypt,
        selection.crash_downgraded,
        selection.downgrade_steps
    );

    let client = LightClient::with_config(client_config);
    let sync = match SyncEngine::with_client_and_config(client, birthday_height, config)
        .with_wallet(
            wallet_id.clone(),
            db_key,
            master_key,
            network_type,
            address_network_type,
        ) {
        Ok(sync) => sync,
        Err(e) => {
            let mut session = session_arc.lock().await;
            session.is_running = false;
            session.startup_in_progress = false;
            clear_sync_runtime_cache(&wallet_id);
            record_sync_profile_failure();
            return Err(anyhow!("Failed to initialize sync engine: {}", e));
        }
    };
    let sync = Arc::new(Mutex::new(sync));
    let (progress, perf, cancel_flag) = {
        let engine = sync.clone().lock_owned().await;
        (
            engine.progress(),
            engine.perf_counters(),
            engine.cancel_flag(),
        )
    };
    let progress_handle = Arc::clone(&progress);
    let perf_handle = Arc::clone(&perf);

    {
        let mut session = session_arc.lock().await;
        session.sync = Some(Arc::clone(&sync));
        session.cancelled = Some(cancel_flag);
        session.progress = Some(progress);
        session.perf = Some(perf);
        session.last_status = SyncStatus {
            local_height: start_height as u64,
            target_height: 0,
            percent: 0.0,
            eta: None,
            stage: crate::models::SyncStage::Headers,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        };
        session.last_target_height_update = None;
        session.last_recovery_attempt = None;
        cache_sync_status(&wallet_id, &session.last_status);
    }
    SYNC_RUNTIME_HANDLES.write().insert(
        wallet_id.clone(),
        SyncRuntimeHandles {
            progress: progress_handle,
            perf: perf_handle,
        },
    );

    let wallet_id_for_task = wallet_id.clone();
    let session_arc_for_task = Arc::clone(&session_arc);
    let sync_for_task = Arc::clone(&sync);
    let task_handle = tokio::spawn(async move {
        let wallet_id_for_log = wallet_id_for_task.clone();
        let result = run_sync_engine_task(sync_for_task.clone(), move |engine| {
            Box::pin(async move {
                tracing::info!(
                    "Starting sync_from_birthday for wallet {}",
                    wallet_id_for_log
                );
                let result = engine
                    .sync_from_birthday()
                    .await
                    .map_err(anyhow::Error::from);
                if let Err(ref e) = result {
                    tracing::error!("Sync error in engine: {:?}", e);
                }
                result
            })
        })
        .await;

        let (progress_arc, perf_snapshot) = {
            let engine = sync_for_task.clone().lock_owned().await;
            (engine.progress(), engine.perf_counters().snapshot())
        };
        if result.is_ok() {
            record_sync_profile_success();
        } else {
            record_sync_profile_failure();
        }
        let status_opt = {
            let progress = progress_arc.read().await;
            let status = SyncStatus {
                local_height: progress.current_height(),
                target_height: progress.target_height(),
                percent: progress.percentage(),
                eta: progress.eta_seconds(),
                stage: map_stage(progress.stage()),
                last_checkpoint: progress.last_checkpoint(),
                blocks_per_second: perf_snapshot.blocks_per_second,
                notes_decrypted: perf_snapshot.notes_decrypted,
                last_batch_ms: perf_snapshot.avg_batch_ms,
            };
            tracing::debug!(
                "Sync status snapshot: local={}, target={}, stage={:?}, percent={:.2}%",
                status.local_height,
                status.target_height,
                status.stage,
                status.percent
            );
            Some(status)
        };

        let mut session = session_arc_for_task.lock().await;
        if let Some(status) = status_opt {
            session.last_status = status;
            cache_sync_status(&wallet_id_for_task, &session.last_status);
        }
        match &result {
            Ok(()) => {
                tracing::info!("Sync task exited for wallet {}", wallet_id_for_task);
                if let Ok(registry_db) = open_wallet_registry() {
                    if let Err(e) = touch_wallet_last_synced(&registry_db, &wallet_id_for_task) {
                        tracing::warn!(
                            "Failed to update last_synced_at for {}: {}",
                            wallet_id_for_task,
                            e
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!("Sync failed for wallet {}: {:?}", wallet_id_for_task, e);
                tracing::error!("Sync error details: {}", e);
                mark_spendability_sync_finalizing(&wallet_id_for_task, 0, 0);
            }
        }
        session.is_running = false;
        session.startup_in_progress = false;
        session.sync = None;
        session.cancelled = None;
        session.progress = None;
        session.perf = None;
        session.last_target_height_update = None;
        session.task = None;
        clear_sync_runtime_cache(&wallet_id_for_task);
    });
    {
        let mut session = session_arc.lock().await;
        session.task = Some(task_handle);
        session.startup_in_progress = false;
    }

    Ok(())
}

pub(super) fn sync_status(wallet_id: WalletId) -> Result<SyncStatus> {
    if is_decoy_mode_active() {
        return Ok(SyncStatus {
            local_height: 0,
            target_height: 0,
            percent: 0.0,
            eta: None,
            stage: SyncStage::Headers,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        });
    }
    let wallet_id_for_panic = wallet_id.clone();
    let result = std::panic::catch_unwind(|| sync_status_inner(wallet_id));
    match result {
        Ok(inner) => inner,
        Err(_) => {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sync_status_panic","timestamp":{},"location":"api.rs:2557","message":"sync_status panic","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                    ts, wallet_id_for_panic
                );
            });
            Ok(SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            })
        }
    }
}

fn schedule_target_height_update(
    sync: Arc<tokio::sync::Mutex<SyncEngine>>,
    session_arc: Arc<tokio::sync::Mutex<SyncSession>>,
) {
    if let Ok(mut session) = session_arc.try_lock() {
        session.last_target_height_update = Some(std::time::Instant::now());
    }

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let sync_clone = Arc::clone(&sync);
        let session_arc_clone = Arc::clone(&session_arc);
        handle.spawn(async move {
            let result = run_sync_engine_task(sync_clone, |engine| {
                Box::pin(async move {
                    engine
                        .update_target_height()
                        .await
                        .map_err(anyhow::Error::from)
                })
            })
            .await;
            if result.is_ok() {
                let mut session = session_arc_clone.lock().await;
                session.last_target_height_update = Some(std::time::Instant::now());
            }
        });
    } else {
        let sync_clone = Arc::clone(&sync);
        let session_arc_clone = Arc::clone(&session_arc);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(runtime) = runtime {
                runtime.block_on(async move {
                    let result = run_sync_engine_task(sync_clone, |engine| {
                        Box::pin(async move {
                            engine
                                .update_target_height()
                                .await
                                .map_err(anyhow::Error::from)
                        })
                    })
                    .await;
                    if result.is_ok() {
                        if let Ok(mut session) = session_arc_clone.try_lock() {
                            session.last_target_height_update = Some(std::time::Instant::now());
                        }
                    }
                });
            }
        });
    }
}

fn maybe_schedule_sync_recovery(
    wallet_id: &WalletId,
    session_arc: &Arc<tokio::sync::Mutex<SyncSession>>,
    status: &SyncStatus,
    is_running: bool,
    has_task: bool,
) {
    if has_task {
        return;
    }
    if status.target_height == 0 || status.local_height >= status.target_height {
        return;
    }
    if RESCAN_IN_FLIGHT.read().contains(wallet_id) || is_rescan_active(wallet_id) {
        return;
    }

    let should_attempt = if let Ok(mut session) = session_arc.try_lock() {
        if is_running {
            session.is_running = false;
        }
        let allow = session
            .last_recovery_attempt
            .map(|last| last.elapsed().as_secs() >= 15)
            .unwrap_or(true);
        if allow {
            session.last_recovery_attempt = Some(std::time::Instant::now());
        }
        allow
    } else {
        false
    };
    if !should_attempt {
        return;
    }

    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_sync_recovery","timestamp":{},"location":"api.rs:sync_status_inner","message":"sync recovery scheduled","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"stage":"{:?}","is_running":{},"has_task":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
            ts,
            wallet_id,
            status.local_height,
            status.target_height,
            status.stage,
            is_running,
            has_task
        );
    });

    let wallet_id_clone = wallet_id.clone();
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = start_sync(wallet_id_clone, SyncMode::Compact).await;
        });
    } else {
        std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                let _ = runtime.block_on(start_sync(wallet_id_clone, SyncMode::Compact));
            }
        });
    }
}

fn sync_status_inner(wallet_id: WalletId) -> Result<SyncStatus> {
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_sync_status_call","timestamp":{},"location":"api.rs:2557","message":"sync_status call","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                ts, wallet_id
            );
        });
    }
    let session_arc = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    };

    let session_arc = match session_arc {
        Some(session) => session,
        None => {
            {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_sync_status_session_none","timestamp":{},"location":"api.rs:2568","message":"sync_status no session in map","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        ts, wallet_id
                    );
                });
            }
            if let Some(status) = get_cached_sync_status(&wallet_id) {
                return Ok(status);
            }
            if let Ok((db, _repo)) = open_wallet_db_for(&wallet_id) {
                let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
                if let Ok(state) = sync_storage.load_sync_state() {
                    let percent = if state.target_height > 0 {
                        (state.local_height as f64 / state.target_height as f64) * 100.0
                    } else {
                        0.0
                    };
                    {
                        pirate_core::debug_log::with_locked_file(|file| {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_sync_status_state","timestamp":{},"location":"api.rs:2585","message":"sync_status returning from sync_state","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                                ts, wallet_id, state.local_height, state.target_height, percent
                            );
                        });
                    }
                    let status = SyncStatus {
                        local_height: state.local_height,
                        target_height: state.target_height,
                        percent,
                        eta: None,
                        stage: crate::models::SyncStage::Verify,
                        last_checkpoint: Some(state.last_checkpoint_height),
                        blocks_per_second: 0.0,
                        notes_decrypted: 0,
                        last_batch_ms: 0,
                    };
                    cache_sync_status(&wallet_id, &status);
                    return Ok(status);
                }
            }
            {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_sync_status_no_session","timestamp":{},"location":"api.rs:2590","message":"sync_status no session","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        ts, wallet_id
                    );
                });
            }
            pirate_core::debug_log::with_locked_file(|file| {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sync_status_default","timestamp":{},"location":"api.rs:2200","message":"sync_status returning default zeros","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"G"}}"#,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                    wallet_id
                );
            });
            let status = SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            };
            cache_sync_status(&wallet_id, &status);
            return Ok(status);
        }
    };

    let (
        progress_handle,
        perf_handle,
        sync_handle,
        last_status,
        last_target_update,
        is_running,
        has_task,
    ) = if let Ok(session) = session_arc.try_lock() {
        (
            session.progress.clone(),
            session.perf.clone(),
            session.sync.clone(),
            session.last_status.clone(),
            session.last_target_height_update,
            session.is_running,
            session.task.is_some() || session.startup_in_progress,
        )
    } else {
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sync_status_lock_busy","timestamp":{},"location":"api.rs:2610","message":"sync_status session lock busy","data":{{"wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                    ts, wallet_id
                );
            });
        }

        if let Some(handles) = SYNC_RUNTIME_HANDLES.read().get(&wallet_id).cloned() {
            if let Ok(progress) = handles.progress.try_read() {
                let perf = handles.perf.snapshot();
                let status = SyncStatus {
                    local_height: progress.current_height(),
                    target_height: progress.target_height(),
                    percent: progress.percentage(),
                    eta: progress.eta_seconds(),
                    stage: map_stage(progress.stage()),
                    last_checkpoint: progress.last_checkpoint(),
                    blocks_per_second: perf.blocks_per_second,
                    notes_decrypted: perf.notes_decrypted,
                    last_batch_ms: perf.avg_batch_ms,
                };
                cache_sync_status(&wallet_id, &status);
                return Ok(status);
            }
        }

        if let Some(status) = get_cached_sync_status(&wallet_id) {
            return Ok(status);
        }

        if let Ok((db, _repo)) = open_wallet_db_for(&wallet_id) {
            let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
            if let Ok(state) = sync_storage.load_sync_state() {
                let percent = if state.target_height > 0 {
                    (state.local_height as f64 / state.target_height as f64) * 100.0
                } else {
                    0.0
                };
                let status = SyncStatus {
                    local_height: state.local_height,
                    target_height: state.target_height,
                    percent,
                    eta: None,
                    stage: crate::models::SyncStage::Verify,
                    last_checkpoint: Some(state.last_checkpoint_height),
                    blocks_per_second: 0.0,
                    notes_decrypted: 0,
                    last_batch_ms: 0,
                };
                cache_sync_status(&wallet_id, &status);
                return Ok(status);
            }
        }

        let status = SyncStatus {
            local_height: 0,
            target_height: 0,
            percent: 0.0,
            eta: None,
            stage: crate::models::SyncStage::Headers,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        };
        cache_sync_status(&wallet_id, &status);
        return Ok(status);
    };

    if let Some(progress) = progress_handle {
        if let Ok(progress) = progress.try_read() {
            let perf_snapshot = perf_handle.as_ref().map(|perf| perf.snapshot());
            let should_update = last_target_update
                .map(|last| last.elapsed().as_secs() >= 10)
                .unwrap_or(true);
            if should_update {
                if let Some(sync) = sync_handle.as_ref() {
                    schedule_target_height_update(Arc::clone(sync), Arc::clone(&session_arc));
                }
            }
            let status = SyncStatus {
                local_height: progress.current_height(),
                target_height: progress.target_height(),
                percent: progress.percentage(),
                eta: progress.eta_seconds(),
                stage: map_stage(progress.stage()),
                last_checkpoint: progress.last_checkpoint(),
                blocks_per_second: perf_snapshot
                    .as_ref()
                    .map_or(0.0, |perf| perf.blocks_per_second),
                notes_decrypted: perf_snapshot
                    .as_ref()
                    .map_or(0, |perf| perf.notes_decrypted),
                last_batch_ms: perf_snapshot.as_ref().map_or(0, |perf| perf.avg_batch_ms),
            };

            if let Ok(mut session) = session_arc.try_lock() {
                session.last_status = status.clone();
            }
            cache_sync_status(&wallet_id, &status);
            maybe_schedule_sync_recovery(&wallet_id, &session_arc, &status, is_running, has_task);

            pirate_core::debug_log::with_locked_file(|file| {
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_sync_status","timestamp":{},"location":"api.rs:2166","message":"sync_status returning","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{},"stage":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                    wallet_id,
                    status.local_height,
                    status.target_height,
                    status.percent,
                    status.stage
                );
            });

            return Ok(status);
        }
    }

    if let Some(sync) = sync_handle {
        if let Ok(engine) = sync.try_lock() {
            if let Ok(progress) = engine.progress().try_read() {
                let perf = engine.perf_counters().snapshot();
                let target_height = progress.target_height();

                let should_update = last_target_update
                    .map(|last| last.elapsed().as_secs() >= 10)
                    .unwrap_or(true);

                if should_update {
                    schedule_target_height_update(Arc::clone(&sync), Arc::clone(&session_arc));
                }

                let status = SyncStatus {
                    local_height: progress.current_height(),
                    target_height,
                    percent: progress.percentage(),
                    eta: progress.eta_seconds(),
                    stage: map_stage(progress.stage()),
                    last_checkpoint: progress.last_checkpoint(),
                    blocks_per_second: perf.blocks_per_second,
                    notes_decrypted: perf.notes_decrypted,
                    last_batch_ms: perf.avg_batch_ms,
                };

                if let Ok(mut session) = session_arc.try_lock() {
                    session.last_status = status.clone();
                }
                cache_sync_status(&wallet_id, &status);
                maybe_schedule_sync_recovery(
                    &wallet_id,
                    &session_arc,
                    &status,
                    is_running,
                    has_task,
                );

                pirate_core::debug_log::with_locked_file(|file| {
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_sync_status","timestamp":{},"location":"api.rs:2166","message":"sync_status returning","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{},"stage":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis(),
                        wallet_id,
                        status.local_height,
                        status.target_height,
                        status.percent,
                        status.stage
                    );
                });

                return Ok(status);
            }
        }
    }

    pirate_core::debug_log::with_locked_file(|file| {
        let _ = writeln!(
            file,
            r#"{{"id":"log_sync_status_fallback","timestamp":{},"location":"api.rs:2192","message":"sync_status using fallback last_status","data":{{"wallet_id":"{}","local_height":{},"target_height":{},"percent":{},"stage":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"F"}}"#,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            wallet_id,
            last_status.local_height,
            last_status.target_height,
            last_status.percent,
            last_status.stage
        );
    });
    cache_sync_status(&wallet_id, &last_status);
    maybe_schedule_sync_recovery(&wallet_id, &session_arc, &last_status, is_running, has_task);
    Ok(last_status)
}

pub(super) fn get_last_checkpoint(wallet_id: WalletId) -> Result<Option<CheckpointInfo>> {
    if is_decoy_mode_active() {
        return Ok(None);
    }
    let sessions = SYNC_SESSIONS.read();

    let checkpoint_height_opt = if let Some(session_arc) = sessions.get(&wallet_id) {
        if let Ok(session) = session_arc.try_lock() {
            session.last_status.last_checkpoint.map(|h| h as u32)
        } else {
            None
        }
    } else {
        None
    };
    drop(sessions);

    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let manager = pirate_storage_sqlite::CheckpointManager::new(db.conn());

    let checkpoint = if let Some(height) = checkpoint_height_opt {
        manager
            .get_at_height(height)?
            .or_else(|| manager.get_latest().ok().flatten())
    } else {
        manager.get_latest()?
    };

    if let Some(checkpoint) = checkpoint {
        Ok(Some(CheckpointInfo {
            height: checkpoint.height,
            timestamp: checkpoint.timestamp,
        }))
    } else {
        Ok(None)
    }
}

struct RescanGuard {
    wallet_id: WalletId,
}

impl Drop for RescanGuard {
    fn drop(&mut self) {
        RESCAN_IN_FLIGHT.write().remove(&self.wallet_id);
    }
}

struct RescanActiveGuard {
    wallet_id: WalletId,
}

impl Drop for RescanActiveGuard {
    fn drop(&mut self) {
        RESCAN_ACTIVE.write().remove(&self.wallet_id);
    }
}

fn acquire_rescan_guard(wallet_id: &WalletId) -> Result<RescanGuard> {
    let mut in_flight = RESCAN_IN_FLIGHT.write();
    if in_flight.contains(wallet_id) {
        return Err(anyhow!(
            "Rescan is already being started for this wallet. Please wait a moment."
        ));
    }
    in_flight.insert(wallet_id.clone());
    Ok(RescanGuard {
        wallet_id: wallet_id.clone(),
    })
}

fn mark_rescan_active(wallet_id: &WalletId) -> RescanActiveGuard {
    RESCAN_ACTIVE.write().insert(wallet_id.clone());
    RescanActiveGuard {
        wallet_id: wallet_id.clone(),
    }
}

fn is_rescan_active(wallet_id: &WalletId) -> bool {
    RESCAN_ACTIVE.read().contains(wallet_id)
}

async fn wait_for_sync_stop(wallet_id: &WalletId, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let session_arc_opt = {
            let sessions = SYNC_SESSIONS.read();
            sessions.get(wallet_id).cloned()
        };

        let running = if let Some(session_arc) = session_arc_opt {
            match session_arc.try_lock() {
                Ok(session) => {
                    session.is_running || session.task.is_some() || session.startup_in_progress
                }
                Err(_) => true,
            }
        } else {
            false
        };

        if !running {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

pub(super) async fn rescan(wallet_id: WalletId, from_height: u32) -> Result<()> {
    ensure_not_decoy("Rescan")?;
    tracing::info!(
        "Rescanning wallet {} from height {}",
        wallet_id,
        from_height
    );
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_start","timestamp":{},"location":"api.rs:3050","message":"rescan start","data":{{"wallet_id":"{}","from_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id, from_height
            );
        });
    }

    if from_height == 0 {
        return Err(anyhow!("Invalid rescan height: must be > 0"));
    }
    let _rescan_guard = acquire_rescan_guard(&wallet_id)?;
    mark_spendability_rescan_required(&wallet_id, SPENDABILITY_REASON_ERR_RESCAN_REQUIRED);
    let mut effective_from_height = from_height;
    let truncate_height: u64;

    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3058","message":"rescan step","data":{{"wallet_id":"{}","step":"cancel_sync_start"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id
            );
        });
    }

    let was_syncing = is_sync_running(wallet_id.clone()).unwrap_or(false);
    if was_syncing {
        let cancel_result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            cancel_sync(wallet_id.clone()),
        )
        .await;
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let step = match &cancel_result {
                    Ok(Ok(())) => "cancel_sync_done",
                    Ok(Err(_)) => "cancel_sync_error",
                    Err(_) => "cancel_sync_timeout",
                };
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3076","message":"rescan step","data":{{"wallet_id":"{}","step":"{}","attempt":1}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id, step
                );
            });
        }

        let wait_ok = wait_for_sync_stop(&wallet_id, std::time::Duration::from_millis(500)).await;
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let step = if wait_ok {
                    "cancel_sync_wait_done"
                } else {
                    "cancel_sync_wait_timeout"
                };
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3090","message":"rescan step","data":{{"wallet_id":"{}","step":"{}","attempt":1}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id, step
                );
            });
        }

        if !wait_ok {
            tracing::warn!(
                "Rescan proceeding after timed out sync-stop wait for wallet {}",
                wallet_id
            );
        }
    } else {
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3090","message":"rescan step","data":{{"wallet_id":"{}","step":"cancel_sync_skipped"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            });
        }
    }

    let rescan_active_guard = mark_rescan_active(&wallet_id);

    if let Some(session_arc) = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    } {
        if let Ok(mut session) = session_arc.try_lock() {
            session.is_running = false;
            session.last_status = SyncStatus {
                local_height: 0,
                target_height: 0,
                percent: 0.0,
                eta: None,
                stage: crate::models::SyncStage::Headers,
                last_checkpoint: None,
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            };
            session.last_target_height_update = None;
        }
    }
    let removed_session = {
        let mut sessions = SYNC_SESSIONS.write();
        sessions.remove(&wallet_id)
    };
    if let Some(session_arc) = removed_session {
        if let Ok(mut session) = session_arc.try_lock() {
            if let Some(task) = session.task.take() {
                task.abort();
            }
            if let Some(cancelled) = session.cancelled.as_ref() {
                cancelled.cancel();
            }
            session.is_running = false;
        }
    }
    clear_sync_runtime_cache(&wallet_id);
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3105","message":"rescan step","data":{{"wallet_id":"{}","step":"session_removed"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id
            );
        });
    }

    {
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3119","message":"rescan step","data":{{"wallet_id":"{}","step":"get_passphrase_start"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            });
        }
        let passphrase = match app_passphrase() {
            Ok(passphrase) => passphrase,
            Err(e) => {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_rescan_passphrase_error","timestamp":{},"location":"api.rs:3070","message":"rescan passphrase error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts, wallet_id, e
                    );
                });
                return Err(e);
            }
        };
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3146","message":"rescan step","data":{{"wallet_id":"{}","step":"get_passphrase_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            });
        }
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3159","message":"rescan step","data":{{"wallet_id":"{}","step":"open_db_start"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            });
        }
        let (mut db, _key, _master_key) =
            open_wallet_db_with_passphrase(&wallet_id, &passphrase).map_err(|e| {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_rescan_open_db_error","timestamp":{},"location":"api.rs:3085","message":"rescan open db error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts,
                        wallet_id,
                        e
                    );
                });
                e
            })?;
        let repo = Repository::new(&db);
        if let Ok(Some(secret)) = repo.get_wallet_secret(&wallet_id) {
            if let Ok(unspent_notes) = repo.get_unspent_notes(secret.account_id) {
                let min_unspent_height = unspent_notes
                    .iter()
                    .filter_map(|note| u32::try_from(note.height).ok())
                    .min();
                if let Some(min_height) = min_unspent_height {
                    if effective_from_height > min_height {
                        tracing::info!(
                            "Adjusting rescan start for wallet {} from {} to {} to preserve witness recoverability for existing unspent notes",
                            wallet_id,
                            effective_from_height,
                            min_height
                        );
                        pirate_core::debug_log::with_locked_file(|file| {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_rescan_adjusted","timestamp":{},"location":"api.rs:rescan","message":"rescan start height adjusted","data":{{"wallet_id":"{}","requested_from_height":{},"effective_from_height":{},"min_unspent_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                                ts, wallet_id, from_height, min_height, min_height
                            );
                        });
                        effective_from_height = min_height;
                    }
                }
            }
        }
        truncate_height = effective_from_height.saturating_sub(1) as u64;
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3181","message":"rescan step","data":{{"wallet_id":"{}","step":"open_db_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            });
        }
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3194","message":"rescan step","data":{{"wallet_id":"{}","step":"truncate_start","truncate_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id, truncate_height
                );
            });
        }
        pirate_storage_sqlite::truncate_above_height(&mut db, truncate_height).map_err(|e| {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_truncate_error","timestamp":{},"location":"api.rs:3098","message":"rescan truncate error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts,
                    wallet_id,
                    e
                );
            });
            e
        })?;
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3219","message":"rescan step","data":{{"wallet_id":"{}","step":"truncate_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            });
        }
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3234","message":"rescan step","data":{{"wallet_id":"{}","step":"reset_state_start","reset_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts,
                    wallet_id,
                    effective_from_height.saturating_sub(1)
                );
            });
        }
        let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
        sync_storage
            .reset_sync_state(effective_from_height.saturating_sub(1) as u64)
            .map_err(|e| {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_rescan_reset_error","timestamp":{},"location":"api.rs:3112","message":"rescan reset error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts,
                        wallet_id,
                        e
                    );
                });
                e
            })?;
        let scan_queue = ScanQueueStorage::new(&db);
        scan_queue.clear_all().map_err(|e| {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_queue_reset_error","timestamp":{},"location":"api.rs:rescan","message":"rescan queue reset error","data":{{"wallet_id":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts,
                    wallet_id,
                    e
                );
            });
            e
        })?;
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3254","message":"rescan step","data":{{"wallet_id":"{}","step":"reset_state_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id
                );
            });
        }
    }
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_reset","timestamp":{},"location":"api.rs:3078","message":"rescan reset ok","data":{{"wallet_id":"{}","truncate_height":{},"reset_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts,
                wallet_id,
                truncate_height,
                effective_from_height.saturating_sub(1)
            );
        });
    }

    let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
    let endpoint_url = endpoint_config.url();
    let tls_enabled = endpoint_config.use_tls;
    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint_config,
        RetryConfig::default(),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(60),
    );

    tracing::info!(
        "rescan: Using endpoint {} (TLS: {}, transport: {:?})",
        endpoint_url,
        tls_enabled,
        client_config.transport
    );

    let network_type = wallet_network_type(&wallet_id)?;
    let address_network_type = address_prefix_network_type(&wallet_id)?;
    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let selection = begin_sync_profile_session(SyncWorkload::Rescan);
    let sync_profile = selection.profile;
    let config = selection.config;
    tracing::info!(
        "rescan: selected local sync profile {} (batch_size={}, max_batch_size={}, target_bytes={}, max_bytes={}, prefetch_depth={}, workers={}, crash_downgraded={}, downgrade_steps={})",
        sync_profile.as_str(),
        config.batch_size,
        config.max_batch_size,
        config.target_batch_bytes,
        config.max_batch_bytes,
        config.prefetch_queue_depth,
        config.max_parallel_decrypt,
        selection.crash_downgraded,
        selection.downgrade_steps
    );

    let client = LightClient::with_config(client_config);
    let sync = match SyncEngine::with_client_and_config(client, effective_from_height, config)
        .with_wallet(
            wallet_id.clone(),
            db_key,
            master_key,
            network_type,
            address_network_type,
        ) {
        Ok(sync) => sync,
        Err(e) => {
            record_sync_profile_failure();
            return Err(anyhow!("Failed to initialize sync engine: {}", e));
        }
    };
    let sync = Arc::new(Mutex::new(sync));
    let (progress, perf, cancel_flag) = {
        let engine = sync.clone().lock_owned().await;
        (
            engine.progress(),
            engine.perf_counters(),
            engine.cancel_flag(),
        )
    };
    let progress_handle = Arc::clone(&progress);
    let perf_handle = Arc::clone(&perf);
    let initial_status = SyncStatus {
        local_height: effective_from_height as u64,
        target_height: 0,
        percent: 0.0,
        eta: None,
        stage: crate::models::SyncStage::Headers,
        last_checkpoint: None,
        blocks_per_second: 0.0,
        notes_decrypted: 0,
        last_batch_ms: 0,
    };

    let rescan_session_arc = {
        let mut sessions = SYNC_SESSIONS.write();
        let session = Arc::new(tokio::sync::Mutex::new(SyncSession {
            sync: Some(Arc::clone(&sync)),
            cancelled: Some(cancel_flag),
            progress: Some(progress),
            perf: Some(perf),
            last_status: initial_status.clone(),
            is_running: true,
            startup_in_progress: true,
            task: None,
            last_target_height_update: None,
            last_recovery_attempt: None,
        }));
        sessions.insert(wallet_id.clone(), Arc::clone(&session));
        session
    };
    cache_sync_status(&wallet_id, &initial_status);
    mark_spendability_sync_finalizing(
        &wallet_id,
        effective_from_height as u64,
        effective_from_height as u64,
    );
    SYNC_RUNTIME_HANDLES.write().insert(
        wallet_id.clone(),
        SyncRuntimeHandles {
            progress: progress_handle,
            perf: perf_handle,
        },
    );
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_session","timestamp":{},"location":"api.rs:3142","message":"rescan session created","data":{{"wallet_id":"{}","from_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts, wallet_id, effective_from_height
            );
        });
    }

    let wallet_id_for_task = wallet_id.clone();
    let session_arc_for_task = Arc::clone(&rescan_session_arc);
    let task_handle = tokio::spawn(async move {
        let rescan_active_guard = rescan_active_guard;
        let sync_opt = { session_arc_for_task.lock().await.sync.clone() };

        if let Some(sync) = sync_opt {
            let result = run_sync_engine_task(sync.clone(), move |engine| {
                Box::pin(async move {
                    let tip_height = engine
                        .latest_block_height()
                        .await
                        .map_err(anyhow::Error::from)?;
                    let end_height = tip_height.max(effective_from_height as u64);
                    engine
                        .sync_range(effective_from_height as u64, Some(end_height))
                        .await
                        .map_err(anyhow::Error::from)
                })
            })
            .await;

            let (progress_arc, perf_snapshot) = {
                let engine = sync.clone().lock_owned().await;
                (engine.progress(), engine.perf_counters().snapshot())
            };
            let status_opt = {
                let progress = progress_arc.read().await;
                Some(SyncStatus {
                    local_height: progress.current_height(),
                    target_height: progress.target_height(),
                    percent: progress.percentage(),
                    eta: progress.eta_seconds(),
                    stage: map_stage(progress.stage()),
                    last_checkpoint: progress.last_checkpoint(),
                    blocks_per_second: perf_snapshot.blocks_per_second,
                    notes_decrypted: perf_snapshot.notes_decrypted,
                    last_batch_ms: perf_snapshot.avg_batch_ms,
                })
            };
            if result.is_ok() {
                record_sync_profile_success();
            } else {
                record_sync_profile_failure();
            }

            let mut session = session_arc_for_task.lock().await;
            if let Some(status) = status_opt {
                session.last_status = status;
                cache_sync_status(&wallet_id_for_task, &session.last_status);
            }
            let rescan_ok = result.is_ok();
            match &result {
                Ok(()) => {
                    tracing::info!("Rescan completed for wallet {}", wallet_id_for_task);
                    pirate_core::debug_log::with_locked_file(|file| {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_rescan_complete","timestamp":{},"location":"api.rs:rescan","message":"rescan complete","data":{{"wallet_id":"{}","from_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                            ts, wallet_id_for_task, effective_from_height
                        );
                    });
                }
                Err(e) => {
                    tracing::error!("Rescan failed for wallet {}: {:?}", wallet_id_for_task, e);
                    mark_spendability_rescan_required(
                        &wallet_id_for_task,
                        SPENDABILITY_REASON_ERR_RESCAN_REQUIRED,
                    );
                }
            }
            session.is_running = false;
            session.startup_in_progress = false;
            session.task = None;
            drop(session);
            clear_sync_runtime_cache(&wallet_id_for_task);
            drop(rescan_active_guard);

            if rescan_ok {
                maybe_trigger_compact_sync(wallet_id_for_task.clone());
            }
        } else {
            record_sync_profile_failure();
            let mut session = session_arc_for_task.lock().await;
            session.is_running = false;
            session.startup_in_progress = false;
            session.task = None;
            clear_sync_runtime_cache(&wallet_id_for_task);
            drop(rescan_active_guard);
        }
    });
    {
        let mut session = rescan_session_arc.lock().await;
        session.task = Some(task_handle);
        session.startup_in_progress = false;
    }
    Ok(())
}

pub(super) async fn cancel_sync_internal(
    wallet_id: WalletId,
    clear_engine_handle: bool,
) -> Result<()> {
    let session_arc_opt = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    };

    if let Some(session_arc) = session_arc_opt {
        let (cancel_opt, sync_opt, task_opt, previous_status) = {
            let mut session = session_arc.lock().await;
            (
                session.cancelled.clone(),
                session.sync.clone(),
                session.task.take(),
                session.last_status.clone(),
            )
        };
        let had_profile_session = task_opt.is_some() || cancel_opt.is_some() || sync_opt.is_some();
        let sync_for_cancel = sync_opt.clone();

        if let Some(task) = task_opt {
            task.abort();
            tracing::info!("Sync task aborted for wallet {}", wallet_id);
            {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_cancel_sync","timestamp":{},"location":"api.rs:cancel_sync","message":"cancel sync","data":{{"wallet_id":"{}","path":"task_abort"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts, wallet_id
                    );
                });
            }
        }

        if let Some(cancelled) = cancel_opt {
            cancelled.cancel();
            tracing::info!("Sync cancelled for wallet {}", wallet_id);
            {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_cancel_sync","timestamp":{},"location":"api.rs:3679","message":"cancel sync","data":{{"wallet_id":"{}","path":"cancel_flag"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts, wallet_id
                    );
                });
            }
        }
        if had_profile_session {
            record_sync_profile_failure();
        }

        if let Some(sync) = sync_for_cancel {
            let result = tokio::time::timeout(
                CANCEL_SYNC_ENGINE_REQUEST_TIMEOUT,
                run_sync_engine_task(sync.clone(), |engine| {
                    Box::pin(async move {
                        engine.cancel().await;
                        Ok(())
                    })
                }),
            )
            .await;
            match result {
                Ok(Ok(())) => {
                    tracing::info!("Sync engine cancel requested for wallet {}", wallet_id);
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to cancel sync for wallet {}: {}", wallet_id, e);
                }
                Err(_) => {
                    tracing::warn!(
                        "Timed out requesting sync engine cancel for wallet {} after {:?}",
                        wallet_id,
                        CANCEL_SYNC_ENGINE_REQUEST_TIMEOUT
                    );
                }
            }

            {
                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_cancel_sync","timestamp":{},"location":"api.rs:3696","message":"cancel sync","data":{{"wallet_id":"{}","path":"engine"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                        ts, wallet_id
                    );
                });
            }
        }

        let recovered_status = open_wallet_db_for(&wallet_id)
            .ok()
            .and_then(|(db, _repo)| {
                let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(db);
                sync_storage.load_sync_state().ok().map(|state| {
                    let mut target_height = state.target_height;
                    if target_height == 0 && state.local_height > 0 {
                        target_height = state.local_height;
                    }
                    let percent = if target_height > 0 {
                        ((state.local_height as f64 / target_height as f64) * 100.0)
                            .clamp(0.0, 100.0)
                    } else {
                        0.0
                    };
                    SyncStatus {
                        local_height: state.local_height,
                        target_height,
                        percent,
                        eta: None,
                        stage: if target_height > 0 && state.local_height >= target_height {
                            crate::models::SyncStage::Verify
                        } else {
                            crate::models::SyncStage::Headers
                        },
                        last_checkpoint: Some(state.last_checkpoint_height),
                        blocks_per_second: 0.0,
                        notes_decrypted: 0,
                        last_batch_ms: 0,
                    }
                })
            })
            .unwrap_or(previous_status);

        {
            let mut session = session_arc.lock().await;
            session.is_running = false;
            session.startup_in_progress = false;
            session.sync = if clear_engine_handle { None } else { sync_opt };
            session.cancelled = None;
            session.progress = None;
            session.perf = None;
            session.last_status = recovered_status.clone();
            session.last_target_height_update = None;
            session.last_recovery_attempt = None;
            session.task = None;
        }
        cache_sync_status(&wallet_id, &recovered_status);
        clear_sync_runtime_cache(&wallet_id);
        RESCAN_ACTIVE.write().remove(&wallet_id);
    }

    Ok(())
}

pub(super) async fn cancel_sync(wallet_id: WalletId) -> Result<()> {
    ensure_not_decoy("Cancel sync")?;
    cancel_sync_internal(wallet_id, true).await
}

pub(super) fn is_sync_running(wallet_id: WalletId) -> Result<bool> {
    if is_decoy_mode_active() {
        return Ok(false);
    }
    let session_arc_opt = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    };

    if let Some(session_arc) = session_arc_opt {
        if let Ok(session) = session_arc.try_lock() {
            return Ok(session.is_running || session.task.is_some() || session.startup_in_progress);
        }
        return Ok(true);
    }

    Ok(false)
}

pub(super) fn clear_wallet_sync_state(wallet_id: &WalletId) {
    {
        let mut sessions = SYNC_SESSIONS.write();
        sessions.remove(wallet_id);
    }
    clear_sync_runtime_cache(wallet_id);
}

pub(super) fn clear_passphrase_change_sync_state() {
    SYNC_SESSIONS.write().clear();
    SYNC_RUNTIME_HANDLES.write().clear();
    SYNC_STATUS_SNAPSHOT_CACHE.write().clear();
    TX_LIST_CACHE.write().clear();
}
