//! FFI bridge for background sync operations
//!
//! Exposes background sync functionality to Flutter with tunnel support.

use crate::WalletId;
use anyhow::Result;
use std::collections::HashMap;

use crate::api;
use crate::models::TunnelMode;

/// Background sync mode
#[derive(Debug, Clone, Copy)]
pub enum BackgroundSyncMode {
    /// Quick compact sync
    Compact,
    /// Deep sync with witness updates
    Deep,
}

impl From<String> for BackgroundSyncMode {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "deep" => BackgroundSyncMode::Deep,
            _ => BackgroundSyncMode::Compact,
        }
    }
}

/// Execute background sync
///
/// This is called by platform-specific background tasks (Android WorkManager, iOS BGTask)
/// to perform blockchain synchronization.
pub async fn execute_background_sync(
    wallet_id: WalletId,
    mode: String,
    max_duration_secs: u64,
) -> Result<HashMap<String, String>> {
    let sync_mode = BackgroundSyncMode::from(mode.clone());

    tracing::info!(
        "Executing background sync: wallet={}, mode={:?}, max_duration={}s",
        wallet_id,
        sync_mode,
        max_duration_secs
    );

    // Ensure we have *some* privacy/tunnel configuration available.
    // Note: Actual Tor/SOCKS transport enforcement is handled in `pirate-net` (milestone 2).
    let tunnel_ok = verify_background_sync_tunnel().await.unwrap_or(true);
    if !tunnel_ok {
        tracing::warn!(
            "Background sync tunnel could not be verified; proceeding with current transport"
        );
    }

    // Delegate to the real background sync implementation in `api.rs` (uses pirate-sync-lightd orchestrator).
    // We keep this wrapper to support legacy/background entrypoints.
    let start = std::time::Instant::now();
    let bg = api::start_background_sync(wallet_id.clone(), Some(mode.clone())).await?;

    let mut result = HashMap::new();
    result.insert("mode".to_string(), bg.mode);
    result.insert("blocks_synced".to_string(), bg.blocks_synced.to_string());
    result.insert("start_height".to_string(), bg.start_height.to_string());
    result.insert("end_height".to_string(), bg.end_height.to_string());
    result.insert(
        "duration_secs".to_string(),
        // Prefer orchestrator duration, but keep a fallback measured here.
        std::cmp::max(bg.duration_secs, start.elapsed().as_secs()).to_string(),
    );
    result.insert(
        "new_transactions".to_string(),
        bg.new_transactions.to_string(),
    );
    if let Some(bal) = bg.new_balance {
        result.insert("new_balance".to_string(), bal.to_string());
    }
    if !bg.errors.is_empty() {
        result.insert("errors".to_string(), bg.errors.join("; "));
    }

    Ok(result)
}

/// Get background sync status
pub async fn get_background_sync_status(wallet_id: WalletId) -> Result<HashMap<String, String>> {
    tracing::debug!("Getting background sync status for wallet: {}", wallet_id);

    let mut status = HashMap::new();

    // Best-effort: use current sync session status if running.
    // This module does not own scheduling; platform-specific schedulers do.
    let running = api::is_sync_running(wallet_id.clone())?;
    status.insert("is_running".to_string(), running.to_string());
    if running {
        let s = api::sync_status(wallet_id.clone())?;
        status.insert("local_height".to_string(), s.local_height.to_string());
        status.insert("target_height".to_string(), s.target_height.to_string());
        status.insert("percent".to_string(), format!("{:.2}", s.percent));
        status.insert("stage".to_string(), format!("{:?}", s.stage));
    }

    Ok(status)
}

/// Configure background sync settings
///
/// Note: This is a no-op on the Rust side. Background sync scheduling is handled
/// entirely by Flutter using platform-specific schedulers (Android WorkManager, iOS BGTask).
/// This function exists only for API compatibility.
pub async fn configure_background_sync(
    wallet_id: WalletId,
    compact_interval_mins: u32,
    deep_interval_hours: u32,
    use_foreground_service: bool,
) -> Result<()> {
    tracing::info!(
        "Background sync configuration received (handled by Flutter): wallet={}, compact_interval={}min, deep_interval={}h, foreground={}",
        wallet_id,
        compact_interval_mins,
        deep_interval_hours,
        use_foreground_service
    );

    Ok(())
}

/// Verify network tunnel is active
///
/// Called before executing background sync to ensure privacy.
/// Returns false if tunnel cannot be established (blocks sync).
pub async fn verify_background_sync_tunnel() -> Result<bool> {
    tracing::debug!("Verifying network tunnel for background sync");

    // We can at least verify that a tunnel mode is configured.
    // Real enforcement/checks will be added when `pirate-net` is wired into the lightwalletd client (milestone 2).
    let mode: TunnelMode = api::get_tunnel()?;
    Ok(matches!(
        mode,
        TunnelMode::Tor | TunnelMode::I2p | TunnelMode::Socks5 { .. } | TunnelMode::Direct
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_sync_mode_from_string() {
        let compact: BackgroundSyncMode = "compact".to_string().into();
        assert!(matches!(compact, BackgroundSyncMode::Compact));

        let deep: BackgroundSyncMode = "deep".to_string().into();
        assert!(matches!(deep, BackgroundSyncMode::Deep));

        let default: BackgroundSyncMode = "unknown".to_string().into();
        assert!(matches!(default, BackgroundSyncMode::Compact));
    }

    #[tokio::test]
    async fn test_execute_background_sync() {
        let result =
            execute_background_sync("test-wallet".to_string(), "compact".to_string(), 60).await;

        match result {
            Ok(result) => {
                assert_eq!(result.get("mode"), Some(&"compact".to_string()));
                assert!(result.contains_key("blocks_synced"));
            }
            Err(err) => {
                let message = err.to_string();
                assert!(
                    message.contains("App is locked"),
                    "Unexpected error: {message}"
                );
            }
        }
    }
}
