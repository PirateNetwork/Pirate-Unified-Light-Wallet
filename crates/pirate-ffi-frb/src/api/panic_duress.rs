use super::*;
use pirate_storage_sqlite::{DecoyVaultManager, PanicPin, VaultMode};

pub(super) const REGISTRY_DURESS_PASSPHRASE_HASH_KEY: &str = "duress_passphrase_hash";
pub(super) const REGISTRY_DURESS_USE_REVERSE_KEY: &str = "duress_passphrase_use_reverse";
pub(super) const DECOY_WALLET_ID: &str = "decoy_wallet";
const DURESS_PASSPHRASE_SIDECAR_FILENAME: &str = "duress_passphrase.hash";

lazy_static::lazy_static! {
    /// Global decoy vault manager
    static ref DECOY_VAULT: Arc<RwLock<DecoyVaultManager>> =
        Arc::new(RwLock::new(DecoyVaultManager::new()));
}

pub(super) fn is_decoy_mode_active() -> bool {
    let vault = DECOY_VAULT.read();
    vault.is_decoy_mode()
}

pub(super) fn decoy_wallet_meta() -> WalletMeta {
    let vault = DECOY_VAULT.read();
    let network = Network::mainnet();
    WalletMeta {
        id: DECOY_WALLET_ID.to_string(),
        name: vault.decoy_name(),
        created_at: chrono::Utc::now().timestamp(),
        watch_only: false,
        birthday_height: network.default_birthday_height,
        network_type: Some(network.name.to_string()),
    }
}

pub(super) fn ensure_decoy_wallet_state() {
    let meta = decoy_wallet_meta();
    *WALLETS.write() = vec![meta.clone()];
    *ACTIVE_WALLET.write() = Some(meta.id);
}

fn reverse_passphrase(passphrase: &str) -> String {
    passphrase.chars().rev().collect()
}

fn duress_sidecar_path() -> Result<PathBuf> {
    Ok(super::encrypted_db::wallet_base_dir()?.join(DURESS_PASSPHRASE_SIDECAR_FILENAME))
}

