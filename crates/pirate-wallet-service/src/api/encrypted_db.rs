use super::*;
use std::rc::Rc;

pub(super) fn wallet_base_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("PIRATE_WALLET_DB_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    if let Ok(path) = std::env::var("PIRATE_WALLET_DB_PATH") {
        if path.contains("{wallet_id}") {
            let parent = Path::new(&path).parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }

        let parsed = PathBuf::from(&path);
        if parsed.extension().is_some() {
            let parent = parsed.parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }
        return Ok(parsed);
    }

    let base = ProjectDirs::from("com", "Pirate", "PirateWallet")
        .map(|dirs| dirs.data_local_dir().join("wallets"))
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base)
}

pub(super) fn wallet_db_path_for(wallet_id: &str) -> Result<PathBuf> {
    if let Ok(template) = std::env::var("PIRATE_WALLET_DB_PATH") {
        if template.contains("{wallet_id}") {
            return Ok(PathBuf::from(template.replace("{wallet_id}", wallet_id)));
        }
    }

    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join(format!("wallet_{}.db", wallet_id)))
}

pub(super) fn wallet_registry_path() -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join("wallet_registry.db"))
}

pub(super) fn app_passphrase() -> Result<String> {
    let passphrase =
        passphrase_store::get_passphrase().map_err(|e| anyhow!("App is locked: {}", e))?;
    Ok(passphrase.as_str().to_string())
}

pub(super) fn wallet_registry_salt_path() -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join("wallet_registry.salt"))
}

pub(super) fn wallet_registry_key_path() -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join("wallet_registry.dbkey"))
}

pub(super) fn wallet_db_salt_path(wallet_id: &str) -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join(format!("wallet_{}.salt", wallet_id)))
}

pub(super) fn wallet_db_key_path(wallet_id: &str) -> Result<PathBuf> {
    let base = wallet_base_dir()?;
    fs::create_dir_all(&base)?;
    Ok(base.join(format!("wallet_{}.dbkey", wallet_id)))
}

fn load_salt(path: &Path) -> Result<[u8; 32]> {
    let data = fs::read(path)?;
    if data.len() != 32 {
        return Err(anyhow!("Invalid salt length in {}", path.display()));
    }
    let mut salt = [0u8; 32];
    salt.copy_from_slice(&data);
    Ok(salt)
}

fn write_salt(path: &Path, salt: &[u8; 32]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, salt)?;
    Ok(())
}

fn load_sealed_key(path: &Path) -> Result<Option<SealedKey>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read(path)?;
    Ok(Some(SealedKey::deserialize(&data)?))
}

fn store_sealed_key(path: &Path, sealed: &SealedKey) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, sealed.serialize())?;
    Ok(())
}

fn try_unseal_db_key(sealed: &SealedKey) -> Result<Option<EncryptionKey>> {
    let Some(keystore) = platform_keystore() else {
        return Ok(None);
    };

    match keystore.unseal_key(sealed) {
        KeystoreResult::Success(master) => {
            let key = EncryptionKey::from_bytes(*master.as_bytes());
            Ok(Some(key))
        }
        KeystoreResult::NotAvailable => Ok(None),
        KeystoreResult::Cancelled => Err(anyhow!("Keystore unlock cancelled")),
        KeystoreResult::AuthFailed => Err(anyhow!("Keystore authentication failed")),
        KeystoreResult::Error(e) => Err(anyhow!("Keystore error: {}", e)),
    }
}

fn maybe_store_sealed_db_key(key: &EncryptionKey, key_id: &str, sealed_path: &Path) -> Result<()> {
    let Some(keystore) = platform_keystore() else {
        return Ok(());
    };

    if sealed_path.exists() {
        return Ok(());
    }

    let master = MasterKey::from_bytes(key.as_bytes(), EncryptionAlgorithm::ChaCha20Poly1305)
        .map_err(|e| anyhow!("Failed to wrap db key: {}", e))?;
    let sealed = match keystore.seal_key(&master, key_id) {
        Ok(sealed) => sealed,
        Err(e) => {
            tracing::warn!("Failed to seal db key (keystore unavailable?): {}", e);
            return Ok(());
        }
    };
    if let Err(e) = store_sealed_key(sealed_path, &sealed) {
        tracing::warn!("Failed to persist sealed db key: {}", e);
    }
    Ok(())
}

