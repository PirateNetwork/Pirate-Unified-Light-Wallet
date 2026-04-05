use crate::models::WalletId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use pirate_storage_sqlite::{seed_warnings, ExportFlowState, SeedExportManager};
use std::sync::Arc;

lazy_static::lazy_static! {
    /// Global seed export manager
    static ref SEED_EXPORT: Arc<RwLock<SeedExportManager>> =
        Arc::new(RwLock::new(SeedExportManager::new()));
}

pub(super) fn start_seed_export(wallet_id: WalletId) -> Result<String> {
    super::panic_duress::ensure_not_decoy("Seed export")?;
    let wallet = super::get_wallet_meta(&wallet_id)?;

    if wallet.watch_only {
        return Err(anyhow!("Cannot export seed from watch-only wallet"));
    }

    let manager = SEED_EXPORT.write();
    let state = manager
        .start_export(wallet_id)
        .map_err(|e| anyhow!("Failed to start export: {}", e))?;

    Ok(format!("{:?}", state))
}

pub(super) fn acknowledge_seed_warning() -> Result<String> {
    super::panic_duress::ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.write();
    let state = manager
        .acknowledge_warning()
        .map_err(|e| anyhow!("Failed to acknowledge: {}", e))?;

    Ok(format!("{:?}", state))
}

pub(super) fn complete_seed_biometric(success: bool) -> Result<String> {
    super::panic_duress::ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.write();
    let state = manager
        .complete_biometric(success)
        .map_err(|e| anyhow!("Failed to complete biometric: {}", e))?;

    Ok(format!("{:?}", state))
}

pub(super) fn skip_seed_biometric() -> Result<String> {
    super::panic_duress::ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.write();
    let state = manager
        .skip_biometric()
        .map_err(|e| anyhow!("Failed to skip biometric: {}", e))?;

    Ok(format!("{:?}", state))
}

pub(super) fn export_seed_with_passphrase(
    wallet_id: WalletId,
    passphrase: String,
) -> Result<Vec<String>> {
    super::panic_duress::ensure_not_decoy("Seed export")?;
    let manager = SEED_EXPORT.read();

    if manager.state() != ExportFlowState::AwaitingPassphrase {
        return Err(anyhow!(
            "Invalid export flow state. Complete previous steps first."
        ));
    }
    manager.ensure_wallet_id(&wallet_id)?;
    drop(manager);

    let wallet = super::get_wallet_meta(&wallet_id)?;
    if wallet.watch_only {
        return Err(anyhow!("Cannot export seed from watch-only wallet"));
    }

    let registry_db = super::encrypted_db::open_wallet_registry()?;
    let passphrase_hash = super::encrypted_db::get_registry_setting(
        &registry_db,
        super::REGISTRY_APP_PASSPHRASE_KEY,
    )?
    .ok_or_else(|| anyhow!("Passphrase not configured"))?;

    {
        let manager = SEED_EXPORT.write();
        manager.set_passphrase_hash(passphrase_hash);
    }
    {
        let manager = SEED_EXPORT.read();
        let verified = manager.verify_passphrase(&passphrase)?;
        if !verified {
            return Err(anyhow!("Invalid passphrase"));
        }
    }

    let (_db, repo) = super::encrypted_db::open_wallet_db_for(&wallet_id)?;
    let secret = repo
        .get_wallet_secret(&wallet_id)?
        .ok_or_else(|| anyhow!("Wallet secret not found for {}", wallet_id))?;

    let mnemonic_bytes = secret.encrypted_mnemonic.ok_or_else(|| {
        anyhow!("Seed not available. This wallet was imported from private key or is watch-only.")
    })?;

    let mnemonic = String::from_utf8(mnemonic_bytes)
        .map_err(|e| anyhow!("Failed to decode mnemonic: {}", e))?;
    let words: Vec<String> = mnemonic.split_whitespace().map(String::from).collect();

    let result = {
        let manager = SEED_EXPORT.read();
        manager.complete_export_verified(&wallet_id, words)?
    };

    tracing::info!(
        "Seed exported for wallet {} (gated flow completed)",
        wallet_id
    );

    Ok(result.words().to_vec())
}

pub(super) fn export_seed_with_cached_passphrase(wallet_id: WalletId) -> Result<Vec<String>> {
    super::panic_duress::ensure_not_decoy("Seed export")?;
    {
        let manager = SEED_EXPORT.read();
        manager.ensure_wallet_id(&wallet_id)?;
    }
    let passphrase = super::encrypted_db::app_passphrase()?;
    export_seed_with_passphrase(wallet_id, passphrase)
}

pub(super) fn cancel_seed_export() -> Result<()> {
    let manager = SEED_EXPORT.write();
    manager.cancel();
    Ok(())
}

pub(super) fn get_seed_export_state() -> Result<String> {
    let manager = SEED_EXPORT.read();
    Ok(format!("{:?}", manager.state()))
}

pub(super) fn are_seed_screenshots_blocked() -> Result<bool> {
    let manager = SEED_EXPORT.read();
    Ok(manager.are_screenshots_blocked())
}

pub(super) fn get_seed_clipboard_remaining() -> Result<Option<u64>> {
    let manager = SEED_EXPORT.read();
    Ok(manager.clipboard_remaining_seconds())
}

pub(super) fn get_seed_export_warnings() -> Result<SeedExportWarnings> {
    Ok(SeedExportWarnings {
        primary: seed_warnings::PRIMARY_WARNING.to_string(),
        secondary: seed_warnings::SECONDARY_WARNING.to_string(),
        backup_instructions: seed_warnings::BACKUP_INSTRUCTIONS.to_string(),
        clipboard_warning: seed_warnings::CLIPBOARD_WARNING.to_string(),
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SeedExportWarnings {
    pub primary: String,
    pub secondary: String,
    pub backup_instructions: String,
    pub clipboard_warning: String,
}
