use super::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckpointInfo {
    pub height: u32,
    pub timestamp: i64,
}

pub(super) fn get_build_info() -> Result<BuildInfo> {
    convert_from_service(service::get_build_info()?)
}

pub(super) fn get_sync_logs(
    wallet_id: WalletId,
    limit: Option<u32>,
) -> Result<Vec<crate::models::SyncLogEntryFfi>> {
    convert_from_service(service::get_sync_logs(wallet_id, limit)?)
}

pub(super) fn get_checkpoint_details(
    wallet_id: WalletId,
    height: u32,
) -> Result<Option<CheckpointInfo>> {
    convert_from_service(service::get_checkpoint_details(wallet_id, height)?)
}