fn force_store_sealed_db_key(
    key: &EncryptionKey,
    key_id: &str,
    sealed_path: &Path,
) -> Result<bool> {
    let Some(keystore) = platform_keystore() else {
        return Ok(false);
    };

    let master = MasterKey::from_bytes(key.as_bytes(), EncryptionAlgorithm::ChaCha20Poly1305)
        .map_err(|e| anyhow!("Failed to wrap db key: {}", e))?;
    let sealed = match keystore.seal_key(&master, key_id) {
        Ok(sealed) => sealed,
        Err(e) => {
            tracing::warn!("Failed to seal db key for reseal: {}", e);
            return Ok(false);
        }
    };
    if let Err(e) = store_sealed_key(sealed_path, &sealed) {
        tracing::warn!("Failed to persist resealed db key: {}", e);
        return Ok(false);
    }
    Ok(true)
}

pub(super) fn reseal_registry_db_key(passphrase: &str) -> Result<bool> {
    let registry_path = wallet_registry_path()?;
    if !registry_path.exists() {
        return Ok(false);
    }

    let _db = open_wallet_registry_with_passphrase(passphrase)?;
    let salt_path = wallet_registry_salt_path()?;
    let key_path = wallet_registry_key_path()?;
    if !salt_path.exists() {
        return Ok(false);
    }
    let salt = load_salt(&salt_path)?;
    let key = derive_db_key(passphrase, &salt)?;
    force_store_sealed_db_key(&key, "pirate_wallet_registry_db", &key_path)
}

pub(super) fn reseal_wallet_db_key(wallet_id: &str, passphrase: &str) -> Result<bool> {
    let (_db, key, _master_key) = open_wallet_db_with_passphrase(wallet_id, passphrase)?;
    let key_path = wallet_db_key_path(wallet_id)?;
    let key_id = format!("pirate_wallet_{}_db", wallet_id);
    force_store_sealed_db_key(&key, &key_id, &key_path)
}

pub(super) fn derive_db_key(passphrase: &str, salt: &[u8; 32]) -> Result<EncryptionKey> {
    EncryptionKey::from_passphrase(passphrase, salt)
        .map_err(|e| anyhow!("Failed to derive db key: {}", e))
}

pub(super) fn open_encrypted_db_with_migration(
    db_path: &Path,
    passphrase: &str,
    salt_path: &Path,
    sealed_key_path: &Path,
    legacy_key: Option<EncryptionKey>,
    master_key: &MasterKey,
    key_id: &str,
) -> Result<(Database, EncryptionKey)> {
    let db_exists = db_path.exists();
    let salt_exists = salt_path.exists();

    if salt_exists {
        let salt = load_salt(salt_path)?;
        let mut used_sealed = false;
        let key = match load_sealed_key(sealed_key_path)? {
            Some(sealed) => {
                if let Some(unsealed) = try_unseal_db_key(&sealed)? {
                    used_sealed = true;
                    unsealed
                } else {
                    derive_db_key(passphrase, &salt)?
                }
            }
            None => derive_db_key(passphrase, &salt)?,
        };

        match Database::open(db_path, &key, master_key.clone()) {
            Ok(db) => {
                maybe_store_sealed_db_key(&key, key_id, sealed_key_path)?;
                return Ok((db, key));
            }
            Err(e) if used_sealed => {
                let derived = derive_db_key(passphrase, &salt)?;
                let db = Database::open(db_path, &derived, master_key.clone()).map_err(|_| e)?;
                maybe_store_sealed_db_key(&derived, key_id, sealed_key_path)?;
                return Ok((db, derived));
            }
            Err(e) => return Err(e.into()),
        }
    }

    if db_exists {
        let legacy = legacy_key.ok_or_else(|| anyhow!("Legacy key not available"))?;
        let db = Database::open(db_path, &legacy, master_key.clone())?;
        let salt = generate_salt();
        let new_key = derive_db_key(passphrase, &salt)?;
        db.rekey(&new_key)?;
        if let Err(e) = write_salt(salt_path, &salt) {
            let _ = db.rekey(&legacy);
            return Err(e);
        }
        maybe_store_sealed_db_key(&new_key, key_id, sealed_key_path)?;
        return Ok((db, new_key));
    }

    let salt = generate_salt();
    write_salt(salt_path, &salt)?;
    let key = derive_db_key(passphrase, &salt)?;
    let db = Database::open(db_path, &key, master_key.clone())?;
    maybe_store_sealed_db_key(&key, key_id, sealed_key_path)?;
    Ok((db, key))
}

