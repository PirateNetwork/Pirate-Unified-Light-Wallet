use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SeedExportWarnings {
    pub primary: String,
    pub secondary: String,
    pub backup_instructions: String,
    pub clipboard_warning: String,
}

pub(super) fn start_seed_export(wallet_id: WalletId) -> Result<String> {
    service::start_seed_export(wallet_id)
}

pub(super) fn acknowledge_seed_warning() -> Result<String> {
    service::acknowledge_seed_warning()
}

pub(super) fn complete_seed_biometric(success: bool) -> Result<String> {
    service::complete_seed_biometric(success)
}

pub(super) fn skip_seed_biometric() -> Result<String> {
    service::skip_seed_biometric()
}

pub(super) fn export_seed_with_passphrase(
    wallet_id: WalletId,
    passphrase: String,
) -> Result<Vec<String>> {
    service::export_seed_with_passphrase(wallet_id, passphrase)
}

pub(super) fn export_seed_with_cached_passphrase(wallet_id: WalletId) -> Result<Vec<String>> {
    service::export_seed_with_cached_passphrase(wallet_id)
}

pub(super) fn cancel_seed_export() -> Result<()> {
    service::cancel_seed_export()
}

pub(super) fn get_seed_export_state() -> Result<String> {
    service::get_seed_export_state()
}

pub(super) fn are_seed_screenshots_blocked() -> Result<bool> {
    service::are_seed_screenshots_blocked()
}

pub(super) fn get_seed_clipboard_remaining() -> Result<Option<u64>> {
    service::get_seed_clipboard_remaining()
}

pub(super) fn get_seed_export_warnings() -> Result<SeedExportWarnings> {
    let warnings = service::get_seed_export_warnings()?;
    Ok(SeedExportWarnings {
        primary: warnings.primary,
        secondary: warnings.secondary,
        backup_instructions: warnings.backup_instructions,
        clipboard_warning: warnings.clipboard_warning,
    })
}
