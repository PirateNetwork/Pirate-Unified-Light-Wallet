//! Comprehensive logging for background sync observability
//!
//! Provides structured logging for monitoring background sync operations.

#![allow(missing_docs)]

use std::collections::HashMap;
use tracing::{debug, error, info, Level};

/// Background sync event type
#[derive(Debug, Clone)]
pub enum BackgroundSyncEvent {
    /// Sync started
    Started {
        mode: String,
        wallet_id: String,
        timestamp: String,
    },
    /// Sync progress update
    Progress {
        current_height: u64,
        target_height: u64,
        percent: f64,
    },
    /// Sync completed
    Completed {
        blocks_synced: u64,
        duration_secs: u64,
        new_transactions: u32,
    },
    /// Sync failed
    Failed { error: String, retry_count: u32 },
    /// Tunnel verification
    TunnelVerified {
        tunnel_type: String,
        is_privacy_preserving: bool,
    },
    /// Notification shown
    NotificationShown {
        notification_type: String,
        transaction_count: u32,
    },
}

/// Background sync logger
pub struct BackgroundSyncLogger {
    events: Vec<BackgroundSyncEvent>,
}

impl BackgroundSyncLogger {
    /// Create new logger
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Log sync started
    pub fn log_started(&mut self, mode: &str, wallet_id: &str) {
        let event = BackgroundSyncEvent::Started {
            mode: mode.to_string(),
            wallet_id: wallet_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        info!(
            event = "background_sync_started",
            mode = %mode,
            wallet_id = %wallet_id,
            timestamp = %chrono::Utc::now().to_rfc3339(),
            "Background sync started"
        );

        self.events.push(event);
    }

    /// Log sync progress
    pub fn log_progress(&mut self, current: u64, target: u64, percent: f64) {
        let event = BackgroundSyncEvent::Progress {
            current_height: current,
            target_height: target,
            percent,
        };

        debug!(
            event = "background_sync_progress",
            current_height = %current,
            target_height = %target,
            percent = %percent,
            "Background sync progress"
        );

        self.events.push(event);
    }

    /// Log sync completed
    pub fn log_completed(&mut self, blocks: u64, duration: u64, new_txs: u32) {
        let event = BackgroundSyncEvent::Completed {
            blocks_synced: blocks,
            duration_secs: duration,
            new_transactions: new_txs,
        };

        info!(
            event = "background_sync_completed",
            blocks_synced = %blocks,
            duration_secs = %duration,
            new_transactions = %new_txs,
            "Background sync completed successfully"
        );

        self.events.push(event);
    }

    /// Log sync failed
    pub fn log_failed(&mut self, error_msg: &str, retry_count: u32) {
        let event = BackgroundSyncEvent::Failed {
            error: error_msg.to_string(),
            retry_count,
        };

        error!(
            event = "background_sync_failed",
            error = %error_msg,
            retry_count = %retry_count,
            "Background sync failed"
        );

        self.events.push(event);
    }

    /// Log tunnel verification
    pub fn log_tunnel_verified(&mut self, tunnel_type: &str, is_privacy: bool) {
        let event = BackgroundSyncEvent::TunnelVerified {
            tunnel_type: tunnel_type.to_string(),
            is_privacy_preserving: is_privacy,
        };

        info!(
            event = "background_sync_tunnel_verified",
            tunnel_type = %tunnel_type,
            is_privacy_preserving = %is_privacy,
            "Network tunnel verified for background sync"
        );

        self.events.push(event);
    }

    /// Log notification shown
    pub fn log_notification(&mut self, notification_type: &str, tx_count: u32) {
        let event = BackgroundSyncEvent::NotificationShown {
            notification_type: notification_type.to_string(),
            transaction_count: tx_count,
        };

        info!(
            event = "background_sync_notification",
            notification_type = %notification_type,
            transaction_count = %tx_count,
            "Background sync notification shown"
        );

        self.events.push(event);
    }

    /// Get all events
    pub fn get_events(&self) -> &[BackgroundSyncEvent] {
        &self.events
    }

    /// Get events as JSON for export
    pub fn export_events(&self) -> HashMap<String, Vec<String>> {
        let mut export = HashMap::new();
        let mut events = Vec::new();

        for event in &self.events {
            events.push(format!("{:?}", event));
        }

        export.insert("events".to_string(), events);
        export
    }

    /// Clear all events
    pub fn clear(&mut self) {
        self.events.clear();
        debug!("Background sync log cleared");
    }
}

impl Default for BackgroundSyncLogger {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize logging for background sync
pub fn init_background_sync_logging() {
    // Configure structured logging
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .json()
        .init();

    info!("Background sync logging initialized");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_sync_logger() {
        let mut logger = BackgroundSyncLogger::new();

        logger.log_started("compact", "wallet-123");
        logger.log_progress(1000, 2000, 50.0);
        logger.log_completed(1000, 30, 5);

        assert_eq!(logger.get_events().len(), 3);
    }

    #[test]
    fn test_logger_export() {
        let mut logger = BackgroundSyncLogger::new();
        logger.log_started("deep", "wallet-456");

        let export = logger.export_events();
        assert!(export.contains_key("events"));
        assert_eq!(export.get("events").unwrap().len(), 1);
    }

    #[test]
    fn test_logger_clear() {
        let mut logger = BackgroundSyncLogger::new();
        logger.log_started("compact", "wallet-789");
        logger.clear();

        assert_eq!(logger.get_events().len(), 0);
    }
}