pub(super) fn registry_master_key(passphrase: &str) -> Result<MasterKey> {
    let salt = Sha256::digest(b"wallet-registry");
    AppPassphrase::derive_key(passphrase, &salt[..16])
        .map_err(|e| anyhow!("Failed to derive registry master key: {}", e))
}

pub(super) fn open_wallet_registry_with_passphrase(passphrase: &str) -> Result<Database> {
    let path = wallet_registry_path()?;
    let salt_path = wallet_registry_salt_path()?;
    let key_path = wallet_registry_key_path()?;
    let master_key = registry_master_key(passphrase)?;
    let legacy_key = EncryptionKey::from_legacy_password(passphrase);
    let key_id = "pirate_wallet_registry_db";

    let (db, _key) = open_encrypted_db_with_migration(
        &path,
        passphrase,
        &salt_path,
        &key_path,
        Some(legacy_key),
        &master_key,
        key_id,
    )?;

    ensure_wallet_registry_schema(&db)?;
    Ok(db)
}

pub(super) fn open_wallet_registry() -> Result<Database> {
    let passphrase = app_passphrase()?;
    open_wallet_registry_with_passphrase(&passphrase)
}

pub(super) fn ensure_wallet_registry_schema(db: &Database) -> Result<()> {
    db.conn().execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS wallet_registry (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            watch_only INTEGER NOT NULL,
            birthday_height INTEGER NOT NULL,
            network_type TEXT,
            last_used_at INTEGER,
            last_synced_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS wallet_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

pub(super) fn get_registry_setting(db: &Database, key: &str) -> Result<Option<String>> {
    let value = db
        .conn()
        .query_row(
            "SELECT value FROM wallet_settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .ok();
    Ok(value)
}

pub(super) fn set_registry_setting(db: &Database, key: &str, value: Option<&str>) -> Result<()> {
    if let Some(val) = value {
        // Use direct execute() calls instead of prepared statements
        // SQLCipher may have issues with prepared statement execute() returning results
        let conn = db.conn();

        // Delete any existing row
        conn.execute("DELETE FROM wallet_settings WHERE key = ?1", params![key])?;

        // Insert new row
        conn.execute(
            "INSERT INTO wallet_settings (key, value) VALUES (?1, ?2)",
            params![key, val],
        )?;
    } else {
        // Delete if value is None
        db.conn()
            .execute("DELETE FROM wallet_settings WHERE key = ?1", params![key])?;
    }
    Ok(())
}

pub(super) fn wallet_master_key(wallet_id: &str, passphrase: &str) -> Result<MasterKey> {
    let salt = Sha256::digest(wallet_id.as_bytes());
    AppPassphrase::derive_key(passphrase, &salt[..16])
        .map_err(|e| anyhow!("Failed to derive master key: {}", e))
}

pub(super) fn open_wallet_db_with_passphrase(
    wallet_id: &str,
    passphrase: &str,
) -> Result<(Database, EncryptionKey, MasterKey)> {
    let path = wallet_db_path_for(wallet_id)?;
    // #region agent log
    {
        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let cwd = std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "<unknown>".to_string());
            let path_str = path.to_string_lossy();
            let _ = writeln!(
                file,
                r#"{{"id":"log_db_path","timestamp":{},"location":"encrypted_db.rs:open_wallet_db_with_passphrase","message":"open_wallet_db_with_passphrase","data":{{"wallet_id":"{}","path":{:?},"cwd":{:?}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, wallet_id, path_str, cwd
            );
        });
    }
    // #endregion
    let salt_path = wallet_db_salt_path(wallet_id)?;
    let key_path = wallet_db_key_path(wallet_id)?;
    let master_key = wallet_master_key(wallet_id, passphrase)?;
    let legacy_key = EncryptionKey::from_legacy_password(&format!("{}:{}", wallet_id, passphrase));
    let key_id = format!("pirate_wallet_{}_db", wallet_id);

    let (db, key) = open_encrypted_db_with_migration(
        &path,
        passphrase,
        &salt_path,
        &key_path,
        Some(legacy_key),
        &master_key,
        &key_id,
    )?;

    Repository::new(&db).normalize_wallet_secrets_storage()?;

    Ok((db, key, master_key))
}