fn write_duress_sidecar(hash: &str) -> Result<()> {
    let path = duress_sidecar_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("hash.tmp");
    fs::write(&tmp, hash)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn read_duress_sidecar() -> Result<Option<String>> {
    let path = duress_sidecar_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let value = fs::read_to_string(path)?.trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn clear_duress_sidecar() -> Result<()> {
    let path = duress_sidecar_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn load_duress_hash_locked_safe() -> Result<Option<String>> {
    if wallet_registry_path()?.exists() {
        if let Ok(registry_db) = open_wallet_registry() {
            if let Some(hash) =
                get_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY)?
            {
                let _ = write_duress_sidecar(&hash);
                return Ok(Some(hash));
            }
        }
    }

    read_duress_sidecar()
}

pub(super) fn sync_duress_sidecar_from_registry(registry_db: &Database) {
    match get_registry_setting(registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY) {
        Ok(Some(hash)) => {
            if let Err(e) = write_duress_sidecar(&hash) {
                tracing::warn!("Failed to write locked-state duress verifier: {}", e);
            }
        }
        Ok(None) => {
            if let Err(e) = clear_duress_sidecar() {
                tracing::warn!("Failed to clear locked-state duress verifier: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to read duress verifier from registry: {}", e),
    }
}

pub(super) fn ensure_not_decoy(operation: &str) -> Result<()> {
    if is_decoy_mode_active() {
        return Err(anyhow!("{} is unavailable in decoy mode", operation));
    }
    Ok(())
}

fn validate_custom_duress_passphrase(passphrase: &str) -> Result<()> {
    const SYMBOLS: &str = "!@#$%^&*(),.?\":{}|<>";

    AppPassphrase::validate(passphrase)?;

    if !passphrase.chars().any(|c| c.is_ascii_lowercase()) {
        return Err(anyhow!("Duress passphrase must include a lowercase letter"));
    }
    if !passphrase.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(anyhow!(
            "Duress passphrase must include an uppercase letter"
        ));
    }
    if !passphrase.chars().any(|c| c.is_ascii_digit()) {
        return Err(anyhow!("Duress passphrase must include a number"));
    }
    if !passphrase.chars().any(|c| SYMBOLS.contains(c)) {
        return Err(anyhow!(
            "Duress passphrase must include a symbol (e.g. !@#$)"
        ));
    }

    Ok(())
}

pub(super) fn refresh_duress_reverse_hash(
    registry_db: &Database,
    new_passphrase: &str,
) -> Result<()> {
    let use_reverse = get_registry_setting(registry_db, REGISTRY_DURESS_USE_REVERSE_KEY)?
        .map(|value| value == "true")
        .unwrap_or(false);

    if !use_reverse {
        return Ok(());
    }

    if new_passphrase.chars().eq(new_passphrase.chars().rev()) {
        set_registry_setting(registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY, None)?;
        set_registry_setting(registry_db, REGISTRY_DURESS_USE_REVERSE_KEY, None)?;
        clear_duress_sidecar()?;
        return Ok(());
    }

    let duress_passphrase = reverse_passphrase(new_passphrase);
    let duress_hash = AppPassphrase::hash(&duress_passphrase)
        .map_err(|e| anyhow!("Failed to hash duress passphrase: {}", e))?;
    set_registry_setting(
        registry_db,
        REGISTRY_DURESS_PASSPHRASE_HASH_KEY,
        Some(duress_hash.hash_string()),
    )?;
    write_duress_sidecar(&duress_hash.hash_string())?;
    Ok(())
}

pub(super) fn set_panic_pin(pin: String) -> Result<()> {
    if pin.len() < 4 || pin.len() > 8 {
        return Err(anyhow!("PIN must be 4-8 digits"));
    }

    if !pin.chars().all(|c| c.is_ascii_digit()) {
        return Err(anyhow!("PIN must contain only digits"));
    }

    let panic_pin = PanicPin::hash(&pin).map_err(|e| anyhow!("Failed to hash PIN: {}", e))?;
    let salt = pirate_storage_sqlite::generate_salt().to_vec();

    let vault = DECOY_VAULT.read();
    vault
        .enable(panic_pin.hash_string().to_string(), salt)
        .map_err(|e| anyhow!("Failed to enable decoy vault: {}", e))?;

    tracing::info!("Panic PIN configured and decoy vault enabled");
    Ok(())
}

pub(super) fn has_panic_pin() -> Result<bool> {
    let vault = DECOY_VAULT.read();
    Ok(vault.config().enabled)
}

pub(super) fn verify_panic_pin(pin: String) -> Result<bool> {
    let vault = DECOY_VAULT.read();

    let is_panic = vault
        .verify_panic_pin(&pin)
        .map_err(|e| anyhow!("Failed to verify PIN: {}", e))?;

    if is_panic {
        vault
            .activate_decoy()
            .map_err(|e| anyhow!("Failed to activate decoy: {}", e))?;
        tracing::warn!("Decoy vault activated via panic PIN");
    }

    Ok(is_panic)
}

pub(super) fn is_decoy_mode() -> Result<bool> {
    let vault = DECOY_VAULT.read();
    Ok(vault.is_decoy_mode())
}

pub(super) fn get_vault_mode() -> Result<String> {
    let vault = DECOY_VAULT.read();
    Ok(match vault.mode() {
        VaultMode::Real => "real".to_string(),
        VaultMode::Decoy => "decoy".to_string(),
    })
}

pub(super) fn clear_panic_pin() -> Result<()> {
    let vault = DECOY_VAULT.read();
    vault
        .disable()
        .map_err(|e| anyhow!("Failed to disable decoy vault: {}", e))?;

    tracing::info!("Panic PIN cleared and decoy vault disabled");
    Ok(())
}

pub(super) fn set_duress_passphrase(custom_passphrase: Option<String>) -> Result<()> {
    let app_passphrase =
        passphrase_store::get_passphrase().map_err(|e| anyhow!("App is locked: {}", e))?;
    let app_passphrase = app_passphrase.as_str();

    let custom_trimmed = custom_passphrase
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let use_reverse = custom_trimmed.is_none();
    let duress_passphrase = if let Some(value) = custom_trimmed {
        validate_custom_duress_passphrase(&value)?;
        value
    } else {
        if app_passphrase.chars().eq(app_passphrase.chars().rev()) {
            return Err(anyhow!(
                "Passphrase reads the same forwards and backwards; set a custom duress passphrase"
            ));
        }
        reverse_passphrase(app_passphrase)
    };

    if duress_passphrase == app_passphrase {
        return Err(anyhow!(
            "Duress passphrase must be different from your app passphrase"
        ));
    }

    let duress_hash = AppPassphrase::hash(&duress_passphrase)
        .map_err(|e| anyhow!("Failed to hash duress passphrase: {}", e))?;

    let registry_db = open_wallet_registry()?;
    set_registry_setting(
        &registry_db,
        REGISTRY_DURESS_PASSPHRASE_HASH_KEY,
        Some(duress_hash.hash_string()),
    )?;
    set_registry_setting(
        &registry_db,
        REGISTRY_DURESS_USE_REVERSE_KEY,
        Some(if use_reverse { "true" } else { "false" }),
    )?;
    write_duress_sidecar(&duress_hash.hash_string())?;

    let vault = DECOY_VAULT.read();
    let salt = generate_salt().to_vec();
    vault
        .enable(duress_hash.hash_string().to_string(), salt)
        .map_err(|e| anyhow!("Failed to enable decoy vault: {}", e))?;

    tracing::info!("Duress passphrase configured");
    Ok(())
}

pub(super) fn has_duress_passphrase() -> Result<bool> {
    Ok(load_duress_hash_locked_safe()?.is_some())
}

pub(super) fn clear_duress_passphrase() -> Result<()> {
    if wallet_registry_path()?.exists() {
        match open_wallet_registry() {
            Ok(registry_db) => {
                set_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY, None)?;
                set_registry_setting(&registry_db, REGISTRY_DURESS_USE_REVERSE_KEY, None)?;
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to clear encrypted duress settings; clearing locked-state verifier only: {}",
                    e
                );
            }
        }
    }

    let vault = DECOY_VAULT.read();
    vault
        .disable()
        .map_err(|e| anyhow!("Failed to disable decoy vault: {}", e))?;
    clear_duress_sidecar()?;
    tracing::info!("Duress passphrase cleared");
    Ok(())
}

pub(super) fn verify_duress_passphrase(passphrase: String) -> Result<bool> {
    let Some(hash) = load_duress_hash_locked_safe()? else {
        return Ok(false);
    };

    let verifier = AppPassphrase::from_hash(hash.clone());
    let is_match = verifier
        .verify(&passphrase)
        .map_err(|e| anyhow!("Failed to verify duress passphrase: {}", e))?;

    if is_match {
        let vault = DECOY_VAULT.read();
        if !vault.config().enabled {
            let salt = generate_salt().to_vec();
            let _ = vault.enable(hash, salt);
        }
        vault
            .activate_decoy()
            .map_err(|e| anyhow!("Failed to activate decoy: {}", e))?;
        ensure_decoy_wallet_state();
        tracing::warn!("Decoy vault activated via duress passphrase");
    }

    Ok(is_match)
}

pub(super) fn set_decoy_wallet_name(name: String) -> Result<()> {
    let vault = DECOY_VAULT.read();
    vault.set_decoy_name(name);
    Ok(())
}

pub(super) fn exit_decoy_mode(passphrase: String) -> Result<()> {
    super::encrypted_db::unlock_app(passphrase)?;
    tracing::info!("Exited decoy mode via real passphrase re-authentication");
    Ok(())
}

pub(super) fn deactivate_decoy() {
    let vault = DECOY_VAULT.read();
    vault.deactivate_decoy();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pirate_ffi_duress_{}_{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn reset_test_state() {
        passphrase_store::clear_passphrase();
        REGISTRY_LOADED.store(false, Ordering::SeqCst);
        *WALLETS.write() = Vec::new();
        *ACTIVE_WALLET.write() = None;
        deactivate_decoy();
        let _ = clear_duress_passphrase();
        let _ = clear_panic_pin();
    }

    fn simulate_locked_restart() {
        passphrase_store::clear_passphrase();
        REGISTRY_LOADED.store(false, Ordering::SeqCst);
        *WALLETS.write() = Vec::new();
        *ACTIVE_WALLET.write() = None;
        deactivate_decoy();
    }

    #[test]
    fn custom_duress_works_after_locked_restart() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let temp_dir = test_dir("custom");
        std::env::set_var("PIRATE_WALLET_DB_DIR", &temp_dir);
        reset_test_state();

        let real_passphrase = "RealPass123!";
        let duress_passphrase = "DuressPass123!";

        encrypted_db::set_app_passphrase(real_passphrase.to_string()).unwrap();
        set_duress_passphrase(Some(duress_passphrase.to_string())).unwrap();
        simulate_locked_restart();

        assert!(has_duress_passphrase().unwrap());
        assert!(verify_duress_passphrase(duress_passphrase.to_string()).unwrap());
        assert!(is_decoy_mode_active());

        reset_test_state();
        std::env::remove_var("PIRATE_WALLET_DB_DIR");
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn default_reverse_duress_works_after_locked_restart() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let temp_dir = test_dir("reverse");
        std::env::set_var("PIRATE_WALLET_DB_DIR", &temp_dir);
        reset_test_state();

        let real_passphrase = "RealPass123!";
        let duress_passphrase: String = real_passphrase.chars().rev().collect();

        encrypted_db::set_app_passphrase(real_passphrase.to_string()).unwrap();
        set_duress_passphrase(None).unwrap();
        simulate_locked_restart();

        assert!(has_duress_passphrase().unwrap());
        assert!(verify_duress_passphrase(duress_passphrase).unwrap());
        assert!(is_decoy_mode_active());

        reset_test_state();
        std::env::remove_var("PIRATE_WALLET_DB_DIR");
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn normal_unlock_migrates_existing_duress_registry_hash() {
        let _guard = TEST_MUTEX.lock().unwrap();
        let temp_dir = test_dir("migration");
        std::env::set_var("PIRATE_WALLET_DB_DIR", &temp_dir);
        reset_test_state();

        let real_passphrase = "RealPass123!";
        let duress_passphrase = "DuressPass123!";

        encrypted_db::set_app_passphrase(real_passphrase.to_string()).unwrap();
        set_duress_passphrase(Some(duress_passphrase.to_string())).unwrap();
        clear_duress_sidecar().unwrap();
        simulate_locked_restart();

        assert!(!has_duress_passphrase().unwrap());
        encrypted_db::unlock_app(real_passphrase.to_string()).unwrap();
        simulate_locked_restart();

        assert!(has_duress_passphrase().unwrap());
        assert!(verify_duress_passphrase(duress_passphrase.to_string()).unwrap());
        assert!(is_decoy_mode_active());

        reset_test_state();
        std::env::remove_var("PIRATE_WALLET_DB_DIR");
        let _ = fs::remove_dir_all(temp_dir);
    }
}
