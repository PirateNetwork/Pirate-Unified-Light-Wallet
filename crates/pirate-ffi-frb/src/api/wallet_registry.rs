use super::*;
use rusqlite::params;

fn load_wallet_registry(db: &Database) -> Result<(Vec<WalletMeta>, Option<WalletId>)> {
    let mut wallets = Vec::new();
    let mut stmt = db.conn().prepare(
        "SELECT id, name, created_at, watch_only, birthday_height, network_type
         FROM wallet_registry
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WalletMeta {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            watch_only: row.get::<_, i64>(3)? != 0,
            birthday_height: row.get::<_, i64>(4)? as u32,
            network_type: row.get(5)?,
        })
    })?;
    for row in rows {
        wallets.push(row?);
    }

    let active_wallet_id = get_registry_setting(db, "active_wallet_id")?;

    Ok((wallets, active_wallet_id))
}

#[derive(Debug, Clone)]
pub(super) struct WalletRegistryActivity {
    pub(super) id: WalletId,
    pub(super) last_used_at: Option<i64>,
    pub(super) last_synced_at: Option<i64>,
}

pub(super) fn load_wallet_registry_activity(db: &Database) -> Result<Vec<WalletRegistryActivity>> {
    let mut wallets = Vec::new();
    let mut stmt = db.conn().prepare(
        "SELECT id, last_used_at, last_synced_at
         FROM wallet_registry
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WalletRegistryActivity {
            id: row.get(0)?,
            last_used_at: row.get(1)?,
            last_synced_at: row.get(2)?,
        })
    })?;
    for row in rows {
        wallets.push(row?);
    }
    Ok(wallets)
}

pub(super) fn persist_wallet_meta(db: &Database, meta: &WalletMeta) -> Result<()> {
    db.conn().execute(
        r#"
        INSERT INTO wallet_registry
            (id, name, created_at, watch_only, birthday_height, network_type, last_used_at, last_synced_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            watch_only = excluded.watch_only,
            birthday_height = excluded.birthday_height,
            network_type = excluded.network_type
        "#,
        params![
            meta.id,
            meta.name,
            meta.created_at,
            if meta.watch_only { 1 } else { 0 },
            meta.birthday_height as i64,
            meta.network_type,
        ],
    )?;
    Ok(())
}

fn delete_wallet_meta(db: &Database, wallet_id: &str) -> Result<()> {
    db.conn().execute(
        "DELETE FROM wallet_registry WHERE id = ?1",
        params![wallet_id],
    )?;
    Ok(())
}

pub(super) fn set_active_wallet_registry(db: &Database, wallet_id: Option<&str>) -> Result<()> {
    set_registry_setting(db, "active_wallet_id", wallet_id)
}

pub(super) fn touch_wallet_last_used(db: &Database, wallet_id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.conn().execute(
        "UPDATE wallet_registry SET last_used_at = ?1 WHERE id = ?2",
        params![now, wallet_id],
    )?;
    Ok(())
}

pub(super) fn touch_wallet_last_synced(db: &Database, wallet_id: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    db.conn().execute(
        "UPDATE wallet_registry SET last_synced_at = ?1 WHERE id = ?2",
        params![now, wallet_id],
    )?;
    Ok(())
}

pub(super) fn ensure_wallet_registry_loaded() -> Result<()> {
    install_debug_panic_hook();
    install_runtime_diagnostics();
    if is_decoy_mode_active() {
        return Ok(());
    }
    if REGISTRY_LOADED.load(Ordering::SeqCst) {
        return Ok(());
    }

    let db = open_wallet_registry()?;
    load_wallet_registry_state(&db)
}

pub(super) fn load_wallet_registry_state(db: &Database) -> Result<()> {
    let (wallets, active) = load_wallet_registry(db)?;
    *WALLETS.write() = wallets;
    *ACTIVE_WALLET.write() = active;

    {
        let mut endpoints = LIGHTD_ENDPOINTS.write();
        endpoints.clear();

        for wallet in WALLETS.read().iter() {
            let endpoint_key = format!("lightd_endpoint_{}", wallet.id);
            let pin_key = format!("lightd_tls_pin_{}", wallet.id);
            let endpoint_url = get_registry_setting(db, &endpoint_key)?;
            let tls_pin = get_registry_setting(db, &pin_key)?;

            if let Some(url) = endpoint_url {
                match parse_endpoint_url(&url) {
                    Ok((host, port, use_tls)) => {
                        let endpoint = LightdEndpoint {
                            host,
                            port,
                            use_tls,
                            tls_pin,
                            label: Some("Custom".to_string()),
                        };
                        endpoints.insert(wallet.id.clone(), endpoint);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse stored endpoint for wallet {}: {}",
                            wallet.id,
                            e
                        );
                    }
                }
            }
        }
    }

    if let Ok(Some(mode)) = tunnel::load_registry_tunnel_mode(db) {
        *TUNNEL_MODE.write() = mode;
    }

    if let Some(pending) = PENDING_TUNNEL_MODE.write().take() {
        if let Err(e) = tunnel::persist_registry_tunnel_mode(db, &pending) {
            tracing::warn!("Failed to persist pending tunnel mode: {}", e);
            *PENDING_TUNNEL_MODE.write() = Some(pending);
        } else {
            *TUNNEL_MODE.write() = pending;
        }
    }

    REGISTRY_LOADED.store(true, Ordering::SeqCst);
    Ok(())
}