pub(super) fn wallet_db_keys(wallet_id: &str) -> Result<(EncryptionKey, MasterKey)> {
    let passphrase = app_passphrase()?;
    let (_db, key, master_key) = open_wallet_db_with_passphrase(wallet_id, &passphrase)?;
    Ok((key, master_key))
}

fn invalidate_wallet_db_cache_epoch() {
    WALLET_DB_CACHE_EPOCH.fetch_add(1, Ordering::SeqCst);
}

fn sync_wallet_db_cache_epoch() {
    let current_epoch = WALLET_DB_CACHE_EPOCH.load(Ordering::SeqCst);
    WALLET_DB_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.epoch != current_epoch {
            cache.entries.clear();
            cache.epoch = current_epoch;
        }
    });
}

pub(super) fn invalidate_wallet_db_cache_for(wallet_id: &str) {
    WALLET_DB_CACHE.with(|cache| {
        cache.borrow_mut().entries.remove(wallet_id);
    });
    invalidate_wallet_db_cache_epoch();
}

pub(super) fn invalidate_all_wallet_db_caches() {
    WALLET_DB_CACHE.with(|cache| {
        cache.borrow_mut().entries.clear();
    });
    invalidate_wallet_db_cache_epoch();
}

pub(super) fn open_wallet_db_for(wallet_id: &str) -> Result<(Rc<Database>, Repository<'static>)> {
    let passphrase = app_passphrase()?;
    sync_wallet_db_cache_epoch();
    ensure_wallet_registry_loaded()?;
    if !WALLETS.read().iter().any(|wallet| wallet.id == wallet_id) {
        return Err(anyhow!("Wallet not found: {}", wallet_id));
    }

    if WALLET_DB_CACHE.with(|cache| cache.borrow().entries.contains_key(wallet_id)) {
        return WALLET_DB_CACHE.with(|cache| {
            let borrowed = cache.borrow();
            let db = borrowed
                .entries
                .get(wallet_id)
                .cloned()
                .ok_or_else(|| anyhow!("Wallet database cache miss"))?;
            let repo = Repository::from_shared(Rc::clone(&db));
            Ok((db, repo))
        });
    }

    let (db, _key, _master_key) = open_wallet_db_with_passphrase(wallet_id, &passphrase)?;
    WALLET_DB_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entries
            .insert(wallet_id.to_string(), Rc::new(db));
    });

    WALLET_DB_CACHE.with(|cache| {
        let borrowed = cache.borrow();
        let db = borrowed
            .entries
            .get(wallet_id)
            .cloned()
            .ok_or_else(|| anyhow!("Wallet database cache miss after insert"))?;
        let repo = Repository::from_shared(Rc::clone(&db));
        Ok((db, repo))
    })
}

