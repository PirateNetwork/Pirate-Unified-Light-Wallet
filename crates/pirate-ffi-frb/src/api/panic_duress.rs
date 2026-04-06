use super::*;
use pirate_storage_sqlite::{DecoyVaultManager, PanicPin, VaultMode};

pub(super) const REGISTRY_DURESS_PASSPHRASE_HASH_KEY: &str = "duress_passphrase_hash";
pub(super) const REGISTRY_DURESS_USE_REVERSE_KEY: &str = "duress_passphrase_use_reverse";
pub(super) const DECOY_WALLET_ID: &str = "decoy_wallet";

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

    let vault = DECOY_VAULT.read();
    let salt = generate_salt().to_vec();
    vault
        .enable(duress_hash.hash_string().to_string(), salt)
        .map_err(|e| anyhow!("Failed to enable decoy vault: {}", e))?;

    tracing::info!("Duress passphrase configured");
    Ok(())
}

pub(super) fn has_duress_passphrase() -> Result<bool> {
    if !wallet_registry_path()?.exists() {
        return Ok(false);
    }
    let registry_db = open_wallet_registry()?;
    Ok(get_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY)?.is_some())
}

pub(super) fn clear_duress_passphrase() -> Result<()> {
    if wallet_registry_path()?.exists() {
        let registry_db = open_wallet_registry()?;
        set_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY, None)?;
        set_registry_setting(&registry_db, REGISTRY_DURESS_USE_REVERSE_KEY, None)?;
    }

    let vault = DECOY_VAULT.read();
    vault
        .disable()
        .map_err(|e| anyhow!("Failed to disable decoy vault: {}", e))?;
    tracing::info!("Duress passphrase cleared");
    Ok(())
}

pub(super) fn verify_duress_passphrase(passphrase: String) -> Result<bool> {
    if !wallet_registry_path()?.exists() {
        return Ok(false);
    }

    let registry_db = open_wallet_registry()?;
    let Some(hash) = get_registry_setting(&registry_db, REGISTRY_DURESS_PASSPHRASE_HASH_KEY)?
    else {
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
