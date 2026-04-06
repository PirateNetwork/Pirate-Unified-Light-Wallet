use super::*;

pub async fn start_background_sync(
    wallet_id: WalletId,
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::BackgroundSyncResult> {
    convert_from_service(
        service::start_background_sync(wallet_id, mode, max_duration_secs, max_blocks).await?,
    )
}

pub async fn start_background_sync_round_robin(
    mode: Option<String>,
    max_duration_secs: Option<u64>,
    max_blocks: Option<u64>,
) -> Result<crate::models::WalletBackgroundSyncResult> {
    convert_from_service(
        service::start_background_sync_round_robin(mode, max_duration_secs, max_blocks).await?,
    )
}

pub async fn is_background_sync_needed(wallet_id: WalletId) -> Result<bool> {
    service::is_background_sync_needed(wallet_id).await
}

pub fn get_recommended_background_sync_mode(
    wallet_id: WalletId,
    minutes_since_last: u32,
) -> Result<String> {
    service::get_recommended_background_sync_mode(wallet_id, minutes_since_last)
}