pub(super) fn set_app_passphrase(passphrase: String) -> Result<()> {
    let app_passphrase = AppPassphrase::hash(&passphrase)
        .map_err(|e| anyhow!("Failed to hash passphrase: {}", e))?;

    let registry_db = open_wallet_registry_with_passphrase(&passphrase)?;
    set_registry_setting(
        &registry_db,
        REGISTRY_APP_PASSPHRASE_KEY,
        Some(app_passphrase.hash_string()),
    )?;
    passphrase_store::set_passphrase(passphrase);
    invalidate_all_wallet_db_caches();
    Ok(())
}

pub(super) fn has_app_passphrase() -> Result<bool> {
    Ok(wallet_registry_path()?.exists())
}

fn verify_app_passphrase_with_db(db: &Database, passphrase: &str) -> Result<bool> {
    match get_registry_setting(db, REGISTRY_APP_PASSPHRASE_KEY) {
        Ok(Some(stored_hash)) => {
            let app_passphrase = AppPassphrase::from_hash(stored_hash);
            Ok(app_passphrase
                .verify(passphrase)
                .map_err(|e| anyhow!("Passphrase verification failed: {}", e))
                .unwrap_or(false))
        }
        Ok(None) => {
            tracing::warn!(
                "App passphrase hash not found in database, but database opened successfully"
            );
            Ok(true)
        }
        Err(e) => {
            tracing::error!("Failed to read passphrase hash from database: {}", e);
            Ok(false)
        }
    }
}

pub(super) fn verify_app_passphrase(passphrase: String) -> Result<bool> {
    let path = wallet_registry_path()?;
    if !path.exists() {
        return Err(anyhow!("Wallet registry database not found"));
    }

    let result = match open_wallet_registry_with_passphrase(&passphrase) {
        Ok(db) => verify_app_passphrase_with_db(&db, &passphrase)?,
        Err(e) => {
            tracing::debug!("Failed to open database with provided passphrase: {}", e);
            false
        }
    };

    Ok(result)
}

pub(super) fn unlock_app(passphrase: String) -> Result<()> {
    let path = wallet_registry_path()?;
    if !path.exists() {
        return Err(anyhow!("Wallet registry database not found"));
    }

    let db = open_wallet_registry_with_passphrase(&passphrase)?;
    let is_valid = verify_app_passphrase_with_db(&db, &passphrase)?;
    if !is_valid {
        return Err(anyhow!("Invalid passphrase"));
    }

    {
        panic_duress::deactivate_decoy();
    }

    passphrase_store::set_passphrase(passphrase);
    REGISTRY_LOADED.store(false, Ordering::SeqCst);
    invalidate_all_wallet_db_caches();
    load_wallet_registry_state(&db)?;

    tracing::info!("App unlocked successfully");
    Ok(())
}

fn reencrypt_blob(old_key: &MasterKey, new_key: &MasterKey, blob: &[u8]) -> Result<Vec<u8>> {
    let plaintext = old_key
        .decrypt(blob)
        .map_err(|e| anyhow!("Failed to decrypt existing data: {}", e))?;
    new_key
        .encrypt(&plaintext)
        .map_err(|e| anyhow!("Failed to encrypt with new key: {}", e))
}

fn reencrypt_optional_blob(
    old_key: &MasterKey,
    new_key: &MasterKey,
    blob: Option<Vec<u8>>,
) -> Result<Option<Vec<u8>>> {
    match blob {
        Some(value) => Ok(Some(reencrypt_blob(old_key, new_key, &value)?)),
        None => Ok(None),
    }
}

