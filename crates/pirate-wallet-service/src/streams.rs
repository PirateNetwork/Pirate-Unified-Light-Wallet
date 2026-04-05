//! Stream APIs for Flutter
//!
//! Real-time streaming data from sync engine.

use crate::api::{is_sync_running, sync_status};
use crate::models::{Balance, SyncStage, SyncStatus, TxInfo};
use std::time::Duration;
use tokio::sync::mpsc;

/// Sync progress stream - emits real sync status updates
///
/// This stream polls the sync engine and emits status updates every second.
/// When sync is complete or not running, updates are less frequent.
pub async fn sync_progress_stream(wallet_id: String) -> mpsc::Receiver<SyncStatus> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        let mut last_height = 0u64;
        let mut idle_count = 0;

        loop {
            // Get real sync status from the engine
            let status = match sync_status(wallet_id.clone()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("Failed to get sync status: {:?}", e);
                    // Return a default idle status on error
                    SyncStatus {
                        local_height: last_height,
                        target_height: last_height,
                        percent: 100.0,
                        eta: None,
                        stage: SyncStage::Verify,
                        last_checkpoint: None,
                        blocks_per_second: 0.0,
                        notes_decrypted: 0,
                        last_batch_ms: 0,
                    }
                }
            };

            // Check if syncing is active
            let is_syncing = status.local_height < status.target_height && status.target_height > 0;

            // Send status update
            if tx.send(status.clone()).await.is_err() {
                // Channel closed, receiver dropped
                break;
            }

            // Track height changes
            if status.local_height != last_height {
                last_height = status.local_height;
                idle_count = 0;
            } else {
                idle_count += 1;
            }

            // Adjust polling interval based on sync activity
            let interval = if is_syncing {
                Duration::from_millis(500) // Fast updates during sync
            } else if idle_count < 10 {
                Duration::from_secs(1) // Normal updates after sync
            } else {
                Duration::from_secs(5) // Slow updates when idle
            };

            tokio::time::sleep(interval).await;
        }

        tracing::debug!("Sync progress stream ended for wallet {}", wallet_id);
    });

    rx
}

/// Transaction stream (new transactions)
///
/// Emits new transactions as they are discovered during sync.
/// Transaction discovery stream
///
/// Emits TxInfo when the sync engine discovers transactions belonging
/// to this wallet (incoming or outgoing).
pub async fn transaction_stream(wallet_id: String) -> mpsc::Receiver<TxInfo> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        tracing::info!("Transaction stream started for wallet {}", wallet_id);

        // Track the last transaction we've seen to detect new ones
        let mut last_seen_txids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut last_check_time = std::time::Instant::now();

        loop {
            // Poll for new transactions every 2 seconds during sync, 5 seconds otherwise
            let is_syncing = crate::api::is_sync_running(wallet_id.clone()).unwrap_or(false);

            let poll_interval = if is_syncing {
                Duration::from_secs(2)
            } else {
                Duration::from_secs(5)
            };

            tokio::time::sleep(poll_interval).await;

            // Check if channel is still open
            if tx.is_closed() {
                break;
            }

            // Get list of recent transactions from database
            match crate::api::list_transactions(wallet_id.clone(), Some(100)) {
                Ok(transactions) => {
                    // Find new transactions (ones we haven't seen before)
                    for tx_info in &transactions {
                        if !last_seen_txids.contains(&tx_info.txid) {
                            // New transaction found!
                            let txid = tx_info.txid.clone();
                            last_seen_txids.insert(txid.clone());

                            // Clone and send to stream
                            if tx.send(tx_info.clone()).await.is_err() {
                                // Channel closed, receiver dropped
                                tracing::debug!(
                                    "Transaction stream channel closed for wallet {}",
                                    wallet_id
                                );
                                return;
                            }

                            tracing::debug!(
                                "Emitted new transaction {} for wallet {}",
                                txid,
                                wallet_id
                            );
                        }
                    }

                    // Limit the size of last_seen_txids to prevent memory growth
                    // Keep only the most recent 1000 transaction IDs
                    if last_seen_txids.len() > 1000 {
                        // Get the most recent transaction IDs from the list
                        let recent_txids: std::collections::HashSet<String> = transactions
                            .iter()
                            .take(1000)
                            .map(|tx| tx.txid.clone())
                            .collect();
                        last_seen_txids = recent_txids;
                    }
                }
                Err(e) => {
                    // If we can't get transactions (e.g., wallet not loaded), log and continue
                    // Only log errors occasionally to avoid spam
                    if last_check_time.elapsed() > Duration::from_secs(30) {
                        tracing::debug!(
                            "Failed to get transactions for wallet {}: {}",
                            wallet_id,
                            e
                        );
                        last_check_time = std::time::Instant::now();
                    }
                }
            }
        }

        tracing::debug!("Transaction stream ended for wallet {}", wallet_id);
    });

    rx
}

/// Balance update stream
///
/// Emits balance updates when balance changes (after sync batches or transactions).
pub async fn balance_stream(wallet_id: String) -> mpsc::Receiver<Balance> {
    let (tx, rx) = mpsc::channel(100);

    tokio::spawn(async move {
        let mut last_balance = 0u64;

        loop {
            // Get current balance from storage
            let balance = match crate::api::get_balance(wallet_id.clone()) {
                Ok(b) => b,
                Err(_) => Balance {
                    total: 0,
                    spendable: 0,
                    pending: 0,
                },
            };

            // Only emit if balance changed
            if balance.total != last_balance {
                last_balance = balance.total;

                if tx.send(balance.clone()).await.is_err() {
                    break;
                }
            }

            // Poll every 2 seconds during sync, 10 seconds otherwise
            let is_syncing = is_sync_running(wallet_id.clone()).unwrap_or(false);

            let interval = if is_syncing {
                Duration::from_secs(2)
            } else {
                Duration::from_secs(10)
            };

            tokio::time::sleep(interval).await;
        }

        tracing::debug!("Balance stream ended for wallet {}", wallet_id);
    });

    rx
}

/// Get latest sync status snapshot (non-streaming)
pub fn get_sync_status_snapshot(wallet_id: &str) -> Option<SyncStatus> {
    sync_status(wallet_id.to_string()).ok()
}

/// Check if sync is currently active for a wallet
pub fn is_sync_active(wallet_id: &str) -> bool {
    is_sync_running(wallet_id.to_string()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sync_progress_stream_creation() {
        let rx = sync_progress_stream("test_wallet".to_string()).await;
        // Stream should be created
        assert!(!rx.is_closed());
    }
}
