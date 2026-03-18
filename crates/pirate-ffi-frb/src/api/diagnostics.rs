use super::*;
use pirate_storage_sqlite::CheckpointManager;

/// Checkpoint information for diagnostics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckpointInfo {
    /// Checkpoint block height
    pub height: u32,
    /// Unix timestamp when checkpoint was created
    pub timestamp: i64,
}

/// Get build information for verification
pub(super) fn get_build_info() -> Result<BuildInfo> {
    Ok(BuildInfo {
        version: option_env!("APP_VERSION")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .to_string(),
        git_commit: option_env!("GIT_COMMIT").unwrap_or("unknown").to_string(),
        build_date: option_env!("BUILD_DATE").unwrap_or("unknown").to_string(),
        rust_version: option_env!("RUSTC_VERSION")
            .or(option_env!("CARGO_PKG_RUST_VERSION"))
            .unwrap_or("unknown")
            .to_string(),
        target_triple: option_env!("BUILD_TARGET").unwrap_or("unknown").to_string(),
    })
}

/// Get sync logs for diagnostics
pub(super) fn get_sync_logs(
    wallet_id: WalletId,
    limit: Option<u32>,
) -> Result<Vec<crate::models::SyncLogEntryFfi>> {
    if is_decoy_mode_active() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(200);

    let (_db, repo) = open_wallet_db_for(&wallet_id)?;
    let logs = repo.get_sync_logs(&wallet_id, limit)?;

    Ok(logs
        .into_iter()
        .map(
            |(timestamp, level, module, message)| crate::models::SyncLogEntryFfi {
                timestamp,
                level,
                module,
                message,
            },
        )
        .collect())
}

/// Get checkpoint details at specific height
pub(super) fn get_checkpoint_details(
    wallet_id: WalletId,
    height: u32,
) -> Result<Option<CheckpointInfo>> {
    let (db, _repo) = open_wallet_db_for(&wallet_id)?;
    let manager = CheckpointManager::new(db.conn());

    match manager.get_at_height(height)? {
        Some(checkpoint) => Ok(Some(CheckpointInfo {
            height: checkpoint.height,
            timestamp: checkpoint.timestamp,
        })),
        None => Ok(None),
    }
}