fn reencrypt_wallet_tables(
    conn: &rusqlite::Connection,
    old_key: &MasterKey,
    new_key: &MasterKey,
) -> Result<()> {
    {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, value, nullifier, commitment, spent, height, txid, output_index, spent_txid, diversifier, note, position, memo, address_id, key_id FROM notes",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, Vec<u8>>(6)?,
                row.get::<_, Vec<u8>>(7)?,
                row.get::<_, Vec<u8>>(8)?,
                row.get::<_, Option<Vec<u8>>>(9)?,
                row.get::<_, Option<Vec<u8>>>(10)?,
                row.get::<_, Option<Vec<u8>>>(11)?,
                row.get::<_, Option<Vec<u8>>>(12)?,
                row.get::<_, Option<Vec<u8>>>(13)?,
                row.get::<_, Option<Vec<u8>>>(14)?,
                row.get::<_, Option<Vec<u8>>>(15)?,
            ))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (
            id,
            account_id,
            value,
            nullifier,
            commitment,
            spent,
            height,
            txid,
            output_index,
            spent_txid,
            diversifier,
            note,
            position,
            memo,
            address_id,
            key_id,
        ) in rows_cache
        {
            conn.execute(
                "UPDATE notes SET account_id = ?1, value = ?2, nullifier = ?3, commitment = ?4, spent = ?5, height = ?6, txid = ?7, output_index = ?8, spent_txid = ?9, diversifier = ?10, note = ?11, position = ?12, memo = ?13, address_id = ?14, key_id = ?15 WHERE id = ?16",
                params![
                    reencrypt_blob(old_key, new_key, &account_id)?,
                    reencrypt_blob(old_key, new_key, &value)?,
                    reencrypt_blob(old_key, new_key, &nullifier)?,
                    reencrypt_blob(old_key, new_key, &commitment)?,
                    reencrypt_blob(old_key, new_key, &spent)?,
                    reencrypt_blob(old_key, new_key, &height)?,
                    reencrypt_blob(old_key, new_key, &txid)?,
                    reencrypt_blob(old_key, new_key, &output_index)?,
                    reencrypt_optional_blob(old_key, new_key, spent_txid)?,
                    reencrypt_optional_blob(old_key, new_key, diversifier)?,
                    reencrypt_optional_blob(old_key, new_key, note)?,
                    reencrypt_optional_blob(old_key, new_key, position)?,
                    reencrypt_optional_blob(old_key, new_key, memo)?,
                    reencrypt_optional_blob(old_key, new_key, address_id)?,
                    reencrypt_optional_blob(old_key, new_key, key_id)?,
                    id,
                ],
            )?;
        }
    }

    {
        let mut stmt = conn.prepare(
            "SELECT rowid, wallet_id, account_id, extsk, dfvk, orchard_extsk, sapling_ivk, orchard_ivk, encrypted_mnemonic, created_at FROM wallet_secrets",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, Option<Vec<u8>>>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
                row.get::<_, Option<Vec<u8>>>(6)?,
                row.get::<_, Option<Vec<u8>>>(7)?,
                row.get::<_, Option<Vec<u8>>>(8)?,
                row.get::<_, Vec<u8>>(9)?,
            ))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (
            row_id,
            wallet_id,
            account_id,
            extsk,
            dfvk,
            orchard_extsk,
            sapling_ivk,
            orchard_ivk,
            encrypted_mnemonic,
            created_at,
        ) in rows_cache
        {
            conn.execute(
                "UPDATE wallet_secrets SET wallet_id = ?1, account_id = ?2, extsk = ?3, dfvk = ?4, orchard_extsk = ?5, sapling_ivk = ?6, orchard_ivk = ?7, encrypted_mnemonic = ?8, created_at = ?9 WHERE rowid = ?10",
                params![
                    wallet_id,
                    reencrypt_blob(old_key, new_key, &account_id)?,
                    reencrypt_blob(old_key, new_key, &extsk)?,
                    reencrypt_optional_blob(old_key, new_key, dfvk)?,
                    reencrypt_optional_blob(old_key, new_key, orchard_extsk)?,
                    reencrypt_optional_blob(old_key, new_key, sapling_ivk)?,
                    reencrypt_optional_blob(old_key, new_key, orchard_ivk)?,
                    reencrypt_optional_blob(old_key, new_key, encrypted_mnemonic)?,
                    reencrypt_blob(old_key, new_key, &created_at)?,
                    row_id,
                ],
            )?;
        }
    }

    {
        let mut stmt = conn.prepare("SELECT id, memo FROM memos")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (id, memo) in rows_cache {
            conn.execute(
                "UPDATE memos SET memo = ?1 WHERE id = ?2",
                params![reencrypt_blob(old_key, new_key, &memo)?, id],
            )?;
        }
    }

    {
        let mut stmt = conn.prepare("SELECT height, frontier FROM frontier_snapshots")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        let mut rows_cache = Vec::new();
        for row in rows {
            rows_cache.push(row?);
        }
        drop(stmt);

        for (height, frontier) in rows_cache {
            conn.execute(
                "UPDATE frontier_snapshots SET frontier = ?1 WHERE height = ?2",
                params![reencrypt_blob(old_key, new_key, &frontier)?, height],
            )?;
        }
    }

    Ok(())
}

