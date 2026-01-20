//! Background sync orchestration for mobile platforms
//!
//! Provides battery-respectful background sync with network tunnel support.

use crate::{Result, SyncEngine};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn, error, debug};

const MIN_DEPTH: u64 = 10;

/// Background sync mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackgroundSyncMode {
    /// Quick compact block sync (for frequent updates)
    Compact,
    /// Deep sync with witness updates (for daily maintenance)
    Deep,
}

/// Background sync result
#[derive(Debug, Clone)]
pub struct BackgroundSyncResult {
    /// Sync mode that was executed
    pub mode: BackgroundSyncMode,
    /// Number of blocks synced
    pub blocks_synced: u64,
    /// Starting height
    pub start_height: u64,
    /// Ending height
    pub end_height: u64,
    /// Duration in seconds
    pub duration_secs: u64,
    /// Any errors encountered (non-fatal)
    pub errors: Vec<String>,
    /// New balance after sync (if changed)
    pub new_balance: Option<u64>,
    /// Number of new transactions
    pub new_transactions: u32,
}

/// Background sync configuration
#[derive(Debug, Clone)]
pub struct BackgroundSyncConfig {
    /// Maximum sync duration before yielding (seconds)
    pub max_duration_secs: u64,
    /// Maximum blocks to sync in one run (normal conditions)
    pub max_blocks: u64,
    /// Maximum blocks during spam periods (reduced to prevent timeouts)
    pub max_blocks_spam: u64,
    /// Compact sync interval (minutes)
    pub compact_interval_mins: u32,
    /// Deep sync interval (hours)
    pub deep_interval_hours: u32,
    /// Use foreground service for long operations
    pub use_foreground_service: bool,
    /// Notify on received funds
    pub notify_on_receive: bool,
}

impl Default for BackgroundSyncConfig {
    fn default() -> Self {
        Self {
            max_duration_secs: 60, // 1 minute max per background task
            max_blocks: 10_000, // Normal max blocks
            max_blocks_spam: 2_500, // Reduced max during spam (prevents timeouts)
            compact_interval_mins: 15,
            deep_interval_hours: 24,
            use_foreground_service: true,
            notify_on_receive: true,
        }
    }
}

/// Background sync orchestrator
pub struct BackgroundSyncOrchestrator {
    sync_engine: Arc<Mutex<SyncEngine>>,
    config: BackgroundSyncConfig,
}

#[allow(dead_code)]
fn _assert_background_sync_orchestrator_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BackgroundSyncOrchestrator>();
}

impl BackgroundSyncOrchestrator {
    /// Create new background sync orchestrator
    pub fn new(sync_engine: Arc<Mutex<SyncEngine>>, config: BackgroundSyncConfig) -> Self {
        Self {
            sync_engine,
            config,
        }
    }

    /// Execute background sync
    pub async fn execute_sync(&self, mode: BackgroundSyncMode) -> Result<BackgroundSyncResult> {
        let start_time = std::time::Instant::now();
        
        info!(
            "Starting background sync: mode={:?}, max_duration={}s, max_blocks={}",
            mode, self.config.max_duration_secs, self.config.max_blocks
        );

        // Get current state
        let start_height = {
            let engine = self.sync_engine.clone().lock_owned().await;
            let progress = engine.progress();
            let progress_guard = progress.read().await;
            progress_guard.current_height()
        };

        // Determine sync range
        let target_height = self.calculate_target_height(start_height, mode).await?;
        
        if target_height <= start_height {
            info!("Already synced to latest height: {}", start_height);
            return Ok(BackgroundSyncResult {
                mode,
                blocks_synced: 0,
                start_height,
                end_height: start_height,
                duration_secs: 0,
                errors: vec![],
                new_balance: None,
                new_transactions: 0,
            });
        }

        // Execute sync with timeout
        let blocks_to_sync = target_height - start_height;
        debug!("Syncing blocks {} to {} ({} blocks)", start_height, target_height, blocks_to_sync);

        let sync_result = match mode {
            BackgroundSyncMode::Compact => {
                self.execute_compact_sync(start_height, target_height).await
            }
            BackgroundSyncMode::Deep => {
                self.execute_deep_sync(start_height, target_height).await
            }
        };

        let duration_secs = start_time.elapsed().as_secs();

        match sync_result {
            Ok((end_height, new_txs)) => {
                info!(
                    "Background sync completed: mode={:?}, blocks_synced={}, duration={}s, new_txs={}",
                    mode,
                    end_height - start_height,
                    duration_secs,
                    new_txs
                );

                Ok(BackgroundSyncResult {
                    mode,
                    blocks_synced: end_height - start_height,
                    start_height,
                    end_height,
                    duration_secs,
                    errors: vec![],
                    new_balance: {
                        let engine = self.sync_engine.clone().lock_owned().await;
                        engine.total_balance_at_height(end_height, MIN_DEPTH)?
                    },
                    new_transactions: new_txs,
                })
            }
            Err(e) => {
                error!("Background sync failed: {:?}", e);
                
                // Return partial result with error
                Ok(BackgroundSyncResult {
                    mode,
                    blocks_synced: 0,
                    start_height,
                    end_height: start_height,
                    duration_secs,
                    errors: vec![e.to_string()],
                    new_balance: None,
                    new_transactions: 0,
                })
            }
        }
    }