pub(super) fn get_wallet_meta(wallet_id: &str) -> Result<WalletMeta> {
    if is_decoy_mode_active() {
        return Ok(decoy_wallet_meta());
    }
    ensure_wallet_registry_loaded()?;
    let wallets = WALLETS.read();
    wallets
        .iter()
        .find(|w| w.id == wallet_id)
        .cloned()
        .ok_or_else(|| anyhow!("Wallet not found"))
}

fn auto_consolidation_setting_key(wallet_id: &WalletId) -> String {
    format!("auto_consolidation_enabled_{}", wallet_id)
}

pub(super) fn auto_consolidation_enabled(wallet_id: &WalletId) -> Result<bool> {
    if !wallet_registry_path()?.exists() {
        return Ok(false);
    }
    let registry_db = open_wallet_registry()?;
    let key = auto_consolidation_setting_key(wallet_id);
    let enabled = get_registry_setting(&registry_db, &key)?
        .map(|value| value == "true")
        .unwrap_or(false);
    Ok(enabled)
}

pub(super) fn wallet_registry_exists() -> Result<bool> {
    let path = wallet_registry_path()?;
    Ok(path.exists())
}

pub(super) fn list_wallets() -> Result<Vec<WalletMeta>> {
    if is_decoy_mode_active() {
        ensure_decoy_wallet_state();
        return Ok(WALLETS.read().clone());
    }
    match ensure_wallet_registry_loaded() {
        Ok(_) => Ok(WALLETS.read().clone()),
        Err(e) => {
            let path = wallet_registry_path()?;
            if path.exists() {
                Err(e)
            } else {
                Ok(Vec::new())
            }
        }
    }
}

pub(super) fn switch_wallet(wallet_id: WalletId) -> Result<()> {
    if is_decoy_mode_active() {
        ensure_decoy_wallet_state();
        return Ok(());
    }
    ensure_wallet_registry_loaded()?;
    {
        let wallets = WALLETS.read();
        if !wallets.iter().any(|w| w.id == wallet_id) {
            return Err(anyhow!("Wallet not found: {}", wallet_id));
        }
    }

    let previous_active = ACTIVE_WALLET.read().clone();
    if let Some(previous_wallet_id) = previous_active.as_ref() {
        if previous_wallet_id != &wallet_id {
            let previous_wallet_id = previous_wallet_id.clone();
            let cancel_result = run_on_runtime_blocking({
                let wallet_id_for_cancel = previous_wallet_id.clone();
                move || async move {
                    cancel_sync_internal(wallet_id_for_cancel.clone(), true).await?;
                    Ok(())
                }
            });

            pirate_core::debug_log::with_locked_file(|file| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_wallet_switch_cancel","timestamp":{},"location":"api.rs:switch_wallet","message":"wallet switch previous sync cancel","data":{{"previous_wallet_id":"{}","next_wallet_id":"{}","success":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"W"}}"#,
                    ts,
                    previous_wallet_id,
                    wallet_id,
                    cancel_result.is_ok(),
                    cancel_result
                        .as_ref()
                        .err()
                        .map(|e| truncate_for_log(&e.to_string(), 180))
                        .unwrap_or_default()
                );
            });

            if let Err(e) = cancel_result {
                tracing::warn!(
                    "Failed to cancel previous wallet sync during switch ({} -> {}): {}",
                    previous_wallet_id,
                    wallet_id,
                    e
                );
            }
        }
    }

    *ACTIVE_WALLET.write() = Some(wallet_id);
    let registry_db = open_wallet_registry()?;
    set_active_wallet_registry(&registry_db, ACTIVE_WALLET.read().as_deref())?;
    if let Some(active) = ACTIVE_WALLET.read().clone() {
        touch_wallet_last_used(&registry_db, &active)?;
    }
    Ok(())
}