pub(super) fn change_app_passphrase(
    current_passphrase: String,
    new_passphrase: String,
) -> Result<()> {
    AppPassphrase::validate(&new_passphrase)
        .map_err(|e| anyhow!("New passphrase does not meet requirements: {}", e))?;
    if !verify_app_passphrase(current_passphrase.clone())? {
        return Err(anyhow!("Invalid current passphrase"));
    }

    passphrase_store::set_passphrase(current_passphrase.clone());
    REGISTRY_LOADED.store(false, Ordering::SeqCst);
    invalidate_all_wallet_db_caches();

    struct WalletRekey {
        wallet_id: String,
        old_db_key: [u8; 32],
        old_master_key: MasterKey,
        new_master_key: MasterKey,
    }

    let rollback_wallets = |updated: &[WalletRekey]| -> Result<()> {
        for info in updated.iter().rev() {
            let (mut db, _key, _master_key) =
                open_wallet_db_with_passphrase(&info.wallet_id, &new_passphrase)?;
            let old_db_key = EncryptionKey::from_bytes(info.old_db_key);
            db.rekey(&old_db_key)?;
            let tx = db.transaction()?;
            reencrypt_wallet_tables(&tx, &info.new_master_key, &info.old_master_key)?;
            tx.commit()?;
            let key_path = wallet_db_key_path(&info.wallet_id)?;
            let key_id = format!("pirate_wallet_{}_db", info.wallet_id);
            let _ = force_store_sealed_db_key(&old_db_key, &key_id, &key_path);
        }
        Ok(())
    };

    let wallet_ids: Vec<String> = {
        ensure_wallet_registry_loaded()?;
        WALLETS.read().iter().map(|w| w.id.clone()).collect()
    };

    let registry_db = open_wallet_registry_with_passphrase(&current_passphrase)?;
    let registry_salt = load_salt(&wallet_registry_salt_path()?)?;
    let old_registry_key = derive_db_key(&current_passphrase, &registry_salt)?;
    let new_registry_key = derive_db_key(&new_passphrase, &registry_salt)?;
    let _ = registry_master_key(&new_passphrase)?;

    let mut updated_wallets = Vec::new();
    for wallet_id in &wallet_ids {
        let (mut db, old_db_key, old_master_key) =
            open_wallet_db_with_passphrase(wallet_id, &current_passphrase)?;
        let wallet_salt = load_salt(&wallet_db_salt_path(wallet_id)?)?;
        let new_db_key = derive_db_key(&new_passphrase, &wallet_salt)?;
        let new_master_key = wallet_master_key(wallet_id, &new_passphrase)?;

        if let Err(e) = db.rekey(&new_db_key) {
            let _ = rollback_wallets(&updated_wallets);
            return Err(anyhow!(
                "Failed to rekey wallet database {}: {}",
                wallet_id,
                e
            ));
        }

        let reencrypt_result: Result<()> = {
            let tx = db.transaction()?;
            if let Err(e) = reencrypt_wallet_tables(&tx, &old_master_key, &new_master_key) {
                let _ = tx.rollback();
                return Err(e);
            }
            tx.commit()
                .map_err(|e| anyhow!("Failed to commit re-encrypted wallet data: {}", e))?;
            Ok(())
        };

        if let Err(e) = reencrypt_result {
            let _ = db.rekey(&old_db_key);
            let _ = rollback_wallets(&updated_wallets);
            return Err(anyhow!(
                "Failed to re-encrypt wallet data {}: {}",
                wallet_id,
                e
            ));
        }

        let key_path = wallet_db_key_path(wallet_id)?;
        let key_id = format!("pirate_wallet_{}_db", wallet_id);
        let _ = force_store_sealed_db_key(&new_db_key, &key_id, &key_path);
        updated_wallets.push(WalletRekey {
            wallet_id: wallet_id.clone(),
            old_db_key: *old_db_key.as_bytes(),
            old_master_key,
            new_master_key,
        });
    }

    registry_db.rekey(&new_registry_key).map_err(|e| {
        let _ = rollback_wallets(&updated_wallets);
        anyhow!("Failed to rekey registry database: {}", e)
    })?;

    let new_hash = AppPassphrase::hash(&new_passphrase)
        .map_err(|e| anyhow!("Failed to hash new passphrase: {}", e))?;
    if let Err(e) = set_registry_setting(
        &registry_db,
        REGISTRY_APP_PASSPHRASE_KEY,
        Some(new_hash.hash_string()),
    ) {
        let _ = registry_db.rekey(&old_registry_key);
        let _ = rollback_wallets(&updated_wallets);
        return Err(anyhow!("Failed to update passphrase hash: {}", e));
    }

    if let Err(e) = panic_duress::refresh_duress_reverse_hash(&registry_db, &new_passphrase) {
        tracing::warn!("Failed to refresh duress passphrase: {}", e);
        let _ = set_registry_setting(
            &registry_db,
            panic_duress::REGISTRY_DURESS_PASSPHRASE_HASH_KEY,
            None,
        );
        let _ = set_registry_setting(
            &registry_db,
            panic_duress::REGISTRY_DURESS_USE_REVERSE_KEY,
            None,
        );
    }

    let registry_key_path = wallet_registry_key_path()?;
    let _ = force_store_sealed_db_key(
        &new_registry_key,
        "pirate_wallet_registry_db",
        &registry_key_path,
    );

    passphrase_store::set_passphrase(new_passphrase);
    REGISTRY_LOADED.store(false, Ordering::SeqCst);
    invalidate_all_wallet_db_caches();
    ensure_wallet_registry_loaded()?;

    sync_control::clear_passphrase_change_sync_state();
    tracing::info!("App passphrase updated successfully");
    Ok(())
}

pub(super) fn change_app_passphrase_with_cached(new_passphrase: String) -> Result<()> {
    let current = app_passphrase()?;
    change_app_passphrase(current, new_passphrase)
}

pub(super) fn reseal_db_keys_for_biometrics() -> Result<()> {
    let passphrase = app_passphrase()?;
    let mut resealed = 0;

    if reseal_registry_db_key(&passphrase)? {
        resealed += 1;
    }

    ensure_wallet_registry_loaded()?;
    let wallet_ids: Vec<String> = WALLETS.read().iter().map(|w| w.id.clone()).collect();
    for wallet_id in wallet_ids {
        if reseal_wallet_db_key(&wallet_id, &passphrase)? {
            resealed += 1;
        }
    }

    tracing::info!(
        "Resealed {} database key(s) using current keystore mode",
        resealed
    );
    Ok(())
}