    /// Execute compact sync (quick, frequent)
    async fn execute_compact_sync(&self, start: u64, target: u64) -> Result<(u64, u32)> {
        let mut engine = self.sync_engine.clone().lock_owned().await;
        
        // Check if we're in a spam period by checking recent sync performance
        // If recent batches were heavy, reduce max_blocks to prevent timeouts
        let max_blocks = {
            let perf = engine.perf_counters();
            let perf_snap = perf.snapshot();
            // If average batch time is very high (>5s per batch), likely spam period
            // Reduce max_blocks to prevent timeout
            if perf_snap.avg_batch_ms > 5000 && perf_snap.blocks_processed > 0 {
                debug!("Spam period detected (avg batch {}ms), reducing max_blocks from {} to {}", 
                    perf_snap.avg_batch_ms, 
                    self.config.max_blocks,
                    self.config.max_blocks_spam
                );
                self.config.max_blocks_spam
            } else {
                self.config.max_blocks
            }
        };
        
        // Limit to max blocks (spam-aware)
        let effective_target = std::cmp::min(target, start + max_blocks);
        
        // Sync with timeout
        let timeout = tokio::time::Duration::from_secs(self.config.max_duration_secs);
        
        match tokio::time::timeout(timeout, engine.sync_range(start, Some(effective_target))).await {
            Ok(result) => {
                result?;
                
                let new_txs = engine
                    .count_transactions_since_height(start, effective_target)?
                    .unwrap_or(0);
                Ok((effective_target, new_txs))
            }
            Err(_) => {
                warn!("Compact sync timed out after {}s", self.config.max_duration_secs);
                
                // Return partial progress
                let progress = engine.progress();
                let progress_guard = progress.read().await;
                Ok((progress_guard.current_height(), 0))
            }
        }
    }

    /// Execute deep sync (thorough, less frequent)
    async fn execute_deep_sync(&self, start: u64, target: u64) -> Result<(u64, u32)> {
        let mut engine = self.sync_engine.clone().lock_owned().await;
        
        // Check if we're in a spam period (same logic as compact sync)
        let max_blocks = {
            let perf = engine.perf_counters();
            let perf_snap = perf.snapshot();
            // If average batch time is very high (>5s per batch), likely spam period
            if perf_snap.avg_batch_ms > 5000 && perf_snap.blocks_processed > 0 {
                debug!("Spam period detected (avg batch {}ms), reducing max_blocks from {} to {}", 
                    perf_snap.avg_batch_ms, 
                    self.config.max_blocks,
                    self.config.max_blocks_spam
                );
                self.config.max_blocks_spam
            } else {
                self.config.max_blocks
            }
        };
        
        // Limit to max blocks (spam-aware) even for deep sync
        let effective_target = std::cmp::min(target, start + max_blocks);
        
        // Deep sync can take longer, but still respect max duration
        let timeout = tokio::time::Duration::from_secs(self.config.max_duration_secs * 2);
        
        match tokio::time::timeout(timeout, engine.sync_range(start, Some(effective_target))).await {
            Ok(result) => {
                result?;
                
                let new_txs = engine
                    .count_transactions_since_height(start, effective_target)?
                    .unwrap_or(0);
                Ok((effective_target, new_txs))
            }
            Err(_) => {
                warn!("Deep sync timed out after {}s", timeout.as_secs());
                
                let progress = engine.progress();
                let progress_guard = progress.read().await;
                Ok((progress_guard.current_height(), 0))
            }
        }
    }

    /// Calculate target height for sync
    async fn calculate_target_height(&self, _current: u64, _mode: BackgroundSyncMode) -> Result<u64> {
        let engine = self.sync_engine.clone().lock_owned().await;
        let progress = engine.progress();
        let progress_guard = progress.read().await;
        
        // For both modes, sync to network height
        Ok(progress_guard.target_height())
    }

    /// Check if background sync is needed
    pub async fn is_sync_needed(&self) -> Result<bool> {
        let engine = self.sync_engine.clone().lock_owned().await;
        let progress = engine.progress();
        let progress_guard = progress.read().await;
        
        // Sync needed if we're behind
        Ok(progress_guard.current_height() < progress_guard.target_height())
    }

    /// Get recommended sync mode based on time since last sync
    pub fn recommend_sync_mode(&self, minutes_since_last: u32) -> BackgroundSyncMode {
        if minutes_since_last >= (self.config.deep_interval_hours * 60) {
            BackgroundSyncMode::Deep
        } else {
            BackgroundSyncMode::Compact
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_sync_mode() {
        assert_eq!(BackgroundSyncMode::Compact, BackgroundSyncMode::Compact);
        assert_ne!(BackgroundSyncMode::Compact, BackgroundSyncMode::Deep);
    }

    #[test]
    fn test_background_sync_config_defaults() {
        let config = BackgroundSyncConfig::default();
        assert_eq!(config.max_duration_secs, 60);
        assert_eq!(config.max_blocks, 10_000);
        assert_eq!(config.compact_interval_mins, 15);
        assert_eq!(config.deep_interval_hours, 24);
    }

    #[test]
    fn test_recommend_sync_mode() {
        let config = BackgroundSyncConfig::default();
        let engine = Arc::new(Mutex::new(SyncEngine::new("http://test".to_string(), 0)));
        
        let orchestrator = BackgroundSyncOrchestrator::new(engine, config);
        
        // Recent sync -> Compact
        assert_eq!(
            orchestrator.recommend_sync_mode(10),
            BackgroundSyncMode::Compact
        );
        
        // Old sync -> Deep
        assert_eq!(
            orchestrator.recommend_sync_mode(24 * 60),
            BackgroundSyncMode::Deep
        );
    }
}