pub(super) fn get_auto_consolidation_enabled(wallet_id: WalletId) -> Result<bool> {
    ensure_wallet_registry_loaded()?;
    auto_consolidation_enabled(&wallet_id)
}

pub(super) fn set_auto_consolidation_enabled(wallet_id: WalletId, enabled: bool) -> Result<()> {
    ensure_wallet_registry_loaded()?;
    let registry_db = open_wallet_registry()?;
    let key = auto_consolidation_setting_key(&wallet_id);
    let value = if enabled { Some("true") } else { None };
    set_registry_setting(&registry_db, &key, value)?;
    Ok(())
}

pub(super) fn get_active_wallet() -> Result<Option<WalletId>> {
    if is_decoy_mode_active() {
        ensure_decoy_wallet_state();
        return Ok(Some(DECOY_WALLET_ID.to_string()));
    }
    ensure_wallet_registry_loaded()?;
    if ACTIVE_WALLET.read().is_none() {
        if let Some(first) = WALLETS.read().first() {
            let id = first.id.clone();
            *ACTIVE_WALLET.write() = Some(id.clone());
            let registry_db = open_wallet_registry()?;
            set_active_wallet_registry(&registry_db, Some(&id))?;
            touch_wallet_last_used(&registry_db, &id)?;
        }
    }
    Ok(ACTIVE_WALLET.read().clone())
}

pub(super) fn rename_wallet(wallet_id: WalletId, new_name: String) -> Result<()> {
    ensure_wallet_registry_loaded()?;
    let mut wallets = WALLETS.write();
    let Some(meta) = wallets.iter_mut().find(|w| w.id == wallet_id) else {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    };
    meta.name = new_name;

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, meta)?;
    Ok(())
}

pub(super) fn set_wallet_birthday_height(wallet_id: WalletId, birthday_height: u32) -> Result<()> {
    if birthday_height == 0 {
        return Err(anyhow!("Invalid birthday height"));
    }
    ensure_wallet_registry_loaded()?;
    let mut wallets = WALLETS.write();
    let Some(meta) = wallets.iter_mut().find(|w| w.id == wallet_id) else {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    };
    meta.birthday_height = birthday_height;

    let registry_db = open_wallet_registry()?;
    persist_wallet_meta(&registry_db, meta)?;
    Ok(())
}

pub(super) fn delete_wallet(wallet_id: WalletId) -> Result<()> {
    ensure_wallet_registry_loaded()?;

    let mut wallets = WALLETS.write();
    let Some(index) = wallets.iter().position(|w| w.id == wallet_id) else {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    };
    wallets.remove(index);

    {
        let registry_db = open_wallet_registry()?;
        delete_wallet_meta(&registry_db, &wallet_id)?;
        let endpoint_key = format!("lightd_endpoint_{}", wallet_id);
        let pin_key = format!("lightd_tls_pin_{}", wallet_id);
        set_registry_setting(&registry_db, &endpoint_key, None)?;
        set_registry_setting(&registry_db, &pin_key, None)?;

        if ACTIVE_WALLET.read().as_ref() == Some(&wallet_id) {
            let next_active = wallets.first().map(|w| w.id.clone());
            *ACTIVE_WALLET.write() = next_active.clone();
            set_active_wallet_registry(&registry_db, next_active.as_deref())?;
            if let Some(active) = next_active {
                touch_wallet_last_used(&registry_db, &active)?;
            }
        }
    }

    {
        let mut sessions = SYNC_SESSIONS.write();
        sessions.remove(&wallet_id);
    }
    clear_sync_runtime_cache(&wallet_id);

    {
        let mut endpoints = LIGHTD_ENDPOINTS.write();
        endpoints.remove(&wallet_id);
    }

    let db_path = wallet_db_path_for(&wallet_id)?;
    let _ = fs::remove_file(db_path);
    let _ = fs::remove_file(wallet_db_salt_path(&wallet_id)?);
    let _ = fs::remove_file(wallet_db_key_path(&wallet_id)?);

    if wallets.is_empty() {
        *ACTIVE_WALLET.write() = None;
        passphrase_store::clear_passphrase();
        REGISTRY_LOADED.store(false, Ordering::SeqCst);

        let _ = fs::remove_file(wallet_registry_path()?);
        let _ = fs::remove_file(wallet_registry_salt_path()?);
        let _ = fs::remove_file(wallet_registry_key_path()?);
    }

    Ok(())
}
