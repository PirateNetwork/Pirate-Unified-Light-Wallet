use super::*;
use pirate_sync_lightd::{BackgroundSyncConfig, BackgroundSyncMode, BackgroundSyncOrchestrator};
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

    let endpoint_url = endpoint_config.url();
    let (transport, socks5_url, allow_direct_fallback) = tunnel_transport_config();
    let tls_enabled = endpoint_config.use_tls;
    let host = endpoint_config.host.clone();
    let is_ip_address = host.parse::<std::net::IpAddr>().is_ok();
    let tls_server_name = if tls_enabled {
        if is_ip_address {
            Some("lightd1.piratechain.com".to_string())
        } else {
            Some(host.clone())
        }
    } else {
        None
    };

    let client_config = LightClientConfig {
        endpoint: endpoint_url,
        transport,
        socks5_url,
        tls: TlsConfig {
            enabled: tls_enabled,
            spki_pin: endpoint_config.tls_pin.clone(),
            server_name: tls_server_name,
        },
        retry: RetryConfig::default(),
        connect_timeout: std::time::Duration::from_secs(30),
        request_timeout: std::time::Duration::from_secs(60),
        allow_direct_fallback,
    };

    let network_type = wallet_network_type(&wallet_id)?;
    let address_network_type = address_prefix_network_type(&wallet_id)?;
    let config = SyncConfig::default();
    let (db_key, master_key) = wallet_db_keys(&wallet_id)?;
    let client = LightClient::with_config(client_config);
    let sync_engine = SyncEngine::with_client_and_config(client, birthday_height, config)
        .with_wallet(
            wallet_id.clone(),
            db_key,
            master_key,
            network_type,
            address_network_type,
        )
        .map_err(|e| anyhow!("Failed to initialize background sync engine: {}", e))?;

    let mut bg_config = BackgroundSyncConfig::default();
    if let Some(value) = max_duration_secs {
        bg_config.max_duration_secs = value.max(1);
    }
    if let Some(value) = max_blocks {
        bg_config.max_blocks = value.max(1);
    }
    let orchestrator =
        BackgroundSyncOrchestrator::new(Arc::new(TokioMutex::new(sync_engine)), bg_config);

    let sync_mode = match mode.as_deref() {
        Some("deep") => BackgroundSyncMode::Deep,
        Some("compact") | None => BackgroundSyncMode::Compact,
        _ => BackgroundSyncMode::Compact,
    };

    let result = orchestrator
        .execute_sync(sync_mode)
        .await
        .map_err(|e| anyhow!("Background sync failed: {}", e))?;

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
    let session_arc_opt = {
        let sessions = SYNC_SESSIONS.read();
        sessions.get(&wallet_id).map(Arc::clone)
    };

    if let Some(session_arc) = session_arc_opt {
        let sync_opt = { session_arc.lock().await.sync.clone() };
        if let Some(sync) = sync_opt {
            let progress_arc = {
                let engine = sync.clone().lock_owned().await;
                engine.progress()
            };
            let progress = progress_arc.read().await;
            Ok(progress.current_height() < progress.target_height())
        } else {
            Ok(false)
        }
    } else {
        let passphrase = app_passphrase()?;
        let (db, _key, _master_key) = open_wallet_db_with_passphrase(&wallet_id, &passphrase)?;
        let sync_state = pirate_storage_sqlite::SyncStateStorage::new(&db);

        match sync_state.load_sync_state() {
            Ok(state) => Ok(state.local_height < state.target_height),
            Err(_) => Ok(false),
        }
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
