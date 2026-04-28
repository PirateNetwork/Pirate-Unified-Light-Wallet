use super::tunnel::tunnel_transport_config;
use super::*;
use pirate_sync_lightd::{
    begin_sync_profile_session, record_sync_profile_failure, record_sync_profile_success,
    BackgroundSyncConfig, BackgroundSyncMode, BackgroundSyncOrchestrator, SyncEngine, SyncWorkload,
};
use tokio::sync::Mutex as TokioMutex;

const WARM_WALLET_WINDOW_SECS: i64 = 7 * 24 * 60 * 60;
const BG_SYNC_CURSOR_KEY: &str = "bg_rr_cursor";

pub async fn start_background_sync(
    wallet_id: WalletId,
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::BackgroundSyncResult> {
    run_on_runtime(move || {
        start_background_sync_inner(wallet_id, mode, max_duration_secs, max_blocks)
    })
    .await
}

async fn start_background_sync_inner(
    wallet_id: WalletId,
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::BackgroundSyncResult> {
    tracing::info!(
        "Starting background sync for wallet {} with mode {:?}",
        wallet_id,
        mode
    );

    let (birthday_height, endpoint_config) = {
        let wallet = get_wallet_meta(&wallet_id)?;
        let birthday_height = wallet.birthday_height;
        let endpoint_config = get_lightd_endpoint_config(wallet_id.clone())?;
        (birthday_height, endpoint_config)
    };

    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    let client_config = endpoint::build_light_client_config(
        &endpoint_config,
        transport,
        socks5_url,
        allow_direct_fallback,
        RetryConfig::default(),
        std::time::Duration::from_secs(30),
        std::time::Duration::from_secs(60),
    );

    let sync_mode = match mode.as_deref() {
        Some("deep") => BackgroundSyncMode::Deep,
        Some("compact") | None => BackgroundSyncMode::Compact,
        _ => BackgroundSyncMode::Compact,
    };
    let workload = match sync_mode {
        BackgroundSyncMode::Compact => SyncWorkload::Compact,
        BackgroundSyncMode::Deep => SyncWorkload::Deep,
    };

    let network_type = wallet_network_type(&wallet_id)?;
    let address_network_type = address_prefix_network_type(&wallet_id)?;
    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let selection = begin_sync_profile_session(workload);
    let sync_profile = selection.profile;
    let config = selection.config;
    tracing::info!(
        "background sync: selected local sync profile {} for {:?} (batch_size={}, max_batch_size={}, target_bytes={}, max_bytes={}, prefetch_depth={}, workers={}, crash_downgraded={}, downgrade_steps={})",
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
    let sync_engine = match SyncEngine::with_client_and_config(client, birthday_height, config)
        .with_wallet(
            wallet_id.clone(),
            db_key,
            master_key,
            network_type,
            address_network_type,
        ) {
        Ok(sync_engine) => sync_engine,
        Err(e) => {
            record_sync_profile_failure();
            return Err(anyhow!(
                "Failed to initialize background sync engine: {}",
                e
            ));
        }
    };

    let mut bg_config = BackgroundSyncConfig::default();
    if let Some(value) = max_duration_secs {
        bg_config.max_duration_secs = value.max(1);
    }
    if let Some(value) = max_blocks {
        bg_config.max_blocks = value.max(1);
    }
    let orchestrator =
        BackgroundSyncOrchestrator::new(Arc::new(TokioMutex::new(sync_engine)), bg_config);

    let result = match orchestrator.execute_sync(sync_mode).await {
        Ok(result) => {
            if result.errors.is_empty() {
                record_sync_profile_success();
            } else {
                record_sync_profile_failure();
            }
            result
        }
        Err(e) => {
            record_sync_profile_failure();
            return Err(anyhow!("Background sync failed: {}", e));
        }
    };

    if let Ok(registry_db) = open_wallet_registry() {
        if let Err(e) = touch_wallet_last_synced(&registry_db, &wallet_id) {
            tracing::warn!("Failed to update last_synced_at for {}: {}", wallet_id, e);
        }
    }

    Ok(crate::models::BackgroundSyncResult {
        mode: match result.mode {
            BackgroundSyncMode::Compact => "compact".to_string(),
            BackgroundSyncMode::Deep => "deep".to_string(),
        },
        blocks_synced: result.blocks_synced,
        start_height: result.start_height,
        end_height: result.end_height,
        duration_secs: result.duration_secs,
        errors: result.errors,
        new_balance: result.new_balance,
        new_transactions: result.new_transactions,
    })
}

pub async fn start_background_sync_round_robin(
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::WalletBackgroundSyncResult> {
    run_on_runtime(move || {
        start_background_sync_round_robin_inner(mode, max_duration_secs, max_blocks)
    })
    .await
}

async fn start_background_sync_round_robin_inner(
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::WalletBackgroundSyncResult> {
    ensure_wallet_registry_loaded()?;

    let registry_db = open_wallet_registry()?;
    let mut candidates = load_wallet_registry_activity(&registry_db)?;
    if candidates.is_empty() {
        return Err(anyhow!("No wallets available for background sync"));
    }

    candidates.retain(|candidate| match is_sync_running(candidate.id.clone()) {
        Ok(is_running) => !is_running,
        Err(_) => true,
    });

    if candidates.is_empty() {
        return Err(anyhow!("All wallets are currently syncing"));
    }

    let now = chrono::Utc::now().timestamp();
    let mut warm = Vec::new();
    let mut cool = Vec::new();
    for candidate in candidates {
        let is_warm = candidate
            .last_used_at
            .map(|ts| now - ts <= WARM_WALLET_WINDOW_SECS)
            .unwrap_or(false);
        if is_warm {
            warm.push(candidate);
        } else {
            cool.push(candidate);
        }
    }

    warm.sort_by_key(|entry| std::cmp::Reverse(entry.last_used_at.unwrap_or(0)));
    cool.sort_by_key(|entry| entry.last_synced_at.unwrap_or(0));

    let mut ordered = Vec::with_capacity(warm.len() + cool.len());
    ordered.extend(warm);
    ordered.extend(cool);

    if ordered.is_empty() {
        return Err(anyhow!("No eligible wallets for background sync"));
    }

    let cursor = get_registry_setting(&registry_db, BG_SYNC_CURSOR_KEY)?;
    if let Some(cursor_id) = cursor.as_deref() {
        if let Some(pos) = ordered.iter().position(|entry| entry.id == cursor_id) {
            let len = ordered.len();
            ordered.rotate_left((pos + 1) % len);
        }
    }

    let next_wallet_id = ordered[0].id.clone();
    let result =
        start_background_sync_inner(next_wallet_id.clone(), mode, max_duration_secs, max_blocks)
            .await?;

    if let Err(e) = set_registry_setting(&registry_db, BG_SYNC_CURSOR_KEY, Some(&next_wallet_id)) {
        tracing::warn!(
            "Failed to update background sync cursor for {}: {}",
            next_wallet_id,
            e
        );
    }

    Ok(crate::models::WalletBackgroundSyncResult {
        wallet_id: next_wallet_id,
        mode: result.mode,
        blocks_synced: result.blocks_synced,
        start_height: result.start_height,
        end_height: result.end_height,
        duration_secs: result.duration_secs,
        errors: result.errors,
        new_balance: result.new_balance,
        new_transactions: result.new_transactions,
    })
}

pub async fn is_background_sync_needed(wallet_id: WalletId) -> Result<bool> {
    if let Some(needs_work) = sync_control::foreground_sync_needs_work(&wallet_id).await {
        return Ok(needs_work);
    }

    let passphrase = app_passphrase()?;
    let (db, _key, _master_key) = open_wallet_db_with_passphrase(&wallet_id, &passphrase)?;
    let sync_state = pirate_storage_sqlite::SyncStateStorage::new(&db);

    match sync_state.load_sync_state() {
        Ok(state) => Ok(state.local_height < state.target_height),
        Err(_) => Ok(false),
    }
}

pub fn get_recommended_background_sync_mode(
    _wallet_id: WalletId,
    minutes_since_last: u32,
) -> Result<String> {
    let config = BackgroundSyncConfig::default();
    let temp_orchestrator = BackgroundSyncOrchestrator::new(
        Arc::new(TokioMutex::new(SyncEngine::new("dummy".to_string(), 0))),
        config,
    );

    let mode = temp_orchestrator.recommend_sync_mode(minutes_since_last);
    Ok(match mode {
        BackgroundSyncMode::Compact => "compact".to_string(),
        BackgroundSyncMode::Deep => "deep".to_string(),
    })
}
