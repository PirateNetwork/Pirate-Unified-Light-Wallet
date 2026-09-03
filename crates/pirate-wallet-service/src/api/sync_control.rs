use super::*;
use parking_lot::RwLock;
use pirate_sync_lightd::{
    begin_guarded_sync_profile_session, monitor_sync_profile_initial_tip, CancelToken,
    PerfCounters, SyncEngine, SyncProfileSession, SyncProgress, SyncWorkload,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const CANCEL_SYNC_TASK_TIMEOUT: Duration = Duration::from_secs(30);

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
    /// Serializes sync startup, cancellation, and rescan setup for each wallet.
    static ref SYNC_OPERATION_LOCKS: Arc<RwLock<HashMap<WalletId, Arc<Mutex<()>>>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

fn sync_operation_lock(wallet_id: &WalletId) -> Arc<Mutex<()>> {
    if let Some(lock) = SYNC_OPERATION_LOCKS.read().get(wallet_id).cloned() {
        return lock;
    }

    SYNC_OPERATION_LOCKS
        .write()
        .entry(wallet_id.clone())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Stop any engine that may hold a stale account-key snapshot and keep sync
/// lifecycle operations serialized until the caller finishes its key mutation.
pub(super) async fn acquire_exclusive_key_import(
    wallet_id: &WalletId,
) -> Result<tokio::sync::OwnedMutexGuard<()>> {
    let operation_guard = sync_operation_lock(wallet_id).lock_owned().await;
    cancel_sync_session(wallet_id.clone(), true).await?;
    Ok(operation_guard)
}

/// True when a sync task ended because it was cancelled rather than because it failed.
fn is_cancelled_sync_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<pirate_sync_lightd::Error>()
        .is_some_and(|inner| matches!(inner, pirate_sync_lightd::Error::Cancelled))
}

#[derive(Clone)]
struct SyncRuntimeHandles {
    progress: Arc<tokio::sync::RwLock<SyncProgress>>,
    perf: Arc<PerfCounters>,
}

#[derive(Clone)]
struct TxListCacheEntry {
    transactions: Arc<[TxInfo]>,
    complete: bool,
}

type TxListCacheMap = HashMap<WalletId, TxListCacheEntry>;

fn map_stage(stage: pirate_sync_lightd::SyncStage) -> crate::models::SyncStage {
    match stage {
        pirate_sync_lightd::SyncStage::Headers => crate::models::SyncStage::Headers,
        pirate_sync_lightd::SyncStage::Notes => crate::models::SyncStage::Notes,
        pirate_sync_lightd::SyncStage::Witness => crate::models::SyncStage::Witness,
        pirate_sync_lightd::SyncStage::Verify => crate::models::SyncStage::Verify,
        pirate_sync_lightd::SyncStage::Preparing => crate::models::SyncStage::Preparing,
        pirate_sync_lightd::SyncStage::TreeState => crate::models::SyncStage::TreeState,
        pirate_sync_lightd::SyncStage::Complete => crate::models::SyncStage::Verify,
    }
}

pub(super) fn get_cached_transactions(
    wallet_id: &WalletId,
    limit: Option<u32>,
) -> Option<Vec<TxInfo>> {
    let cached = TX_LIST_CACHE.read().get(wallet_id).cloned()?;
    if limit.is_none() && !cached.complete {
        return None;
    }
    if let Some(limit) = limit {
        let limit = limit as usize;
        return Some(cached.transactions.iter().take(limit).cloned().collect());
    }
    Some(cached.transactions.to_vec())
}

pub(super) fn get_complete_transaction_snapshot(wallet_id: &WalletId) -> Option<Arc<[TxInfo]>> {
    let cache = TX_LIST_CACHE.read();
    let cached = cache.get(wallet_id)?;
    cached.complete.then(|| Arc::clone(&cached.transactions))
}

pub(super) fn put_cached_transactions(wallet_id: &WalletId, limit: Option<u32>, txs: &[TxInfo]) {
    // A limited query that fills its requested page may have omitted older
    // transactions. Treat it as incomplete unless it returned fewer entries
    // than requested.
    let complete = match limit {
        None => true,
        Some(limit) => txs.len() < limit as usize,
    };
    let transactions: Arc<[TxInfo]> = Arc::from(txs.to_vec());
    let mut cache = TX_LIST_CACHE.write();
    if !complete {
        if let Some(existing) = cache.get(wallet_id) {
            // Polling the same recent prefix must not downgrade an existing
            // complete snapshot. Any changed prefix invalidates it so the next
            // full-history read reloads a coherent snapshot from storage.
            if existing.complete
                && existing.transactions.len() >= transactions.len()
                && existing.transactions[..transactions.len()] == transactions[..]
            {
                return;
            }
        }
    }
    cache.insert(
        wallet_id.clone(),
        TxListCacheEntry {
            transactions,
            complete,
        },
    );
}

#[cfg(test)]
mod transaction_cache_tests {
    use super::*;

    fn tx(txid: &str, height: u32, amount: i64) -> TxInfo {
        TxInfo {
            txid: txid.to_string(),
            height: Some(height),
            timestamp: height as i64,
            amount,
            fee: 1_000,
            memo: None,
            confirmed: true,
            expired: false,
            expiry_height: None,
        }
    }

    fn wallet_id(label: &str) -> WalletId {
        format!("transaction-cache-test-{label}-{}", uuid::Uuid::new_v4())
    }

    #[test]
    fn full_history_read_rejects_an_exact_limit_snapshot() {
        let wallet_id = wallet_id("partial");
        let recent = vec![tx("new", 20, 2), tx("old", 10, 1)];

        put_cached_transactions(&wallet_id, Some(2), &recent);

        assert_eq!(get_cached_transactions(&wallet_id, None), None);
        assert_eq!(
            get_cached_transactions(&wallet_id, Some(1)),
            Some(vec![recent[0].clone()])
        );
        clear_wallet_data_caches(&wallet_id);
    }

    #[test]
    fn short_limited_result_is_known_to_be_complete() {
        let wallet_id = wallet_id("short");
        let history = vec![tx("new", 20, 2), tx("old", 10, 1)];

        put_cached_transactions(&wallet_id, Some(3), &history);

        assert_eq!(get_cached_transactions(&wallet_id, None), Some(history));
        clear_wallet_data_caches(&wallet_id);
    }

    #[test]
    fn unchanged_polling_prefix_preserves_a_complete_snapshot() {
        let wallet_id = wallet_id("unchanged");
        let history = vec![tx("new", 30, 3), tx("middle", 20, 2), tx("old", 10, 1)];

        put_cached_transactions(&wallet_id, None, &history);
        put_cached_transactions(&wallet_id, Some(2), &history[..2]);

        assert_eq!(get_cached_transactions(&wallet_id, None), Some(history));
        clear_wallet_data_caches(&wallet_id);
    }

    #[test]
    fn changed_polling_prefix_invalidates_a_complete_snapshot() {
        let wallet_id = wallet_id("changed");
        let history = vec![tx("new", 30, 3), tx("middle", 20, 2), tx("old", 10, 1)];
        let changed = vec![tx("newer", 40, 4), history[0].clone()];

        put_cached_transactions(&wallet_id, None, &history);
        put_cached_transactions(&wallet_id, Some(2), &changed);

        assert_eq!(get_cached_transactions(&wallet_id, None), None);
        assert_eq!(get_cached_transactions(&wallet_id, Some(2)), Some(changed));
        clear_wallet_data_caches(&wallet_id);
    }

    #[test]
    fn complete_snapshot_preserves_split_entries_for_one_txid() {
        let wallet_id = wallet_id("split");
        let history = vec![tx("split", 20, 5), tx("split", 20, -7)];

        put_cached_transactions(&wallet_id, None, &history);

        assert_eq!(get_cached_transactions(&wallet_id, None), Some(history));
        clear_wallet_data_caches(&wallet_id);
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

pub(super) fn clear_wallet_data_caches(wallet_id: &WalletId) {
    SYNC_STATUS_SNAPSHOT_CACHE.write().remove(wallet_id);
    TX_LIST_CACHE.write().remove(wallet_id);
    BALANCE_CACHE.write().remove(wallet_id);
}

fn load_spendability_status_internal(wallet_id: &str) -> Result<SpendabilityStatus> {
    let (db, _repo) = open_wallet_db_for(wallet_id)?;
    let storage = SpendabilityStateStorage::new(&db);
    let state = storage.load_state()?;
    let scan_queue = ScanQueueStorage::new(&db);
    let queue_has_work = scan_queue.next_found_note_range()?.is_some();
    // Queue rows may be consumed before witness reconstruction and anchor
    // validation finish. The persisted flag is therefore a durable latch, not
    // merely a cached view of the queue.
    let repair_pending = state.repair_queued || queue_has_work;

    let epoch_ok = state.anchor_height != 0 && state.validated_anchor_height >= state.anchor_height;
    let spendable = !state.rescan_required && !repair_pending && epoch_ok;
    let reason_code = if state.rescan_required {
        SPENDABILITY_REASON_ERR_RESCAN_REQUIRED.to_string()
    } else if repair_pending {
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
        repair_queued: repair_pending,
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
        let storage = SpendabilityStateStorage::new(&db);
        if let Err(e) = storage.mark_rescan_required(reason_code) {
            tracing::warn!(
                "Failed to mark spendability rescan-required for {}: {}",
                wallet_id,
                e
            );
        }
    }
}

fn record_known_sync_height(wallet_id: &str, height: u64) -> Result<()> {
    let (db, _repo) = open_wallet_db_for(wallet_id)?;
    SpendabilityStateStorage::new(&db)
        .record_known_sync_height(height)
        .map_err(Into::into)
}

fn mark_spendability_sync_interrupted(wallet_id: &str) -> Result<()> {
    let (db, _repo) = open_wallet_db_for(wallet_id)?;
    SpendabilityStateStorage::new(&db)
        .mark_sync_interrupted()
        .map_err(Into::into)
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
            let mutating = session.has_active_work();
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
                Ok(session) => session.has_active_work(),
                Err(_) => true,
            }
        } else {
            false
        }
    };
    if session_running {
        return;
    }

    crate::runtime::spawn_detached(async move {
        if let Err(error) = start_sync(wallet_id.clone(), SyncMode::Compact).await {
            tracing::error!(
                wallet_id = %wallet_id,
                error = ?error,
                "Failed to start automatic compact sync"
            );
        }
    });
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
    profile_session: Option<SyncProfileSession>,
    last_status: SyncStatus,
    is_running: bool,
    startup_in_progress: bool,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl SyncSession {
    fn has_active_work(&self) -> bool {
        self.startup_in_progress || self.task.as_ref().is_some_and(|task| !task.is_finished())
    }
}

impl Default for SyncSession {
    fn default() -> Self {
        Self {
            sync: None,
            cancelled: None,
            progress: None,
            perf: None,
            profile_session: None,
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
        }
    }
}

pub(super) async fn start_sync(wallet_id: WalletId, mode: SyncMode) -> Result<()> {
    ensure_not_decoy("Sync")?;
    let operation_lock = sync_operation_lock(&wallet_id);
    let _operation_guard = operation_lock.lock().await;
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
        let (is_running, has_work) = {
            let session = session_arc.lock().await;
            (session.is_running, session.has_active_work())
        };
        if has_work {
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
        } else if is_running {
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
    // Wallets restored by older releases may not yet have a discovery marker.
    // Only prepare lookahead automatically while their scan state is empty;
    // an already-synced wallet needs an explicit rescan so historical coverage
    // is not falsely inferred from a tip-only update.
    {
        let (db, repo) = open_wallet_db_for(&wallet_id)?;
        let local_height = pirate_storage_sqlite::SyncStateStorage::new(&db)
            .load_sync_state()?
            .local_height;
        if local_height == 0 {
            let secret = repo
                .get_wallet_secret(&wallet_id)?
                .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
            super::seed_account_discovery::prepare_legacy_sapling_account_discovery(
                &repo,
                &secret,
                birthday_height,
            )?;
        }
    }
    let start_height = {
        let resume_height_opt = open_wallet_db_for(&wallet_id).ok().and_then(|(db, _repo)| {
            let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
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

    record_known_sync_height(&wallet_id, u64::from(start_height))?;

    let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
    let endpoint_url = endpoint_config.url();
    let client_config = tunnel::light_client_config_for_endpoint(
        &endpoint_config,
        RetryConfig::default(),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(180),
    );
    let tls_enabled = endpoint_config.use_tls;
    let host = endpoint_config.host.clone();
    let tls_server_name = endpoint::tls_server_name(&endpoint_config);
    let automatic_failover = endpoint_config.automatic_failover;
    let failover_count = client_config.failover_endpoints.len();

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
            r#"{{"id":"log_{}","timestamp":{},"location":"api.rs:1964","message":"start_sync config","data":{{"endpoint":"{}","tls_enabled":{},"transport":"{:?}","host":"{}","tls_server_name":"{:?}","automatic_failover":{},"failover_count":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
            id,
            ts,
            endpoint_url,
            tls_enabled,
            client_config.transport,
            host,
            tls_server_name,
            automatic_failover,
            failover_count
        );
    });

    let network_type = wallet_network_type(&wallet_id)?;
    let address_network_type = address_prefix_network_type(&wallet_id)?;
    let workload = match mode {
        SyncMode::Compact => SyncWorkload::Compact,
        SyncMode::Deep => SyncWorkload::Deep,
    };

    let db_path = wallet_db_path_for(&wallet_id)?;
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
        session.profile_session = None;
        session.task = None;
        session.last_status = SyncStatus {
            local_height: start_height as u64,
            target_height: 0,
            percent: 0.0,
            eta: None,
            stage: crate::models::SyncStage::Preparing,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        };
        cache_sync_status(&wallet_id, &session.last_status);
    }

    let (selection, profile_session) = begin_guarded_sync_profile_session(workload);
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
        .with_wallet_at_path(
            wallet_id.clone(),
            db_path,
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
            profile_session.record_failure();
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
        // Publish the durable resume height before the first network RPC. If a
        // private circuit stalls, status polling must not replace a known
        // wallet height with the SyncProgress default of 0/0.
        let progress = progress.write().await;
        progress.set_current(start_height as u64);
        progress.set_stage(pirate_sync_lightd::progress::SyncStage::Preparing);
    }
    tokio::spawn(monitor_sync_profile_initial_tip(
        profile_session.clone(),
        Arc::clone(&progress),
    ));

    {
        let mut session = session_arc.lock().await;
        session.sync = Some(Arc::clone(&sync));
        session.cancelled = Some(cancel_flag);
        session.progress = Some(progress);
        session.perf = Some(perf);
        session.profile_session = Some(profile_session.clone());
        session.last_status = SyncStatus {
            local_height: start_height as u64,
            target_height: 0,
            percent: 0.0,
            eta: None,
            stage: crate::models::SyncStage::Preparing,
            last_checkpoint: None,
            blocks_per_second: 0.0,
            notes_decrypted: 0,
            last_batch_ms: 0,
        };
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
    let profile_session_for_task = profile_session.clone();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let task_handle = tokio::spawn(async move {
        if start_rx.await.is_err() {
            return;
        }
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
        if result.is_err() {
            progress_arc
                .write()
                .await
                .set_stage(pirate_sync_lightd::progress::SyncStage::Verify);
        }
        if result.is_ok() {
            profile_session_for_task.record_success();
        } else {
            profile_session_for_task.record_failure();
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
                if let Err(error) =
                    super::seed_account_discovery::finalize_legacy_sapling_account_discovery(
                        &wallet_id_for_task,
                    )
                {
                    tracing::warn!(
                        "Could not finalize seed account discovery for {}: {}",
                        wallet_id_for_task,
                        error
                    );
                }
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
            Err(e) if is_cancelled_sync_error(e) => {
                // Cancellation is an orderly stop, not a sync failure. Wiping the
                // persisted spendability heights here would zero the known chain tip
                // that `import_spending_key_verified` validates birthdays against,
                // and a completed one-shot sync ends in a self-cancel, so the tip
                // would otherwise become unknown moments after validation.
                tracing::info!("Sync task cancelled for wallet {}", wallet_id_for_task);
            }
            Err(e) => {
                tracing::error!("Sync failed for wallet {}: {:?}", wallet_id_for_task, e);
                tracing::error!("Sync error details: {}", e);
                if let Err(state_error) = mark_spendability_sync_interrupted(&wallet_id_for_task) {
                    tracing::warn!(
                        "Failed to preserve spendability state after sync error for {}: {}",
                        wallet_id_for_task,
                        state_error
                    );
                }
            }
        }
        session.is_running = false;
        session.startup_in_progress = false;
        session.sync = None;
        session.cancelled = None;
        session.progress = None;
        session.perf = None;
        session.profile_session = None;
        session.task = None;
        clear_sync_runtime_cache(&wallet_id_for_task);
    });
    {
        let mut session = session_arc.lock().await;
        session.task = Some(task_handle);
        session.startup_in_progress = false;
    }
    if start_tx.send(()).is_err() {
        let mut session = session_arc.lock().await;
        session.is_running = false;
        session.startup_in_progress = false;
        session.task = None;
        clear_sync_runtime_cache(&wallet_id);
        profile_session.record_failure();
        return Err(anyhow!(
            "Failed to start sync task for wallet {}",
            wallet_id
        ));
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
                let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
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

    let (progress_handle, perf_handle, sync_handle, last_status) = if let Ok(session) =
        session_arc.try_lock()
    {
        (
            session.progress.clone(),
            session.perf.clone(),
            session.sync.clone(),
            session.last_status.clone(),
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
            let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
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
    clear_phase_status_on_drop: bool,
}

impl RescanGuard {
    fn mark_phase_published(&mut self) {
        self.clear_phase_status_on_drop = true;
    }

    fn transfer_status_to_session(&mut self) {
        self.clear_phase_status_on_drop = false;
    }
}

impl Drop for RescanGuard {
    fn drop(&mut self) {
        RESCAN_IN_FLIGHT.write().remove(&self.wallet_id);
        if self.clear_phase_status_on_drop {
            SYNC_STATUS_SNAPSHOT_CACHE.write().remove(&self.wallet_id);
        }
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
        clear_phase_status_on_drop: false,
    })
}

fn rescan_phase_status(
    local_height: u64,
    prior_target_height: u64,
    stage: crate::models::SyncStage,
) -> SyncStatus {
    let target_height = if prior_target_height > local_height {
        prior_target_height
    } else {
        0
    };
    let percent = if target_height > 0 {
        ((local_height as f64 / target_height as f64) * 100.0).clamp(0.0, 99.9)
    } else {
        0.0
    };

    SyncStatus {
        local_height,
        target_height,
        percent,
        eta: None,
        stage,
        last_checkpoint: None,
        blocks_per_second: 0.0,
        notes_decrypted: 0,
        last_batch_ms: 0,
    }
}

fn rescan_spendability_heights(local_height: u64, prior_target_height: u64) -> (u64, u64) {
    (prior_target_height.max(local_height), local_height)
}

fn select_rescan_prior_target(cached_target_height: u64, durable_target_height: u64) -> u64 {
    cached_target_height.max(durable_target_height)
}

/// Keep rollback durability separate from the height presented to SyncEngine.
/// The engine first bootstraps the requested frontier from lightwalletd and
/// falls back to the retained checkpoint only when a partial replay is valid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RescanStartPlan {
    requested_sync_from_height: u64,
    retained_tree_height: u64,
    fallback_replay_from_height: u64,
}

fn is_wallet_data_encryption_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<pirate_storage_sqlite::Error>(),
            Some(pirate_storage_sqlite::Error::Encryption(_))
        )
    })
}

fn rescan_storage_access_error(
    wallet_id: &WalletId,
    operation: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    tracing::error!(
        "Rescan {} failed for wallet {} before completion: {:#}",
        operation,
        wallet_id,
        error
    );
    if is_wallet_data_encryption_error(&error) {
        anyhow!(concat!(
            "ERR_WALLET_DATA_UNAVAILABLE: Wallet data could not be decrypted. ",
            "Lock and unlock the app with your app passphrase, then retry. ",
            "If this continues, keep the existing wallet data intact and restore ",
            "the seed into a new wallet profile."
        ))
    } else {
        anyhow!("Could not {} before rescan: {}", operation, error)
    }
}

fn validate_rescan_storage(wallet_id: &WalletId, passphrase: &str) -> Result<()> {
    let (db, _key, _master_key) = open_wallet_db_with_passphrase(wallet_id, passphrase)?;
    let repo = Repository::new(&db);
    let secret = repo
        .get_wallet_secret(wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;
    repo.validate_account_encryption(secret.account_id)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RequiredKeyReplay {
    from_height: u32,
    import_generation: u64,
}

fn required_key_replay(
    state: &pirate_storage_sqlite::SpendabilityStateRow,
) -> Result<Option<RequiredKeyReplay>> {
    if !state.rescan_required || state.required_rescan_from_height == 0 {
        return Ok(None);
    }

    let from_height = u32::try_from(state.required_rescan_from_height)
        .map_err(|_| anyhow!("Required rescan height exceeds the supported chain height"))?;
    Ok(Some(RequiredKeyReplay {
        from_height,
        import_generation: state.key_import_generation,
    }))
}

fn clamp_rescan_start(requested_from_height: u32, required: Option<RequiredKeyReplay>) -> u32 {
    required
        .map(|pending| requested_from_height.min(pending.from_height))
        .unwrap_or(requested_from_height)
}

fn complete_required_key_replay(
    wallet_id: &WalletId,
    requirement: RequiredKeyReplay,
    replayed_from_height: u64,
) -> Result<bool> {
    let (db, _repo) = open_wallet_db_for(wallet_id)?;
    SpendabilityStateStorage::new(&db)
        .complete_required_rescan(requirement.import_generation, replayed_from_height)
        .map_err(Into::into)
}

fn plan_rescan_start(effective_from_height: u32, retained_tree_height: u64) -> RescanStartPlan {
    let requested_sync_from_height = u64::from(effective_from_height).max(1);
    let retained_tree_height =
        retained_tree_height.min(requested_sync_from_height.saturating_sub(1));
    let fallback_replay_from_height = retained_tree_height.saturating_add(1).max(1);

    RescanStartPlan {
        requested_sync_from_height,
        retained_tree_height,
        fallback_replay_from_height,
    }
}

#[cfg(test)]
mod rescan_start_plan_tests {
    use super::*;

    #[test]
    fn cancellation_is_not_classified_as_a_sync_failure() {
        let cancelled = anyhow::Error::from(pirate_sync_lightd::Error::Cancelled);
        assert!(is_cancelled_sync_error(&cancelled));

        // The engine-task select wraps the typed cancellation with context;
        // classification must see through the context chain.
        let wrapped =
            anyhow::Error::from(pirate_sync_lightd::Error::Cancelled).context("Sync cancelled");
        assert!(is_cancelled_sync_error(&wrapped));

        let network = anyhow::Error::from(pirate_sync_lightd::Error::Network("down".to_string()));
        assert!(!is_cancelled_sync_error(&network));

        let unrelated = anyhow::anyhow!("some other failure");
        assert!(!is_cancelled_sync_error(&unrelated));
    }

    #[test]
    fn encryption_failures_use_an_actionable_rescan_error() {
        let cause = pirate_storage_sqlite::Error::Encryption("aead::Error".to_string());
        let error = anyhow::Error::new(cause);

        assert!(is_wallet_data_encryption_error(&error));

        let mapped = rescan_storage_access_error(
            &"test-wallet".to_string(),
            "verify encrypted wallet data",
            error,
        )
        .to_string();

        assert!(mapped.starts_with("ERR_WALLET_DATA_UNAVAILABLE:"));
        assert!(mapped.contains("Lock and unlock the app"));
        assert!(!mapped.contains("aead::Error"));
    }

    #[test]
    fn ordinary_rescan_storage_failures_preserve_the_operation_context() {
        let error = anyhow!("database is busy");

        assert!(!is_wallet_data_encryption_error(&error));

        let mapped = rescan_storage_access_error(
            &"test-wallet".to_string(),
            "verify encrypted wallet data",
            error,
        )
        .to_string();

        assert_eq!(
            mapped,
            "Could not verify encrypted wallet data before rescan: database is busy"
        );
    }

    #[test]
    fn pruned_checkpoint_bootstraps_at_the_requested_rescan_height() {
        let plan = plan_rescan_start(2_400_000, 0);

        assert_eq!(plan.requested_sync_from_height, 2_400_000);
        assert_eq!(plan.retained_tree_height, 0);
        assert_eq!(plan.fallback_replay_from_height, 1);
    }

    #[test]
    fn partial_tree_replay_remains_available_without_replacing_the_requested_start() {
        let plan = plan_rescan_start(2_400_000, 152_854);

        assert_eq!(plan.requested_sync_from_height, 2_400_000);
        assert_eq!(plan.retained_tree_height, 152_854);
        assert_eq!(plan.fallback_replay_from_height, 152_855);
    }

    #[test]
    fn exact_checkpoint_needs_no_earlier_fallback() {
        let plan = plan_rescan_start(2_400_000, 2_399_999);

        assert_eq!(plan.requested_sync_from_height, 2_400_000);
        assert_eq!(plan.retained_tree_height, 2_399_999);
        assert_eq!(plan.fallback_replay_from_height, 2_400_000);
    }

    #[test]
    fn rescan_rewind_preserves_a_known_chain_tip() {
        assert_eq!(
            rescan_spendability_heights(152_849, 152_860),
            (152_860, 152_849)
        );
        assert_eq!(rescan_spendability_heights(152_849, 0), (152_849, 152_849));
        assert_eq!(select_rescan_prior_target(0, 152_860), 152_860);
        assert_eq!(select_rescan_prior_target(152_861, 152_860), 152_861);
    }

    #[test]
    fn verified_key_floor_clamps_a_later_rescan_request() {
        let state = pirate_storage_sqlite::SpendabilityStateRow {
            required_rescan_from_height: 400,
            key_import_generation: 2,
            ..Default::default()
        };
        let required = required_key_replay(&state).unwrap();

        assert_eq!(
            required,
            Some(RequiredKeyReplay {
                from_height: 400,
                import_generation: 2,
            })
        );
        assert_eq!(clamp_rescan_start(700, required), 400);
        assert_eq!(clamp_rescan_start(300, required), 300);
    }

    #[test]
    fn ordinary_rescan_state_does_not_introduce_an_import_floor() {
        let state = pirate_storage_sqlite::SpendabilityStateRow::default();
        let required = required_key_replay(&state).unwrap();

        assert_eq!(required, None);
        assert_eq!(clamp_rescan_start(700, required), 700);
    }
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

async fn wait_for_sync_stop(wallet_id: &WalletId, timeout: Duration) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let session_arc_opt = {
            let sessions = SYNC_SESSIONS.read();
            sessions.get(wallet_id).cloned()
        };

        let running = if let Some(session_arc) = session_arc_opt {
            let session = session_arc.lock().await;
            session.has_active_work()
        } else {
            false
        };

        if !running {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "Timed out waiting {:?} for wallet {} sync to stop; rescan was not started",
                timeout,
                wallet_id
            ));
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
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
    let mut rescan_guard = acquire_rescan_guard(&wallet_id)?;
    let operation_lock = sync_operation_lock(&wallet_id);
    let _operation_guard = operation_lock.lock().await;
    let passphrase = app_passphrase()?;
    validate_rescan_storage(&wallet_id, &passphrase).map_err(|error| {
        rescan_storage_access_error(&wallet_id, "verify encrypted wallet data", error)
    })?;
    let required_key_replay = {
        let (db, _repo) = open_wallet_db_for(&wallet_id)?;
        let state = SpendabilityStateStorage::new(&db).load_state()?;
        required_key_replay(&state)?
    };
    let mut effective_from_height = clamp_rescan_start(from_height, required_key_replay);
    if effective_from_height != from_height {
        tracing::info!(
            "Adjusting rescan start for wallet {} from {} to durable imported-key floor {}",
            wallet_id,
            from_height,
            effective_from_height
        );
    }
    let truncate_height: u64;
    let rescan_start_plan: RescanStartPlan;
    let mut historical_sapling_mark_positions = Vec::new();
    let mut historical_ironwood_mark_positions = Vec::new();

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
        let cancel_result = cancel_sync_session(wallet_id.clone(), true).await;
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let step = match &cancel_result {
                    Ok(()) => "cancel_sync_done",
                    Err(_) => "cancel_sync_error",
                };
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:3076","message":"rescan step","data":{{"wallet_id":"{}","step":"{}","attempt":1}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    ts, wallet_id, step
                );
            });
        }
        cancel_result?;
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

    // Snapshot the target only after any running sync has been cancelled and
    // joined. The durable state is authoritative across controller restarts;
    // the runtime cache can only raise that target, never replace it.
    let cached_target_height = get_cached_sync_status(&wallet_id)
        .map(|status| status.target_height)
        .unwrap_or(0);
    let durable_target_height = {
        let (db, _repo) = open_wallet_db_for(&wallet_id)?;
        SpendabilityStateStorage::new(&db)
            .load_state()?
            .target_height
    };
    let prior_target_height =
        select_rescan_prior_target(cached_target_height, durable_target_height);

    {
        {
            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_rescan_step","timestamp":{},"location":"api.rs:rescan","message":"rescan step","data":{{"wallet_id":"{}","step":"storage_preflight_done"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
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
        let (db, _key, _master_key) =
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
                rescan_storage_access_error(&wallet_id, "open encrypted wallet data", e)
            })?;
        let repo = Repository::new(&db);
        if let Ok(Some(secret)) = repo.get_wallet_secret(&wallet_id) {
            super::seed_account_discovery::prepare_legacy_sapling_account_discovery(
                &repo,
                &secret,
                get_wallet_meta(&wallet_id)
                    .map(|wallet| wallet.birthday_height)
                    .unwrap_or(effective_from_height),
            )?;
            match repo.get_historical_note_positions(secret.account_id) {
                Ok(positions) => {
                    for (note_type, position) in positions {
                        match note_type {
                            StoredNoteType::Sapling => {
                                historical_sapling_mark_positions.push(position)
                            }
                            StoredNoteType::Ironwood => {
                                historical_ironwood_mark_positions.push(position)
                            }
                        }
                    }
                    pirate_core::debug_log::with_locked_file(|file| {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_rescan_mark_hints","timestamp":{},"location":"api.rs:rescan","message":"historical rescan mark hints captured","data":{{"wallet_id":"{}","sapling_count":{},"ironwood_count":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"P"}}"#,
                            ts,
                            wallet_id,
                            historical_sapling_mark_positions.len(),
                            historical_ironwood_mark_positions.len()
                        );
                    });
                }
                Err(error) => {
                    tracing::warn!(
                        "Could not load historical note-position hints for wallet {}: {}",
                        wallet_id,
                        error
                    );
                }
            }
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
        let (spendability_target, spendability_anchor) =
            rescan_spendability_heights(truncate_height, prior_target_height);
        SpendabilityStateStorage::new(&db).begin_rescan(
            spendability_target,
            spendability_anchor,
            SPENDABILITY_REASON_ERR_RESCAN_REQUIRED,
        )?;
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
        let tree_replay_height =
            pirate_storage_sqlite::truncate_above_height(&db, truncate_height).map_err(
                |e| {
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
                    rescan_storage_access_error(
                        &wallet_id,
                        "rewind encrypted wallet state",
                        e.into(),
                    )
                },
            )?;
        rescan_start_plan = plan_rescan_start(effective_from_height, tree_replay_height);
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
                    ts, wallet_id, tree_replay_height
                );
            });
        }
        let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
        sync_storage
            .reset_sync_state(rescan_start_plan.retained_tree_height)
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
        }
    }
    let removed_session = {
        let mut sessions = SYNC_SESSIONS.write();
        sessions.remove(&wallet_id)
    };
    if let Some(session_arc) = removed_session {
        if let Ok(mut session) = session_arc.try_lock() {
            session.is_running = false;
            session.startup_in_progress = false;
            session.profile_session = None;
            session.task = None;
        }
    }
    clear_sync_runtime_cache(&wallet_id);
    let preparing_status = rescan_phase_status(
        u64::from(effective_from_height.saturating_sub(1)),
        prior_target_height,
        crate::models::SyncStage::Preparing,
    );
    cache_sync_status(&wallet_id, &preparing_status);
    rescan_guard.mark_phase_published();
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
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let _ = writeln!(
                file,
                r#"{{"id":"log_rescan_reset","timestamp":{},"location":"api.rs:3078","message":"rescan reset ok","data":{{"wallet_id":"{}","truncate_height":{},"reset_height":{},"requested_sync_from_height":{},"fallback_replay_from_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts,
                wallet_id,
                truncate_height,
                rescan_start_plan.retained_tree_height,
                rescan_start_plan.requested_sync_from_height,
                rescan_start_plan.fallback_replay_from_height
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
        std::time::Duration::from_secs(180),
    );
    let automatic_failover = endpoint_config.automatic_failover;
    let failover_count = client_config.failover_endpoints.len();

    tracing::info!(
        "rescan: Using endpoint {} (TLS: {}, transport: {:?}, automatic failover: {}, alternates: {})",
        endpoint_url,
        tls_enabled,
        client_config.transport,
        automatic_failover,
        failover_count
    );

    pirate_core::debug_log::with_locked_file(|file| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(
            file,
            r#"{{"id":"log_rescan_endpoint_config","timestamp":{},"location":"api.rs:rescan","message":"rescan endpoint config","data":{{"tls_enabled":{},"transport":"{:?}","automatic_failover":{},"failover_count":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"C"}}"#,
            ts, tls_enabled, client_config.transport, automatic_failover, failover_count
        );
    });

    let network_type = wallet_network_type(&wallet_id)?;
    let address_network_type = address_prefix_network_type(&wallet_id)?;
    let db_path = wallet_db_path_for(&wallet_id)?;
    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let (selection, profile_session) = begin_guarded_sync_profile_session(SyncWorkload::Rescan);
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
        .with_historical_mark_positions(
            historical_sapling_mark_positions,
            historical_ironwood_mark_positions,
        )
        .with_wallet_at_path(
            wallet_id.clone(),
            db_path,
            db_key,
            master_key,
            network_type,
            address_network_type,
        ) {
        Ok(sync) => sync,
        Err(e) => {
            profile_session.record_failure();
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
    let initial_status = rescan_phase_status(
        rescan_start_plan
            .requested_sync_from_height
            .saturating_sub(1),
        prior_target_height,
        crate::models::SyncStage::Preparing,
    );
    {
        let progress = progress.write().await;
        progress.set_current(initial_status.local_height);
        progress.set_target(initial_status.target_height);
        progress.set_stage(pirate_sync_lightd::SyncStage::Preparing);
        progress.start();
    }
    tokio::spawn(monitor_sync_profile_initial_tip(
        profile_session.clone(),
        Arc::clone(&progress),
    ));

    let rescan_session_arc = {
        let mut sessions = SYNC_SESSIONS.write();
        let session = Arc::new(tokio::sync::Mutex::new(SyncSession {
            sync: Some(Arc::clone(&sync)),
            cancelled: Some(cancel_flag),
            progress: Some(progress),
            perf: Some(perf),
            profile_session: Some(profile_session.clone()),
            last_status: initial_status.clone(),
            is_running: true,
            startup_in_progress: true,
            task: None,
        }));
        sessions.insert(wallet_id.clone(), Arc::clone(&session));
        session
    };
    cache_sync_status(&wallet_id, &initial_status);
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
                r#"{{"id":"log_rescan_session","timestamp":{},"location":"api.rs:3142","message":"rescan session created","data":{{"wallet_id":"{}","from_height":{},"requested_sync_from_height":{},"fallback_replay_from_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                ts,
                wallet_id,
                effective_from_height,
                rescan_start_plan.requested_sync_from_height,
                rescan_start_plan.fallback_replay_from_height
            );
        });
    }

    let wallet_id_for_task = wallet_id.clone();
    let session_arc_for_task = Arc::clone(&rescan_session_arc);
    let profile_session_for_task = profile_session.clone();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let task_handle = tokio::spawn(async move {
        let rescan_active_guard = rescan_active_guard;
        if start_rx.await.is_err() {
            return;
        }
        let sync_opt = { session_arc_for_task.lock().await.sync.clone() };

        if let Some(sync) = sync_opt {
            let result = run_sync_engine_task(sync.clone(), move |engine| {
                Box::pin(async move {
                    engine
                        .sync_rescan_to_latest(rescan_start_plan.requested_sync_from_height)
                        .await
                        .map_err(anyhow::Error::from)
                })
            })
            .await;

            let (progress_arc, perf_snapshot) = {
                let engine = sync.clone().lock_owned().await;
                (engine.progress(), engine.perf_counters().snapshot())
            };
            if result.is_err() {
                progress_arc
                    .write()
                    .await
                    .set_stage(pirate_sync_lightd::progress::SyncStage::Verify);
            }
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
                profile_session_for_task.record_success();
            } else {
                profile_session_for_task.record_failure();
            }

            let mut session = session_arc_for_task.lock().await;
            if let Some(status) = status_opt {
                session.last_status = status;
                cache_sync_status(&wallet_id_for_task, &session.last_status);
            }
            let mut rescan_ok = result.is_ok();
            match &result {
                Ok(()) => {
                    if let Some(requirement) = required_key_replay {
                        match complete_required_key_replay(
                            &wallet_id_for_task,
                            requirement,
                            rescan_start_plan.requested_sync_from_height,
                        ) {
                            Ok(true) => tracing::info!(
                                "Completed verified-key replay requirement for wallet {} at generation {}",
                                wallet_id_for_task,
                                requirement.import_generation
                            ),
                            Ok(false) => {
                                rescan_ok = false;
                                tracing::warn!(
                                    "Verified-key replay gate changed before completion for wallet {}; keeping rescan required",
                                    wallet_id_for_task
                                );
                            }
                            Err(error) => {
                                rescan_ok = false;
                                tracing::error!(
                                    "Could not complete verified-key replay gate for wallet {}: {}",
                                    wallet_id_for_task,
                                    error
                                );
                            }
                        }
                    }
                    tracing::info!("Rescan completed for wallet {}", wallet_id_for_task);
                    if let Err(error) =
                        super::seed_account_discovery::finalize_legacy_sapling_account_discovery(
                            &wallet_id_for_task,
                        )
                    {
                        tracing::warn!(
                            "Could not finalize seed account discovery for {}: {}",
                            wallet_id_for_task,
                            error
                        );
                    }
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
            session.profile_session = None;
            session.task = None;
            drop(session);
            clear_sync_runtime_cache(&wallet_id_for_task);
            drop(rescan_active_guard);

            if rescan_ok {
                maybe_trigger_compact_sync(wallet_id_for_task.clone());
            }
        } else {
            profile_session_for_task.record_failure();
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
    if start_tx.send(()).is_err() {
        let mut session = rescan_session_arc.lock().await;
        session.is_running = false;
        session.startup_in_progress = false;
        session.task = None;
        clear_sync_runtime_cache(&wallet_id);
        RESCAN_ACTIVE.write().remove(&wallet_id);
        profile_session.record_failure();
        return Err(anyhow!(
            "Failed to start rescan task for wallet {}",
            wallet_id
        ));
    }
    rescan_guard.transfer_status_to_session();
    Ok(())
}

async fn cancel_sync_session(wallet_id: WalletId, clear_engine_handle: bool) -> Result<()> {
    let session_arc_opt = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).cloned()
    };

    if let Some(session_arc) = session_arc_opt {
        let (cancel_opt, sync_opt, has_task, startup_in_progress, previous_status) = {
            let session = session_arc.lock().await;
            (
                session.cancelled.clone(),
                session.sync.clone(),
                session.task.is_some(),
                session.startup_in_progress,
                session.last_status.clone(),
            )
        };

        if let Some(cancelled) = cancel_opt {
            cancelled.cancel();
            tracing::info!("Sync cancellation requested for wallet {}", wallet_id);
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

        if has_task {
            wait_for_sync_stop(&wallet_id, CANCEL_SYNC_TASK_TIMEOUT).await?;
        } else if startup_in_progress {
            // Lifecycle operations are serialized, so a session with no task cannot
            // still be constructing an engine while this cancellation owns the lock.
            let mut session = session_arc.lock().await;
            session.is_running = false;
            session.startup_in_progress = false;
            if let Some(profile_session) = session.profile_session.take() {
                profile_session.record_failure();
            }
        }

        let recovered_status = open_wallet_db_for(&wallet_id)
            .ok()
            .and_then(|(db, _repo)| {
                let sync_storage = pirate_storage_sqlite::SyncStateStorage::new(&db);
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
            session.task = None;
        }
        cache_sync_status(&wallet_id, &recovered_status);
        clear_sync_runtime_cache(&wallet_id);
        RESCAN_ACTIVE.write().remove(&wallet_id);
    }

    Ok(())
}

pub(super) async fn cancel_sync_internal(
    wallet_id: WalletId,
    clear_engine_handle: bool,
) -> Result<()> {
    let operation_lock = sync_operation_lock(&wallet_id);
    let _operation_guard = operation_lock.lock().await;
    cancel_sync_session(wallet_id, clear_engine_handle).await
}

pub(super) async fn cancel_sync_for_wallet_switch(wallet_id: WalletId) -> Result<()> {
    let operation_lock = sync_operation_lock(&wallet_id);
    let _operation_guard = operation_lock.lock().await;
    if RESCAN_IN_FLIGHT.read().contains(&wallet_id) || is_rescan_active(&wallet_id) {
        tracing::info!(
            "Preserving active rescan for wallet {} during wallet switch",
            wallet_id
        );
        return Ok(());
    }

    cancel_sync_session(wallet_id, true).await
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
            return Ok(session.has_active_work());
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

pub(super) fn active_sync_wallet_ids() -> Vec<WalletId> {
    SYNC_SESSIONS.read().keys().cloned().collect()
}

pub(super) fn clear_all_runtime_state() {
    SYNC_SESSIONS.write().clear();
    SYNC_RUNTIME_HANDLES.write().clear();
    SYNC_STATUS_SNAPSHOT_CACHE.write().clear();
    TX_LIST_CACHE.write().clear();
    BALANCE_CACHE.write().clear();
    RESCAN_IN_FLIGHT.write().clear();
    RESCAN_ACTIVE.write().clear();
}

pub(super) fn clear_passphrase_change_sync_state() {
    clear_all_runtime_state();
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[tokio::test]
    async fn status_polling_does_not_restart_an_idle_session() {
        let wallet_id = format!("test-sync-status-{}", uuid::Uuid::new_v4());
        let session = Arc::new(Mutex::new(SyncSession {
            last_status: SyncStatus {
                local_height: 4_077_400,
                target_height: 4_077_500,
                percent: 99.99,
                eta: None,
                stage: SyncStage::Headers,
                last_checkpoint: Some(4_077_400),
                blocks_per_second: 0.0,
                notes_decrypted: 0,
                last_batch_ms: 0,
            },
            ..SyncSession::default()
        }));
        SYNC_SESSIONS
            .write()
            .insert(wallet_id.clone(), Arc::clone(&session));

        for _ in 0..3 {
            let status = sync_status(wallet_id.clone()).unwrap();
            assert_eq!(status.local_height, 4_077_400);
            assert_eq!(status.target_height, 4_077_500);
            assert!(matches!(status.stage, SyncStage::Headers));
        }

        let state = session.lock().await;
        assert!(!state.is_running);
        assert!(!state.startup_in_progress);
        assert!(state.task.is_none());
        drop(state);
        SYNC_SESSIONS.write().remove(&wallet_id);
        clear_sync_runtime_cache(&wallet_id);
    }

    #[tokio::test]
    async fn sync_stop_waits_for_cooperative_task_completion() {
        let wallet_id = format!("test-sync-stop-{}", uuid::Uuid::new_v4());
        let session = Arc::new(Mutex::new(SyncSession::default()));
        let session_for_task = Arc::clone(&session);
        let (finish_tx, finish_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = finish_rx.await;
            let mut session = session_for_task.lock().await;
            session.is_running = false;
            session.task = None;
        });
        {
            let mut state = session.lock().await;
            state.is_running = true;
            state.task = Some(task);
        }
        SYNC_SESSIONS
            .write()
            .insert(wallet_id.clone(), Arc::clone(&session));

        finish_tx.send(()).unwrap();
        wait_for_sync_stop(&wallet_id, Duration::from_secs(1))
            .await
            .unwrap();

        assert!(!session.lock().await.has_active_work());
        SYNC_SESSIONS.write().remove(&wallet_id);
    }

    #[tokio::test]
    async fn sync_stop_timeout_keeps_active_task_registered() {
        let wallet_id = format!("test-sync-timeout-{}", uuid::Uuid::new_v4());
        let session = Arc::new(Mutex::new(SyncSession::default()));
        let task = tokio::spawn(std::future::pending::<()>());
        {
            let mut state = session.lock().await;
            state.is_running = true;
            state.task = Some(task);
        }
        SYNC_SESSIONS
            .write()
            .insert(wallet_id.clone(), Arc::clone(&session));

        let result = wait_for_sync_stop(&wallet_id, Duration::from_millis(10)).await;

        assert!(result.is_err());
        let task = session.lock().await.task.take().unwrap();
        task.abort();
        SYNC_SESSIONS.write().remove(&wallet_id);
    }

    #[tokio::test]
    async fn exclusive_key_import_joins_the_active_sync_task() {
        let wallet_id = format!("test-key-import-stop-{}", uuid::Uuid::new_v4());
        let session = Arc::new(Mutex::new(SyncSession::default()));
        let cancellation = CancelToken::new();
        let cancellation_for_task = cancellation.clone();
        let session_for_task = Arc::clone(&session);
        let task = tokio::spawn(async move {
            cancellation_for_task.cancelled().await;
            let mut state = session_for_task.lock().await;
            state.is_running = false;
            state.task = None;
        });
        {
            let mut state = session.lock().await;
            state.cancelled = Some(cancellation.clone());
            state.is_running = true;
            state.task = Some(task);
        }
        SYNC_SESSIONS
            .write()
            .insert(wallet_id.clone(), Arc::clone(&session));

        let operation_guard = acquire_exclusive_key_import(&wallet_id).await.unwrap();

        assert!(cancellation.is_cancelled());
        assert!(!session.lock().await.has_active_work());
        drop(operation_guard);
        SYNC_SESSIONS.write().remove(&wallet_id);
        SYNC_OPERATION_LOCKS.write().remove(&wallet_id);
    }
}
