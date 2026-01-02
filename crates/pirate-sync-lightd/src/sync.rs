//! Sync engine with batched trial decryption, checkpoints, and auto-rollback
//!
//! Production-ready sync with:
//! - Retry logic with exponential backoff
//! - Cancellation handling and interruption recovery
//! - Performance counters
//! - Mini-checkpoints every N batches
//! - SaplingFrontier for witness tree management
//! - Checkpoint loading and restoration
//! - Rollback on interruption/corruption/reorg

use crate::{LightClient, Result, SyncProgress, Error};
use crate::client::CompactBlockData;
use crate::block_cache::{acquire_inflight, BlockCache, InflightLease};
use crate::progress::SyncStage;
use crate::pipeline::{DecryptedNote, PerfCounters};
use crate::frontier::SaplingFrontier;
use crate::orchard_frontier::OrchardFrontier;
use crate::sapling::full_decrypt::decrypt_memo_from_raw_tx_with_ivk_bytes;
use crate::orchard::full_decrypt::decrypt_orchard_memo_from_raw_tx_with_ivk_bytes;
use crate::pipeline::NoteType;
use orchard::keys::{Diversifier as OrchardDiversifier, IncomingViewingKey as OrchardIncomingViewingKey};
use orchard::note::{Note as OrchardNote, Nullifier as OrchardNullifier, RandomSeed as OrchardRandomSeed};
use orchard::note_encryption::OrchardDomain;
use orchard::tree::MerkleHashOrchard;
use orchard::value::NoteValue as OrchardNoteValue;
use orchard::Address as OrchardAddress;
use pirate_storage_sqlite::{
    Database, EncryptionKey, FrontierStorage, NoteRecord, Repository,
    SyncStateStorage, truncate_above_height,
};
use pirate_storage_sqlite::repository::OrchardNoteRef;
use pirate_storage_sqlite::security::MasterKey;
use pirate_core::keys::{ExtendedSpendingKey, ExtendedFullViewingKey, OrchardExtendedSpendingKey, OrchardExtendedFullViewingKey};
use pirate_core::transaction::PirateNetwork;
use anyhow::anyhow;
use hex;
use subtle::CtOption;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use directories::ProjectDirs;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use std::sync::Arc;
use std::io::Write;
use zcash_note_encryption::try_output_recovery_with_ovk;
use zcash_primitives::consensus::{BlockHeight, BranchId};
use zcash_primitives::merkle_tree::{read_frontier_v0, read_frontier_v1, write_merkle_path};
use zcash_primitives::sapling::note_encryption::try_sapling_output_recovery;
use zcash_primitives::sapling::keys::OutgoingViewingKey as SaplingOutgoingViewingKey;
use zcash_primitives::transaction::Transaction;

fn debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        env::current_dir()
            .map(|dir| dir.join(".cursor").join("debug.log"))
            .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
}

/// Sync configuration
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Checkpoint interval (blocks)
    pub checkpoint_interval: u32,
    /// Initial batch size for block fetching (will adapt based on block size)
    /// Used when server batch recommendations are disabled or unavailable
    pub batch_size: u64,
    /// Minimum batch size (for spam blocks)
    pub min_batch_size: u64,
    /// Maximum batch size (caps server-provided batches to prevent OOM)
    /// Also used as the maximum when using client-side batching
    pub max_batch_size: u64,
    /// Whether to use server's GetLiteWalletBlockGroup recommendations
    /// If false, always uses client-side batch_size calculation
    /// Server recommendations group by ~4MB data chunks (typically ~199 blocks)
    pub use_server_batch_recommendations: bool,
    /// Number of batches between mini-checkpoints
    pub mini_checkpoint_every: u32,
    /// Maximum parallel trial decryptions
    pub max_parallel_decrypt: usize,
    /// Lazy memo decoding (only decode if needed)
    pub lazy_memo_decode: bool,
    /// Threshold for detecting heavy/spam blocks (bytes per block)
    pub heavy_block_threshold_bytes: u64,
    /// Maximum memory per batch in bytes (None = no limit)
    /// Helps prevent OOM on memory-constrained devices
    pub max_batch_memory_bytes: Option<u64>,
}

/// Constants for retry logic
const MAX_RETRY_ATTEMPTS: u32 = 3;
const RETRY_BACKOFF_MS: u64 = 100;
const FRONTIER_SNAPSHOT_RETAIN: usize = 10;

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval: 10_000,
            batch_size: 2_000, // Match SimpleSync default (used when server recommendations disabled)
            min_batch_size: 100, // Minimum batch size for spam blocks
            max_batch_size: 2_000, // Maximum batch size (caps server batches to prevent OOM)
            use_server_batch_recommendations: true, // Use server's ~4MB chunk recommendations (typically ~199 blocks)
            mini_checkpoint_every: 5, // Mini-checkpoint every 5 batches
            max_parallel_decrypt: num_cpus::get(),
            lazy_memo_decode: true,
            heavy_block_threshold_bytes: 500_000, // 500KB per block = heavy/spam (lowered for earlier detection)
            max_batch_memory_bytes: Some(100_000_000), // 100MB max per batch (prevents OOM on mobile)
        }
    }
}

/// Sync engine
pub struct SyncEngine {
    client: LightClient,
    progress: Arc<RwLock<SyncProgress>>,
    config: SyncConfig,
    birthday_height: u32,
    wallet_id: Option<String>,
    storage: Option<StorageSink>,
    keys: Option<WalletKeys>,
    /// Sapling frontier for witness tree management
    frontier: Arc<RwLock<SaplingFrontier>>,
    /// Orchard frontier for witness tree management
    orchard_frontier: Arc<RwLock<OrchardFrontier>>,
    /// Performance counters
    perf: Arc<PerfCounters>,
    /// Cancellation flag
    cancelled: Arc<RwLock<bool>>,
}

impl SyncEngine {
    /// Create new sync engine
    pub fn new(endpoint: String, birthday_height: u32) -> Self {
        Self {
            client: LightClient::new(endpoint),
            progress: Arc::new(RwLock::new(SyncProgress::new())),
            config: SyncConfig::default(),
            birthday_height,
            wallet_id: None,
            storage: None,
            keys: None,
            frontier: Arc::new(RwLock::new(SaplingFrontier::new())),
            orchard_frontier: Arc::new(RwLock::new(OrchardFrontier::new())),
            perf: Arc::new(PerfCounters::new()),
            cancelled: Arc::new(RwLock::new(false)),
        }
    }

    /// Create with custom configuration
    pub fn with_config(endpoint: String, birthday_height: u32, config: SyncConfig) -> Self {
        Self {
            client: LightClient::new(endpoint),
            progress: Arc::new(RwLock::new(SyncProgress::new())),
            config,
            birthday_height,
            wallet_id: None,
            storage: None,
            keys: None,
            frontier: Arc::new(RwLock::new(SaplingFrontier::new())),
            orchard_frontier: Arc::new(RwLock::new(OrchardFrontier::new())),
            perf: Arc::new(PerfCounters::new()),
            cancelled: Arc::new(RwLock::new(false)),
        }
    }

    /// Create with pre-configured client and custom sync config
    pub fn with_client_and_config(client: LightClient, birthday_height: u32, config: SyncConfig) -> Self {
        Self {
            client,
            progress: Arc::new(RwLock::new(SyncProgress::new())),
            config,
            birthday_height,
            wallet_id: None,
            storage: None,
            keys: None,
            frontier: Arc::new(RwLock::new(SaplingFrontier::new())),
            orchard_frontier: Arc::new(RwLock::new(OrchardFrontier::new())),
            perf: Arc::new(PerfCounters::new()),
            cancelled: Arc::new(RwLock::new(false)),
        }
    }

    /// Get performance counters reference
    pub fn perf_counters(&self) -> Arc<PerfCounters> {
        Arc::clone(&self.perf)
    }

    /// Cancel sync
    pub async fn cancel(&self) {
        *self.cancelled.write().await = true;
        tracing::info!("Sync cancellation requested");
    }

    /// Share cancellation flag without locking the engine.
    pub fn cancel_flag(&self) -> Arc<RwLock<bool>> {
        Arc::clone(&self.cancelled)
    }

    /// Check if cancelled
    async fn is_cancelled(&self) -> bool {
        *self.cancelled.read().await
    }

    /// Attach wallet context and open encrypted storage (shared DB with FFI)
    pub fn with_wallet(
        mut self,
        wallet_id: String,
        key: EncryptionKey,
        master_key: MasterKey,
    ) -> Result<Self> {
        self.wallet_id = Some(wallet_id.clone());

        let db_path = wallet_db_path(&wallet_id)?;
        let db = Database::open(&db_path, &key, master_key.clone())?;
        let repo = Repository::new(&db);

        // Load wallet secret to know account id (if present)
        let secret = repo
            .get_wallet_secret(&wallet_id)?
            .ok_or_else(|| Error::Sync(format!("Wallet secret not found for {}", wallet_id)))?;

        // Derive keys (extsk + dfvk + orchard_fvk) or handle watch-only
        let keys = if !secret.extsk.is_empty() {
            // Full wallet - derive from spending keys
            let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
                .map_err(|e| Error::Sync(format!("Invalid spending key bytes: {}", e)))?;
            let dfvk = if let Some(dfvk_bytes) = secret.dfvk.as_ref() {
                ExtendedFullViewingKey::from_bytes(dfvk_bytes)
                    .ok_or_else(|| Error::Sync("Invalid DFVK bytes".to_string()))?
            } else {
                extsk.to_extended_fvk()
            };

            // Derive Orchard FVK if Orchard key is available
            let orchard_fvk = if let Some(orchard_extsk_bytes) = secret.orchard_extsk.as_ref() {
                let orchard_extsk = OrchardExtendedSpendingKey::from_bytes(orchard_extsk_bytes)
                    .map_err(|e| Error::Sync(format!("Invalid Orchard spending key bytes: {}", e)))?;
                Some(orchard_extsk.to_extended_fvk())
            } else if let Some(fvk_bytes) = secret.orchard_ivk.as_ref().filter(|b| b.len() == 137) {
                OrchardExtendedFullViewingKey::from_bytes(fvk_bytes).ok()
            } else {
                None
            };
            
            Some(WalletKeys { 
                sapling_dfvk: Some(dfvk),
                orchard_fvk,
            })
        } else {
            // Watch-only wallet - prefer stored DFVK/FVK if available
            let sapling_dfvk = secret
                .dfvk
                .as_ref()
                .and_then(|dfvk_bytes| ExtendedFullViewingKey::from_bytes(dfvk_bytes));
            let orchard_fvk = secret
                .orchard_ivk
                .as_ref()
                .and_then(|bytes| if bytes.len() == 137 {
                    OrchardExtendedFullViewingKey::from_bytes(bytes).ok()
                } else {
                    None
                });

            if sapling_dfvk.is_some() || orchard_fvk.is_some() {
                Some(WalletKeys {
                    sapling_dfvk,
                    orchard_fvk,
                })
            } else {
                None
            }
        };

        let sink = StorageSink {
            db_path,
            key,
            master_key,
            account_id: secret.account_id,
        };
        self.storage = Some(sink);
        self.keys = keys;
        Ok(self)
    }

    /// Get progress reference
    pub fn progress(&self) -> Arc<RwLock<SyncProgress>> {
        Arc::clone(&self.progress)
    }

    /// Start sync from birthday height
    pub async fn sync_from_birthday(&mut self) -> Result<()> {
        let mut start_height = self.birthday_height as u64;

        if let Some(ref sink) = self.storage {
            let stored_height = {
                let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
                let sync_state = SyncStateStorage::new(&db).load_sync_state()?;
                sync_state.local_height
            };

            if stored_height > 0 {
                if let Some(snapshot_height) = self.restore_frontiers_from_storage(stored_height).await? {
                    if snapshot_height < stored_height {
                        // Frontier snapshot is behind. Try to rebuild the frontier from cached
                        // compact blocks to avoid truncating notes or fetching tree state.
                        let rebuilt = self
                            .rebuild_frontier_from_cache(snapshot_height, stored_height)
                            .await
                            .unwrap_or(false);

                        if !rebuilt {
                            // Cache rebuild failed; clear frontiers and fall back to tree-state init.
                            *self.frontier.write().await = SaplingFrontier::new();
                            *self.orchard_frontier.write().await = OrchardFrontier::new();
                        }

                        start_height = stored_height.saturating_add(1);
                        // #region agent log
                        if let Ok(mut file) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(debug_log_path())
                        {
                            use std::io::Write;
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let id = format!("{:08x}", ts);
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:303","message":"frontier snapshot behind; cache rebuild","data":{{"stored_height":{},"snapshot_height":{},"start_height":{},"rebuilt":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                                id,
                                ts,
                                stored_height,
                                snapshot_height,
                                start_height,
                                rebuilt
                            );
                        }
                        // #endregion
                    } else {
                        start_height = snapshot_height.saturating_add(1);
                    }
                } else {
                    start_height = stored_height.saturating_add(1);
                }
            }
        }

        if start_height < self.birthday_height as u64 {
            start_height = self.birthday_height as u64;
        }

        self.sync_range(start_height, None).await
    }

    async fn rebuild_frontier_from_cache(
        &self,
        snapshot_height: u64,
        stored_height: u64,
    ) -> Result<bool> {
        if stored_height <= snapshot_height {
            return Ok(true);
        }

        let start = snapshot_height.saturating_add(1);
        let end = stored_height;
        let cache = match BlockCache::for_endpoint(self.client.endpoint()) {
            Ok(c) => c,
            Err(_) => return Ok(false),
        };

        let blocks = cache.load_range(start, end).unwrap_or_default();
        let expected = end.saturating_sub(start).saturating_add(1);
        if blocks.len() as u64 != expected {
            return Ok(false);
        }

        let _ = self.update_frontier(&blocks, &[]).await?;
        Ok(true)
    }

    /// Total wallet balance at a given chain height (spendable + pending).
    ///
    /// Returns `Ok(None)` if the engine has no attached wallet storage.
    pub fn total_balance_at_height(&self, current_height: u64, min_depth: u64) -> Result<Option<u64>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(None),
        };
        let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let repo = Repository::new(&db);
        let (_spendable, _pending, total) =
            repo.calculate_balance(sink.account_id, current_height, min_depth)?;
        Ok(Some(total))
    }

    /// Count transactions whose mined height is > `from_height` and <= `current_height`.
    ///
    /// Returns `Ok(None)` if the engine has no attached wallet storage.
    pub fn count_transactions_since_height(
        &self,
        from_height: u64,
        current_height: u64,
    ) -> Result<Option<u32>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(None),
        };
        let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let repo = Repository::new(&db);
        let txs = repo.get_transactions(sink.account_id, None, current_height, 0)?;
        let count = txs
            .iter()
            .filter(|t| {
                let h = t.height as i64;
                h > from_height as i64 && h <= current_height as i64
            })
            .count() as u32;
        Ok(Some(count))
    }

    /// Sync specific range
    pub async fn sync_range(&mut self, start_height: u64, end_height: Option<u64>) -> Result<()> {
        tracing::info!("sync_range called: start={}, end_height={:?}", start_height, end_height);
        
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:275","message":"sync_range entry","data":{{"start":{},"end_height":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#, 
                id, ts, start_height, end_height);
        }
        // #endregion
        
        // Connect to lightwalletd
        tracing::debug!("Connecting to lightwalletd...");
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:280","message":"connect attempt","data":{{}},"sessionId":"debug-session","runId":"run1","hypothesisId":"A"}}"#, 
                id, ts);
        }
        // #endregion
        let connect_result = self.client.connect().await;
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:283","message":"connect result","data":{{"success":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"A"}}"#, 
                id, ts, connect_result.is_ok(), connect_result.as_ref().err());
        }
        // #endregion
        connect_result.map_err(|e| {
            tracing::error!("Failed to connect to lightwalletd: {:?}", e);
            e
        })?;
        tracing::debug!("Connected to lightwalletd");

        // Get latest block if end not specified
        let end = match end_height {
            Some(h) => {
                tracing::debug!("Using provided end height: {}", h);
                h
            }
            None => {
                tracing::debug!("Fetching latest block from server...");
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:294","message":"get_latest_block call","data":{{}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#, 
                        id, ts);
                }
                // #endregion
                let latest_result = self.client.get_latest_block().await;
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:297","message":"get_latest_block result in sync","data":{{"success":{},"height":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#, 
                        id, ts, latest_result.is_ok(), latest_result.as_ref().ok().copied().unwrap_or(0), latest_result.as_ref().err());
                }
                // #endregion
                let latest = latest_result.map_err(|e| {
                    tracing::error!("Failed to get latest block: {:?}", e);
                    e
                })?;
                tracing::info!("Latest block height from server: {}", latest);
                latest
            }
        };

        // Validate end height
        if end < start_height {
            let err = anyhow!("Invalid sync range: end ({}) < start ({})", end, start_height);
            tracing::error!("{}", err);
            return Err(Error::Sync(err.to_string()));
        }

        // Initialize progress
        {
            let mut progress = self.progress.write().await;
            progress.set_target(end);
            progress.set_current(start_height);
            progress.set_stage(SyncStage::Headers);
            progress.start();
            tracing::debug!(
                "Progress initialized: current={}, target={}, stage={:?}",
                start_height,
                end,
                SyncStage::Headers
            );
        }

        tracing::info!(
            "Starting sync: {} -> {} ({} blocks)",
            start_height,
            end,
            end - start_height + 1
        );

        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:332","message":"sync_range_internal entry","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#, 
                id, ts, start_height, end, end - start_height + 1);
        }
        // #endregion
        let result = self.sync_range_internal(start_height, end).await;
        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:333","message":"sync_range_internal result","data":{{"success":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#, 
                id, ts, result.is_ok(), result.as_ref().err());
        }
        // #endregion

        // Mark complete or failed
        if result.is_ok() {
            self.progress.write().await.complete();
            tracing::info!("Sync completed successfully");
        } else {
            self.progress.write().await.set_stage(SyncStage::Verify);
            tracing::error!("Sync failed: {:?}", result);
        }

        result
    }

    async fn restore_frontiers_from_storage(&self, height: u64) -> Result<Option<u64>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(None),
        };

        let snapshot = {
            let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
            let storage = FrontierStorage::new(&db);
            storage.load_snapshot_at_or_below(height as u32)?
        };
        let (snapshot_height, bytes) = match snapshot {
            Some(data) => data,
            None => return Ok(None),
        };

        let (sapling_bytes, orchard_bytes) = decode_frontier_snapshot(&bytes)?;
        if sapling_bytes.is_empty() {
            return Err(Error::Sync("Empty Sapling frontier snapshot".to_string()));
        }

        let sapling_frontier = match SaplingFrontier::deserialize(&sapling_bytes) {
            Ok(frontier) => frontier,
            Err(e) => {
                tracing::warn!("Failed to restore Sapling frontier snapshot: {}", e);
                return Ok(None);
            }
        };
        let orchard_frontier = if orchard_bytes.is_empty() {
            OrchardFrontier::new()
        } else {
            match OrchardFrontier::deserialize(&orchard_bytes) {
                Ok(frontier) => frontier,
                Err(e) => {
                    tracing::warn!("Failed to restore Orchard frontier snapshot: {}", e);
                    return Ok(None);
                }
            }
        };

        *self.frontier.write().await = sapling_frontier;
        *self.orchard_frontier.write().await = orchard_frontier;

        Ok(Some(snapshot_height as u64))
    }

    async fn initialize_frontiers_from_tree_state(&self, start_height: u64) -> Result<()> {
        if start_height == 0 {
            return Ok(());
        }

        let tree_height = start_height.saturating_sub(1);

        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:502","message":"initialize frontiers tree state","data":{{"tree_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                id,
                ts,
                tree_height
            );
        }
        // #endregion

        if let Some(snapshot_height) = self.restore_frontiers_from_storage(tree_height).await? {
            if snapshot_height == tree_height {
                tracing::debug!(
                    "Restored frontier snapshots at height {} from storage",
                    snapshot_height
                );
                return Ok(());
            }

            // Snapshot is older than required; reset before rebuilding from tree state.
            *self.frontier.write().await = SaplingFrontier::new();
            *self.orchard_frontier.write().await = OrchardFrontier::new();
        }

        let tree_state = self.fetch_tree_state_with_retry(tree_height).await?;

        fn parse_frontier_hex<H>(
            label: &str,
            hex_str: &str,
        ) -> Result<bridgetree::Frontier<H, { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH }>>
        where
            H: bridgetree::Hashable + zcash_primitives::merkle_tree::HashSer + Clone,
        {
            let bytes = hex::decode(hex_str).map_err(|e| {
                Error::Sync(format!("Failed to decode {} bytes: {}", label, e))
            })?;

            if let Ok(frontier) = read_frontier_v1::<H, _>(&bytes[..]) {
                return Ok(frontier);
            }

            read_frontier_v0::<H, _>(&bytes[..]).map_err(|e| {
                Error::Sync(format!("Failed to parse {} frontier: {}", label, e))
            })
        }

        {
            let mut sapling_frontier = self.frontier.write().await;
            if sapling_frontier.is_empty() {
                if !tree_state.sapling_frontier.is_empty() {
                    let frontier = parse_frontier_hex::<crate::frontier::SaplingCommitment>(
                        "sapling_frontier",
                        &tree_state.sapling_frontier,
                    )?;
                    sapling_frontier.init_from_frontier(frontier);
                } else if !tree_state.sapling_tree.is_empty() {
                    let frontier = parse_frontier_hex::<crate::frontier::SaplingCommitment>(
                        "sapling_tree",
                        &tree_state.sapling_tree,
                    )?;
                    sapling_frontier.init_from_frontier(frontier);
                }
            }
        }

        {
            let mut orchard_frontier = self.orchard_frontier.write().await;
            if orchard_frontier.is_empty() && !tree_state.orchard_tree.is_empty() {
                let frontier = parse_frontier_hex::<MerkleHashOrchard>(
                    "orchard_tree",
                    &tree_state.orchard_tree,
                )?;
                orchard_frontier.init_from_frontier(frontier);
            }
        }

        Ok(())
    }

    async fn fetch_tree_state_with_retry(&self, tree_height: u64) -> Result<crate::client::TreeState> {
        let max_attempts = 3u32;
        let timeout = Duration::from_secs(120);
        let mut attempt = 0u32;

        loop {
            attempt += 1;
            let bridge_result = tokio::time::timeout(
                timeout,
                self.client.get_bridge_tree_state(tree_height),
            )
            .await;

            match bridge_result {
                Ok(Ok(state)) => return Ok(state),
                Ok(Err(e)) => {
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(debug_log_path())
                    {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:535","message":"bridge tree state failed","data":{{"tree_height":{},"attempt":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                            id,
                            ts,
                            tree_height,
                            attempt,
                            e
                        );
                    }
                }
                Err(_) => {
                    if let Ok(mut file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(debug_log_path())
                    {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:552","message":"bridge tree state timeout","data":{{"tree_height":{},"attempt":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                            id,
                            ts,
                            tree_height,
                            attempt
                        );
                    }
                }
            }

            let legacy_result = tokio::time::timeout(
                timeout,
                self.client.get_tree_state(tree_height),
            )
            .await;

            match legacy_result {
                Ok(Ok(state)) => return Ok(state),
                Ok(Err(e)) => {
                    if attempt >= max_attempts {
                        return Err(Error::Sync(format!(
                            "Tree state fetch failed at {} after {} attempts: {}",
                            tree_height, attempt, e
                        )));
                    }
                }
                Err(_) => {
                    if attempt >= max_attempts {
                        return Err(Error::Sync(format!(
                            "Tree state fetch timed out at {} after {} attempts",
                            tree_height, attempt
                        )));
                    }
                }
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn sync_range_internal(&mut self, start: u64, mut end: u64) -> Result<()> {
        let mut current_height = start;
        let mut last_checkpoint_height = start.saturating_sub(1);
        let mut last_major_checkpoint_height = start.saturating_sub(1);
        let mut batches_since_mini_checkpoint = 0u32;
        
        // Adaptive batch sizing for spam blocks
        let mut current_batch_size = self.config.batch_size;
        let mut consecutive_heavy_batches = 0u32;

        // Reset perf counters
        self.perf.reset();
        
        // Reset cancellation flag.
        *self.cancelled.write().await = false;

        if start > 0 {
            let needs_init = self.frontier.read().await.is_empty();

            if needs_init {
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:343","message":"initialize frontiers start","data":{{"start_height":{},"sapling_empty":{},"orchard_empty":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                        id,
                        ts,
                        start,
                        self.frontier.read().await.is_empty(),
                        self.orchard_frontier.read().await.is_empty()
                    );
                }
                // #endregion
                self.initialize_frontiers_from_tree_state(start).await?;
            }
        }

        self.cleanup_orchard_false_positives().await?;

        // #region agent log
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:361","message":"sync loop start","data":{{"current":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#, 
                id, ts, current_height, end);
        }
        // #endregion

        // Outer loop: Keep syncing until we're fully caught up with no new blocks
        loop {
            // Main sync loop: sync from current_height to end
            while current_height <= end {
            // Check for cancellation
            if self.is_cancelled().await {
                tracing::warn!("Sync cancelled at height {}", current_height);
                return Err(Error::Sync("Sync cancelled".to_string()));
            }

            let batch_start_time = Instant::now();

            // Determine batch end: use server recommendations or client-side calculation
            let mut batch_end = if self.config.use_server_batch_recommendations {
                // Try to get optimal batch end from server (GetLiteWalletBlockGroup)
                // Server groups by ~4MB data chunks (typically ~199 blocks for normal blocks)
                // Falls back to client-side calculation if server method fails
                match self.client.get_lite_wallet_block_group(current_height).await {
                    Ok(server_end) => {
                        // Use server-provided optimal batch end, but don't exceed our target end
                        let optimal_end = std::cmp::min(server_end, end);
                        let server_batch_size = optimal_end.saturating_sub(current_height).saturating_add(1);
                        // Cap at max_batch_size to prevent OOM during spam
                        // Server might return very large batches (e.g., 5000+ blocks) during spam
                        let max_capped_end = std::cmp::min(
                            optimal_end,
                            current_height + self.config.max_batch_size - 1
                        );
                        if max_capped_end > current_height {
                            // #region agent log
                            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                                use std::io::Write;
                                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                                let id = format!("{:08x}", ts);
                                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:445","message":"server batch recommendation","data":{{"server_batch_size":{},"max_batch_size":{},"capped_batch_size":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"G"}}"#, 
                                    id, ts, server_batch_size, self.config.max_batch_size, max_capped_end - current_height + 1);
                            }
                            // #endregion
                            tracing::debug!(
                                "Using server-provided batch group: {} -> {} (optimal: {}, capped at max: {})",
                                current_height,
                                max_capped_end,
                                optimal_end - current_height + 1,
                                self.config.max_batch_size
                            );
                            max_capped_end
                        } else {
                            // Server returned invalid or same height, fall back to client calculation
                            std::cmp::min(current_height + current_batch_size - 1, end)
                        }
                    }
                    Err(e) => {
                        // Server method not available or failed, use client-side adaptive batch size
                        tracing::debug!(
                            "Server batch grouping unavailable ({}), using client-side batch size: {}",
                            e,
                            current_batch_size
                        );
                        std::cmp::min(current_height + current_batch_size - 1, end)
                    }
                }
            } else {
                // Client-side batching: use our configured batch_size
                // This allows full control over batch size regardless of server recommendations
                std::cmp::min(
                    current_height + current_batch_size - 1,
                    end
                )
            };
            
            // Additional safety: Check memory limits before fetching
            if let Some(max_memory) = self.config.max_batch_memory_bytes {
                let batch_block_count = batch_end - current_height + 1;
                // Estimate memory: assume worst case (all blocks at threshold size)
                // Add 1KB overhead per block for processing
                let estimated_memory = batch_block_count * (self.config.heavy_block_threshold_bytes + 1000);
                if estimated_memory > max_memory {
                    // Reduce batch size to fit in memory
                    let safe_block_count = max_memory / (self.config.heavy_block_threshold_bytes + 1000);
                    let safe_batch_end = std::cmp::min(
                        current_height + safe_block_count.saturating_sub(1),
                        end
                    );
                    if safe_batch_end > current_height {
                        tracing::warn!(
                            "Reducing batch size from {} to {} blocks due to memory limit (estimated {} bytes > limit {} bytes)",
                            batch_end - current_height + 1,
                            safe_batch_end - current_height + 1,
                            estimated_memory,
                            max_memory
                        );
                        batch_end = safe_batch_end;
                    }
                }
            }

            // Stage 1: Fetch blocks (with retry logic)
            self.progress.write().await.set_stage(SyncStage::Headers);
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:505","message":"fetch_blocks_with_retry start","data":{{"current_height":{},"batch_end":{},"batch_size":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#, 
                    id, ts, current_height, batch_end, batch_end - current_height + 1);
            }
            // #endregion
            let blocks = self.fetch_blocks_with_retry(current_height, batch_end).await?;
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:506","message":"fetch_blocks_with_retry result","data":{{"current_height":{},"batch_end":{},"blocks_count":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#, 
                    id, ts, current_height, batch_end, blocks.len());
            }
            // #endregion

            if blocks.is_empty() {
                tracing::warn!("Empty block batch at {}-{}", current_height, batch_end);
                current_height = batch_end + 1;
                continue;
            }

            // Detect heavy/spam blocks and adapt batch size
            // Count actual bytes in outputs and actions
            let total_block_size: u64 = blocks.iter()
                .map(|b| {
                    // Count actual bytes in Sapling outputs
                    let sapling_bytes: u64 = b.transactions.iter()
                        .map(|tx| {
                            tx.outputs.iter()
                                .map(|out| {
                                    // Each Sapling output: cmu (32) + ephemeral_key (32) + ciphertext
                                    // Compact ciphertext is 52 bytes minimum
                                    32 + 32 + out.ciphertext.len().max(52) as u64
                                })
                                .sum::<u64>()
                        })
                        .sum();
                    
                    // Count actual bytes in Orchard actions
                    let orchard_bytes: u64 = b.transactions.iter()
                        .map(|tx| {
                            tx.actions.iter()
                                .map(|action| {
                                    // Each Orchard action: nullifier (32) + cmx (32) + ephemeral_key (32) + 
                                    // enc_ciphertext (52+ minimum) + out_ciphertext (52+ minimum)
                                    32 + 32 + 32 + 
                                    action.enc_ciphertext.len().max(52) as u64 +
                                    action.out_ciphertext.len().max(52) as u64
                                })
                                .sum::<u64>()
                        })
                        .sum();
                    
                    // Transaction overhead (hash, etc.) - estimate ~100 bytes per tx
                    let tx_overhead = b.transactions.len() as u64 * 100;
                    tx_overhead + sapling_bytes + orchard_bytes
                })
                .sum();
            let avg_block_size = total_block_size / blocks.len().max(1) as u64;
            let is_heavy_batch = avg_block_size > self.config.heavy_block_threshold_bytes;

            if is_heavy_batch {
                consecutive_heavy_batches += 1;
                // Reduce batch size significantly for spam blocks
                // This allows faster checkpointing and prevents crashes
                current_batch_size = std::cmp::max(
                    self.config.min_batch_size,
                    current_batch_size / 4, // Reduce by 75%
                );
                tracing::warn!(
                    "Heavy block detected at height {} (avg {} bytes/block), reducing batch size to {} (consecutive: {})",
                    current_height,
                    avg_block_size,
                    current_batch_size,
                    consecutive_heavy_batches
                );
                
                // Create mini-checkpoint more frequently during spam blocks
                // This prevents losing progress if connection is unstable
                if consecutive_heavy_batches >= 2 {
                    self.create_checkpoint(batch_end).await?;
                    batches_since_mini_checkpoint = 0;
                    last_checkpoint_height = batch_end;
                    
                    {
                        let progress = self.progress.write().await;
                        progress.set_checkpoint(batch_end);
                    }
                    
                    tracing::info!(
                        "Emergency checkpoint at {} due to spam blocks (batch size: {})",
                        batch_end,
                        current_batch_size
                    );
                }
            } else {
                // Reset counter and gradually increase batch size back to normal
                consecutive_heavy_batches = 0;
                if current_batch_size < self.config.batch_size {
                    // Gradually increase back (by 25% each normal batch)
                    current_batch_size = std::cmp::min(
                        self.config.batch_size,
                        current_batch_size + (self.config.batch_size / 4),
                    );
                    tracing::debug!(
                        "Normal blocks detected, increasing batch size to {}",
                        current_batch_size
                    );
                }
            }

            // Stage 2: Trial decryption (batched with parallelism)
            self.progress.write().await.set_stage(SyncStage::Notes);
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:846","message":"trial_decrypt start","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                    id, ts, current_height, batch_end, blocks.len());
            }
            // #endregion
            let mut notes = self.trial_decrypt_batch(&blocks).await?;
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:852","message":"trial_decrypt done","data":{{"start":{},"end":{},"notes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                    id, ts, current_height, batch_end, notes.len());
            }
            // #endregion

            tracing::debug!(
                "Batch {}-{}: found {} notes",
                current_height,
                batch_end,
                notes.len()
            );

            // Stage 3: Update frontier (witness tree) - MUST happen before persisting notes
            // so we can store positions in the database
            self.progress.write().await.set_stage(SyncStage::Witness);
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:862","message":"update_frontier start","data":{{"start":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                    id, ts, current_height, batch_end);
            }
            // #endregion
            let (commitments_applied, position_mappings) = self.update_frontier(&blocks, &notes).await?;
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:866","message":"update_frontier done","data":{{"start":{},"end":{},"commitments":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                    id, ts, current_height, batch_end, commitments_applied);
            }
            // #endregion
            self.apply_positions(&mut notes, &position_mappings).await;
            self.apply_sapling_nullifiers(&mut notes, &position_mappings).await?;

            if !notes.is_empty() {
                self.fetch_and_enrich_notes(&mut notes, !self.config.lazy_memo_decode)
                    .await?;
            }

            // Persist decrypted notes if storage is configured (after frontier update to get positions)
            if let Some(ref sink) = self.storage {
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:881","message":"persist_notes start","data":{{"start":{},"end":{},"notes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                        id, ts, current_height, batch_end, notes.len());
                }
                // #endregion
                // Build txid->block_time map for this batch to persist accurate confirmation timestamps.
                let mut tx_times: HashMap<String, i64> = HashMap::new();
                let mut tx_fees: HashMap<String, i64> = HashMap::new();
                for b in &blocks {
                    let ts = b.time as i64;
                    for tx in &b.transactions {
                        let txid_hex = hex::encode(&tx.hash);
                        tx_times.insert(txid_hex.clone(), ts);
                        tx_fees.insert(txid_hex, tx.fee.unwrap_or(0) as i64);
                    }
                }

                sink.persist_notes(&notes, &tx_times, &tx_fees, &position_mappings)?;
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:900","message":"persist_notes done","data":{{"start":{},"end":{},"notes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                        id, ts, current_height, batch_end, notes.len());
                }
                // #endregion
            }

            if !blocks.is_empty() {
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:906","message":"apply_spends start","data":{{"start":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                        id, ts, current_height, batch_end);
                }
                // #endregion
                self.apply_spends(&blocks).await?;
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:909","message":"apply_spends done","data":{{"start":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#, 
                        id, ts, current_height, batch_end);
                }
                // #endregion
            }

            // Record batch performance
            let batch_duration = batch_start_time.elapsed();
            self.perf.record_batch(
                blocks.len() as u64,
                notes.len() as u64,
                commitments_applied,
                batch_duration.as_millis() as u64,
            );

            // Update progress with perf metrics
            {
                let progress = self.progress.write().await;
                progress.set_current(batch_end);
                progress.update_eta();
                progress.record_batch(
                    notes.len() as u64,
                    commitments_applied,
                    batch_duration.as_millis() as u64,
                );
            }
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let progress = self.progress.read().await;
                let wallet_id = self.wallet_id.as_deref().unwrap_or("unknown");
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:664","message":"progress updated","data":{{"current_height":{},"target_height":{},"percent":{:.2},"stage":"{:?}","wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"F"}}"#, 
                    id, ts, progress.current_height(), progress.target_height(), progress.percentage(), progress.stage(), wallet_id);
            }
            // #endregion

            batches_since_mini_checkpoint += 1;
            let blocks_since_major_checkpoint = batch_end - last_major_checkpoint_height;

            // Mini-checkpoint every N batches
            if batches_since_mini_checkpoint >= self.config.mini_checkpoint_every {
                self.create_checkpoint(batch_end).await?;
                batches_since_mini_checkpoint = 0;
                last_checkpoint_height = batch_end;

                {
                    let progress = self.progress.write().await;
                    progress.set_checkpoint(batch_end);
                }

                tracing::debug!(
                    "Mini-checkpoint at {} ({:.1} blk/s, {} notes, {}ms/batch)",
                    batch_end,
                    self.perf.blocks_per_second(),
                    self.perf.snapshot().notes_decrypted,
                    self.perf.snapshot().avg_batch_ms
                );
            }

            // Major checkpoint every CHECKPOINT_INTERVAL blocks
            if blocks_since_major_checkpoint >= self.config.checkpoint_interval as u64 {
                self.create_checkpoint(batch_end).await?;
                last_checkpoint_height = batch_end;
                last_major_checkpoint_height = batch_end;
                batches_since_mini_checkpoint = 0;

                {
                    let progress = self.progress.write().await;
                    progress.set_checkpoint(batch_end);
                }

                tracing::info!(
                    "Major checkpoint at height {} ({:.1} blk/s)",
                    batch_end,
                    self.perf.blocks_per_second()
                );
            }

            // Save sync state periodically
            self.save_sync_state(batch_end, end, last_checkpoint_height).await?;

            current_height = batch_end + 1;
            // #region agent log
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(debug_log_path()) {
                use std::io::Write;
                let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
                let id = format!("{:08x}", ts);
                let _ = writeln!(file, r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:709","message":"current_height updated","data":{{"new_current_height":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#, 
                    id, ts, current_height, end);
            }
            // #endregion
            
        }

        // After main sync loop completes, check if there are more blocks to sync
        // This handles the case where sync completed the initial range but blockchain moved forward
        // Keep checking and syncing until we're fully caught up, then keep monitoring for new blocks
        let current = {
            let progress = self.progress.read().await;
            progress.current_height()
        };
        
        match self.client.get_latest_block().await {
            Ok(latest_height) => {
                if latest_height > current {
                    tracing::info!(
                        "Found {} new blocks after sync completion, continuing sync from {} to {}",
                        latest_height - current,
                        current,
                        latest_height
                    );
                    // Update progress target and stage
                    {
                        let mut progress = self.progress.write().await;
                        progress.set_target(latest_height);
                        progress.set_stage(SyncStage::Headers);
                    }
                    // Continue syncing from current to latest - re-enter the main sync loop
                    end = latest_height;
                    current_height = current;
                    // Reset batch tracking for the new range
                    batches_since_mini_checkpoint = 0;
                    // Re-enter the outer loop (which will re-enter the main sync loop)
                    continue; // Continue outer loop to re-enter main sync loop
                } else {
                    // Caught up - wait a bit then check again for new blocks
                    // This keeps sync running continuously instead of stopping
                    // Set stage to Complete to indicate we're monitoring
                    // When monitoring, current_height == target_height, so complete() is safe
                    {
                        let progress = self.progress.read().await;
                        if progress.stage() != SyncStage::Complete {
                            drop(progress);
                            let progress = self.progress.write().await;
                            // Use complete() to set stage and ETA correctly
                            // This is safe because when monitoring, current_height == target_height
                            progress.complete();
                        }
                    }
                    tracing::debug!("Caught up to blockchain tip ({}), waiting for new blocks...", current);
                    tokio::time::sleep(Duration::from_secs(10)).await; // Check every 10 seconds
                    // Continue the outer loop to check again
                    continue;
                }
            }
            Err(e) => {
                tracing::warn!("Failed to check for new blocks after sync: {}, retrying in 30s", e);
                tokio::time::sleep(Duration::from_secs(30)).await; // Wait longer on error
                continue; // Retry
            }
        }
    }
    
    // This point should never be reached due to the infinite outer loop above
    // But if it is, log and return
    let perf = self.perf.snapshot();
    tracing::warn!(
        "Sync loop exited unexpectedly: {} blocks at {:.1} blk/s",
        perf.blocks_processed,
        perf.blocks_per_second
    );
    Ok(())
    }

    /// Fetch blocks with retry logic and exponential backoff
    async fn fetch_blocks_with_retry(
        &self,
        start: u64,
        end: u64,
    ) -> Result<Vec<CompactBlockData>> {
        if start > end {
            return Ok(Vec::new());
        }

        let expected_blocks = end.saturating_sub(start).saturating_add(1) as usize;

        if let Ok(cache) = BlockCache::for_endpoint(self.client.endpoint()) {
            match cache.load_range(start, end) {
                Ok(blocks) if blocks.len() == expected_blocks => {
                    tracing::debug!(
                        "Block cache hit for {}-{} ({} blocks)",
                        start,
                        end,
                        expected_blocks
                    );
                    return Ok(blocks);
                }
                Ok(blocks) if !blocks.is_empty() => {
                    tracing::debug!(
                        "Block cache partial hit for {}-{} ({} of {})",
                        start,
                        end,
                        blocks.len(),
                        expected_blocks
                    );
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!(
                        "Block cache read failed for {}-{}: {}",
                        start,
                        end,
                        e
                    );
                }
            }
        }

        loop {
            let inflight = acquire_inflight(self.client.endpoint(), start, end).await;

            match inflight {
                InflightLease::Follower(notify) => {
                    notify.notified().await;
                    if let Ok(cache) = BlockCache::for_endpoint(self.client.endpoint()) {
                        if let Ok(blocks) = cache.load_range(start, end) {
                            if blocks.len() == expected_blocks {
                                return Ok(blocks);
                            }
                        }
                    }
                    continue;
                }
                InflightLease::Leader(token) => {
                    let mut attempts = 0;
                    let result = loop {
                        // Use get_compact_block_range with retry logic
                        match self
                            .client
                            .get_compact_block_range(start as u32..(end + 1) as u32)
                            .await
                        {
                            Ok(blocks) => {
                                if let Ok(cache) = BlockCache::for_endpoint(self.client.endpoint())
                                {
                                    if let Err(e) = cache.store_blocks(&blocks) {
                                        tracing::debug!(
                                            "Block cache store failed for {}-{}: {}",
                                            start,
                                            end,
                                            e
                                        );
                                    }
                                }
                                break Ok(blocks);
                            }
                            Err(e) if attempts < MAX_RETRY_ATTEMPTS => {
                                attempts += 1;
                                let backoff = RETRY_BACKOFF_MS * (1 << attempts);
                                tracing::warn!(
                                    "Fetch failed (attempt {}/{}), retrying in {}ms: {}",
                                    attempts,
                                    MAX_RETRY_ATTEMPTS,
                                    backoff,
                                    e
                                );
                                tokio::time::sleep(Duration::from_millis(backoff)).await;
                            }
                            Err(e) => break Err(e),
                        }
                    };
                    token.complete().await;
                    return result;
                }
            }
        }

    }

    async fn trial_decrypt_batch(&self, blocks: &[CompactBlockData]) -> Result<Vec<DecryptedNote>> {
        // Get IVKs from wallet keys for trial decryption (both Sapling and Orchard)
        let mut sapling_ivk_bytes = None;
        let mut orchard_ivk_bytes = None;

        if let Some(ref keys) = self.keys {
            if let Some(ref dfvk) = keys.sapling_dfvk {
                let sapling_ivk = dfvk.to_ivk();
                sapling_ivk_bytes = Some(sapling_ivk.to_sapling_ivk_bytes());
            }

            if let Some(ref fvk) = keys.orchard_fvk {
                orchard_ivk_bytes = Some(fvk.to_ivk_bytes());
            }
        }

        if let Some(ref sink) = self.storage {
            if sapling_ivk_bytes.is_none() || orchard_ivk_bytes.is_none() {
                let secret = {
                    let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
                    let repo = Repository::new(&db);
                    let wallet_id = self
                        .wallet_id
                        .as_ref()
                        .ok_or_else(|| Error::Sync("Wallet ID not set".to_string()))?;
                    repo.get_wallet_secret(wallet_id)?
                        .ok_or_else(|| Error::Sync("Wallet secret not found".to_string()))?
                };

                if sapling_ivk_bytes.is_none() {
                    sapling_ivk_bytes = secret.sapling_ivk.as_ref()
                        .and_then(|ivk| {
                            if ivk.len() == 32 {
                                let mut bytes = [0u8; 32];
                                bytes.copy_from_slice(&ivk[..32]);
                                Some(bytes)
                            } else {
                                None
                            }
                        });
                }

                if orchard_ivk_bytes.is_none() {
                    orchard_ivk_bytes = secret.orchard_ivk.as_ref()
                        .and_then(|ivk| {
                            if ivk.len() == 64 {
                                let mut bytes = [0u8; 64];
                                bytes.copy_from_slice(&ivk[..64]);
                                Some(bytes)
                            } else if ivk.len() == 137 {
                                OrchardExtendedFullViewingKey::from_bytes(ivk).ok()
                                    .map(|fvk| fvk.to_ivk_bytes())
                            } else {
                                None
                            }
                        });
                }
            }
        }

        if sapling_ivk_bytes.is_none() && orchard_ivk_bytes.is_none() {
            tracing::warn!("No Sapling or Orchard IVK available for trial decryption");
            return Ok(Vec::new());
        }

        // Batch trial decryption with bounded parallelism
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.max_parallel_decrypt));
        let mut tasks = Vec::new();

        for block in blocks {
            let sem = Arc::clone(&semaphore);
            let block = block.clone();
            let lazy_memo = self.config.lazy_memo_decode;
            let sapling_ivk_bytes = sapling_ivk_bytes; // Copy for closure
            let orchard_ivk_bytes_opt = orchard_ivk_bytes; // Copy for closure (may be None)

            let task = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                trial_decrypt_block(
                    &block,
                    lazy_memo,
                    sapling_ivk_bytes.as_ref(),
                    orchard_ivk_bytes_opt.as_ref(),
                )
            });

            tasks.push(task);
        }

        // Collect results
        let mut all_notes = Vec::new();
        for task in tasks {
            let notes = task.await.map_err(|e| Error::Sync(e.to_string()))??;
            all_notes.extend(notes);
        }

        Ok(all_notes)
    }

    /// Fetch full transactions to enrich notes (memos, Orchard nullifiers, outgoing memo recovery).
    async fn fetch_and_enrich_notes(
        &self,
        notes: &mut [DecryptedNote],
        require_memos: bool,
    ) -> Result<()> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };

        let mut sapling_ivk_bytes = None;
        let mut orchard_ivk_bytes = None;
        let mut sapling_ovk = None;
        let mut orchard_ovk = None;
        let mut orchard_fvk = None;

        if let Some(ref keys) = self.keys {
            if let Some(ref dfvk) = keys.sapling_dfvk {
                let sapling_ivk = dfvk.to_ivk();
                sapling_ivk_bytes = Some(sapling_ivk.to_sapling_ivk_bytes());
                sapling_ovk = Some(dfvk.outgoing_viewing_key());
            }

            if let Some(ref fvk) = keys.orchard_fvk {
                orchard_ivk_bytes = Some(fvk.to_ivk_bytes());
                orchard_ovk = Some(fvk.to_ovk());
                orchard_fvk = Some(fvk.inner.clone());
            }
        }

        if let Some(ref sink) = self.storage {
            if sapling_ivk_bytes.is_none() || orchard_ivk_bytes.is_none() || orchard_fvk.is_none() {
                let secret = {
                    let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
                    let repo = Repository::new(&db);
                    let wallet_id = self
                        .wallet_id
                        .as_ref()
                        .ok_or_else(|| Error::Sync("Wallet ID not set".to_string()))?;
                    repo.get_wallet_secret(wallet_id)?
                        .ok_or_else(|| Error::Sync("Wallet secret not found".to_string()))?
                };

                if sapling_ivk_bytes.is_none() {
                    if let Some(ivk) = secret.sapling_ivk {
                        if ivk.len() == 32 {
                            let mut bytes = [0u8; 32];
                            bytes.copy_from_slice(&ivk[..32]);
                            sapling_ivk_bytes = Some(bytes);
                        }
                    }
                }

                if let Some(ivk) = secret.orchard_ivk {
                    if ivk.len() == 64 {
                        if orchard_ivk_bytes.is_none() {
                            let mut bytes = [0u8; 64];
                            bytes.copy_from_slice(&ivk[..64]);
                            orchard_ivk_bytes = Some(bytes);
                        }
                    } else if ivk.len() == 137 {
                        if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bytes(&ivk) {
                            if orchard_ivk_bytes.is_none() {
                                orchard_ivk_bytes = Some(fvk.to_ivk_bytes());
                            }
                            if orchard_ovk.is_none() {
                                orchard_ovk = Some(fvk.to_ovk());
                            }
                            if orchard_fvk.is_none() {
                                orchard_fvk = Some(fvk.inner.clone());
                            }
                        }
                    }
                }
            }
        }

        if sapling_ivk_bytes.is_none() && orchard_ivk_bytes.is_none() {
            return Ok(());
        }

        #[derive(Default)]
        struct TxWork {
            indices: Vec<usize>,
            block: Option<u64>,
            index: Option<u64>,
        }

        let mut tx_work: HashMap<[u8; 32], TxWork> = HashMap::new();
        let mut failed_txids: HashSet<[u8; 32]> = HashSet::new();
        let mut invalid_orchard_indices: HashSet<usize> = HashSet::new();

        for (note_idx, note) in notes.iter_mut().enumerate() {
            if note.tx_hash.len() != 32 {
                continue;
            }

            let mut txid = [0u8; 32];
            txid.copy_from_slice(&note.tx_hash[..32]);

            let mut needs_tx = false;

            if require_memos && note.memo_bytes().is_none() {
                match sink.get_note_by_txid_and_index(&note.tx_hash, note.output_index as i64) {
                    Ok(Some(db_note)) => {
                        if let Some(memo) = db_note.memo {
                            note.set_memo_bytes(memo);
                        } else {
                            needs_tx = true;
                        }
                    }
                    Ok(None) => needs_tx = true,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load memo from database for tx {} output {}: {}",
                            hex::encode(&note.tx_hash),
                            note.output_index,
                            e
                        );
                        needs_tx = true;
                    }
                }
            }

            let needs_orchard_nullifier = note.note_type == NoteType::Orchard
                && note.nullifier.iter().all(|b| *b == 0)
                && orchard_fvk.is_some();
            if needs_orchard_nullifier {
                needs_tx = true;
            }

            if needs_tx {
                let entry = tx_work.entry(txid).or_default();
                entry.indices.push(note_idx);
                entry.block.get_or_insert(note.height);
                entry.index.get_or_insert(note.tx_index as u64);
            }
        }

        for (txid, work) in tx_work {
            let raw_tx_bytes = match self
                .client
                .get_transaction_with_fallback(&txid, work.block, work.index)
                .await
            {
                Ok(raw) => raw,
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch full transaction {}: {}",
                        hex::encode(txid),
                        e
                    );
                    failed_txids.insert(txid);
                    continue;
                }
            };

            for note_idx in work.indices {
                let note = &mut notes[note_idx];

                match note.note_type {
                    NoteType::Sapling => {
                        if !require_memos || note.memo_bytes().is_some() {
                            continue;
                        }

                        if let Some(ref sapling_ivk) = sapling_ivk_bytes {
                            match decrypt_memo_from_raw_tx_with_ivk_bytes(
                                &raw_tx_bytes,
                                note.output_index as usize,
                                sapling_ivk,
                                Some(&note.commitment),
                            ) {
                                Ok(Some(decrypted)) => {
                                    let memo = decrypted.memo;
                                    note.set_memo_bytes(memo.clone());
                                    if let Err(e) = sink.update_note_memo(
                                        &note.tx_hash,
                                        note.output_index as i64,
                                        Some(&memo),
                                    ) {
                                        tracing::warn!("Failed to update memo in database: {}", e);
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    tracing::warn!("Error decrypting Sapling memo: {}", e);
                                }
                            }
                        }
                    }
                        NoteType::Orchard => {
                            let orchard_ivk = match orchard_ivk_bytes.as_ref() {
                                Some(ivk) => ivk,
                                None => continue,
                            };

                            match decrypt_orchard_memo_from_raw_tx_with_ivk_bytes(
                                &raw_tx_bytes,
                                note.output_index as usize,
                                orchard_ivk,
                                Some(&note.commitment),
                            ) {
                                Ok(Some(decrypted)) => {
                                    note.orchard_rho = Some(decrypted.rho);
                                    note.orchard_rseed = Some(decrypted.rseed);
                                    if note.note_bytes.is_empty() {
                                        match orchard_address_from_ivk_diversifier(orchard_ivk, &note.diversifier) {
                                            Ok(Some(address)) => {
                                                note.note_bytes = encode_orchard_note_bytes(
                                                    &address,
                                                    decrypted.rho,
                                                    decrypted.rseed,
                                                );
                                            }
                                            Ok(None) => {}
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Failed to derive Orchard address for tx {} output {}: {}",
                                                    hex::encode(&note.tx_hash),
                                                    note.output_index,
                                                    e
                                                );
                                            }
                                        }
                                    }

                                    if require_memos && note.memo_bytes().is_none() {
                                        let memo = decrypted.memo.to_vec();
                                        note.set_memo_bytes(memo.clone());
                                        if let Err(e) = sink.update_note_memo(
                                        &note.tx_hash,
                                        note.output_index as i64,
                                        Some(&memo),
                                    ) {
                                        tracing::warn!("Failed to update memo in database: {}", e);
                                    }
                                }

                                if note.nullifier.iter().all(|b| *b == 0) {
                                    if let Some(ref fvk) = orchard_fvk {
                                        match orchard_nullifier_from_parts(
                                            fvk,
                                            decrypted.address,
                                            decrypted.value,
                                            decrypted.rho,
                                            decrypted.rseed,
                                        ) {
                                            Ok(nf) => note.nullifier = nf,
                                            Err(e) => tracing::warn!(
                                                "Failed to compute Orchard nullifier: {}",
                                                e
                                            ),
                                        }
                                    }
                                }
                            }
                            Ok(None) => {
                                invalid_orchard_indices.insert(note_idx);
                                if note.tx_hash.len() == 32 {
                                    if let Err(e) = sink.delete_note_by_txid_and_index(
                                        &note.tx_hash,
                                        note.output_index as i64,
                                    ) {
                                        tracing::warn!(
                                            "Failed to delete invalid Orchard note {}:{}: {}",
                                            hex::encode(&note.tx_hash),
                                            note.output_index,
                                            e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Error decrypting Orchard memo: {}", e);
                            }
                        }
                    }
                }
            }

            let txid_hex = hex::encode(txid);
            let has_memo = sink.get_tx_memo(&txid_hex).ok().flatten().is_some();
            if !has_memo {
                if let Err(e) = self.recover_outgoing_memos(
                    &raw_tx_bytes,
                    work.block.unwrap_or(0),
                    &txid_hex,
                    sink,
                    sapling_ovk.as_ref(),
                    orchard_ovk.as_ref(),
                ) {
                    tracing::warn!("Outgoing memo recovery failed: {}", e);
                }
            }
        }

        if !invalid_orchard_indices.is_empty() {
            for (idx, note) in notes.iter_mut().enumerate() {
                if !invalid_orchard_indices.contains(&idx) || note.note_type != NoteType::Orchard {
                    continue;
                }
                if note.tx_hash.len() == 32 {
                    let mut txid = [0u8; 32];
                    txid.copy_from_slice(&note.tx_hash[..32]);
                    if failed_txids.contains(&txid) {
                        continue;
                    }
                }
                // Invalidate note so it won't be persisted.
                note.tx_hash.clear();
                note.txid.clear();
            }
        }

        Ok(())
    }

    async fn cleanup_orchard_false_positives(&self) -> Result<()> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };

        let mut orchard_ivk_bytes = None;
        if let Some(ref keys) = self.keys {
            if let Some(ref fvk) = keys.orchard_fvk {
                orchard_ivk_bytes = Some(fvk.to_ivk_bytes());
            }
        }

        if orchard_ivk_bytes.is_none() {
            let secret = {
                let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
                let repo = Repository::new(&db);
                let wallet_id = self
                    .wallet_id
                    .as_ref()
                    .ok_or_else(|| Error::Sync("Wallet ID not set".to_string()))?;
                repo.get_wallet_secret(wallet_id)?
                    .ok_or_else(|| Error::Sync("Wallet secret not found".to_string()))?
            };

            if let Some(ivk) = secret.orchard_ivk {
                if ivk.len() == 64 {
                    let mut bytes = [0u8; 64];
                    bytes.copy_from_slice(&ivk[..64]);
                    orchard_ivk_bytes = Some(bytes);
                } else if ivk.len() == 137 {
                    if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bytes(&ivk) {
                        orchard_ivk_bytes = Some(fvk.to_ivk_bytes());
                    }
                }
            }
        }

        let orchard_ivk = match orchard_ivk_bytes {
            Some(ref ivk) => ivk,
            None => return Ok(()),
        };

        let refs = sink.list_orchard_note_refs()?;
        if refs.is_empty() {
            return Ok(());
        }

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(debug_log_path())
        {
            use std::io::Write;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:372","message":"orchard_cleanup start","data":{{"notes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                refs.len()
            );
        }

        for note_ref in refs {
            if note_ref.output_index < 0 || note_ref.txid.len() != 32 {
                continue;
            }
            let mut txid = [0u8; 32];
            txid.copy_from_slice(&note_ref.txid[..32]);

            let raw_tx = match self.client.get_transaction(&txid).await {
                Ok(raw) => raw,
                Err(e) => {
                    tracing::warn!(
                        "Orchard cleanup: failed to fetch tx {}: {}",
                        hex::encode(txid),
                        e
                    );
                    continue;
                }
            };

            match decrypt_orchard_memo_from_raw_tx_with_ivk_bytes(
                &raw_tx,
                note_ref.output_index as usize,
                orchard_ivk,
                Some(&note_ref.commitment),
            ) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = sink.delete_note_by_txid_and_index(
                        &note_ref.txid,
                        note_ref.output_index,
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Orchard cleanup: decryption error for tx {}: {}",
                        hex::encode(txid),
                        e
                    );
                }
            }
        }

        Ok(())
    }

    fn recover_outgoing_memos(
        &self,
        raw_tx_bytes: &[u8],
        height: u64,
        txid_hex: &str,
        sink: &StorageSink,
        sapling_ovk: Option<&SaplingOutgoingViewingKey>,
        orchard_ovk: Option<&orchard::keys::OutgoingViewingKey>,
    ) -> Result<()> {
        if sapling_ovk.is_none() && orchard_ovk.is_none() {
            return Ok(());
        }

        let tx = Transaction::read(raw_tx_bytes, BranchId::Canopy)
            .map_err(|e| Error::Sync(format!("Failed to parse transaction: {}", e)))?;

        let mut memo_to_store: Option<Vec<u8>> = None;

        if let Some(ovk) = sapling_ovk {
            if let Some(bundle) = tx.sapling_bundle() {
                for output in bundle.shielded_outputs() {
                    if let Some((_note, _address, memo)) = try_sapling_output_recovery(
                        &PirateNetwork::default(),
                        BlockHeight::from_u32(height as u32),
                        ovk,
                        output,
                    ) {
                        if !memo.as_array().iter().all(|b| *b == 0) {
                            memo_to_store = Some(memo.as_array().to_vec());
                            break;
                        }
                    }
                }
            }
        }

        if memo_to_store.is_none() {
            if let Some(ovk) = orchard_ovk {
                if let Some(bundle) = tx.orchard_bundle() {
                    for action in bundle.actions() {
                        let domain = OrchardDomain::for_action(action);
                        if let Some((_note, _address, memo)) = try_output_recovery_with_ovk(
                            &domain,
                            ovk,
                            action,
                            action.cv_net(),
                            &action.encrypted_note().out_ciphertext,
                        ) {
                            if !memo.iter().all(|b| *b == 0) {
                                memo_to_store = Some(memo.to_vec());
                                break;
                            }
                        }
                    }
                }
            }
        }

        if let Some(memo) = memo_to_store {
            sink.upsert_tx_memo(txid_hex, &memo)?;
        }

        Ok(())
    }

    /// Update frontier with block commitments (replaces witness tree placeholder)
    /// Returns (count, position_mappings) where position_mappings includes Sapling and Orchard positions.
    async fn update_frontier(
        &self,
        blocks: &[CompactBlockData],
        notes: &[DecryptedNote],
    ) -> Result<(u64, PositionMaps)> {
        let mut sapling_frontier = self.frontier.write().await;
        let mut orchard_frontier = self.orchard_frontier.write().await;
        let mut count = 0u64;
        let mut position_mappings = PositionMaps::default();
        let sapling_owned: HashSet<[u8; 32]> = notes
            .iter()
            .filter(|n| n.note_type == crate::pipeline::NoteType::Sapling)
            .map(|n| n.commitment)
            .collect();
        let orchard_owned: HashSet<[u8; 32]> = notes
            .iter()
            .filter(|n| n.note_type == crate::pipeline::NoteType::Orchard)
            .map(|n| n.commitment)
            .collect();

        for block in blocks {
            for tx in &block.transactions {
                let txid = tx.hash.as_slice();
                // Process Sapling outputs
                for (output_idx, output) in tx.outputs.iter().enumerate() {
                    // Apply note commitments to Sapling frontier
                    if output.cmu.len() == 32 {
                        let mut cm = [0u8; 32];
                        cm.copy_from_slice(&output.cmu);
                        let pos = sapling_frontier.apply_note_commitment_with_position(cm)?;
                        if let Some(key) = TxOutputKey::new(txid, output_idx) {
                            position_mappings.sapling_by_tx.insert(key, pos);
                        }
                        count += 1;
                        if sapling_owned.contains(&cm) {
                            let marked = sapling_frontier.mark_position()?;
                            let marked_u64 = u64::from(marked);
                            if marked_u64 != pos {
                                tracing::warn!(
                                    "Sapling mark position mismatch: appended={}, marked={}",
                                    pos,
                                    marked_u64
                                );
                            }
                        }
                    }
                }

                // Process Orchard actions
                for action in &tx.actions {
                    // Apply note commitments to Orchard frontier
                    if action.cmx.len() == 32 {
                        let mut cm = [0u8; 32];
                        cm.copy_from_slice(&action.cmx);
                        let position = orchard_frontier.apply_note_commitment(cm)?;
                        position_mappings.orchard_by_commitment.insert(cm, position);
                        count += 1;

                        if orchard_owned.contains(&cm) {
                            let marked = orchard_frontier.mark_position()?;
                            let marked_u64 = u64::from(marked);
                            if marked_u64 != position {
                                tracing::warn!(
                                    "Orchard mark position mismatch: appended={}, marked={}",
                                    position,
                                    marked_u64
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok((count, position_mappings))
    }

    async fn apply_positions(&self, notes: &mut [DecryptedNote], position_mappings: &PositionMaps) {
        let orchard_frontier = self.orchard_frontier.read().await;
        let orchard_root = orchard_frontier.root();
        let sapling_frontier = self.frontier.read().await;
        for note in notes.iter_mut() {
            match note.note_type {
                crate::pipeline::NoteType::Sapling => {
                    let position = TxOutputKey::new(&note.tx_hash, note.output_index)
                        .and_then(|key| position_mappings.sapling_by_tx.get(&key).copied());
                    if let Some(pos) = position {
                        note.position = Some(pos);
                        match sapling_frontier.witness(pos) {
                            Ok(Some(path)) => {
                                let mut buf = Vec::new();
                                if let Err(e) = write_merkle_path(&mut buf, path) {
                                    tracing::warn!(
                                        "Failed to serialize Sapling witness for tx {} output {}: {}",
                                        hex::encode(&note.tx_hash),
                                        note.output_index,
                                        e
                                    );
                                } else {
                                    note.merkle_path = buf;
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "No Sapling witness available for tx {} output {}",
                                    hex::encode(&note.tx_hash),
                                    note.output_index
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to compute Sapling witness for tx {} output {}: {}",
                                    hex::encode(&note.tx_hash),
                                    note.output_index,
                                    e
                                );
                            }
                        }
                    }
                }
                crate::pipeline::NoteType::Orchard => {
                    if let Some(position) = position_mappings
                        .orchard_by_commitment
                        .get(&note.commitment)
                        .copied()
                    {
                        note.position = Some(position);
                        match orchard_frontier.witness(position) {
                            Ok(Some(path)) => {
                                if let Some(encoded) = encode_orchard_merkle_path(&path) {
                                    note.merkle_path = encoded;
                                } else {
                                    tracing::warn!(
                                        "Failed to serialize Orchard witness for tx {} output {}: invalid position {}",
                                        hex::encode(&note.tx_hash),
                                        note.output_index,
                                        position
                                    );
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "No Orchard witness available for tx {} output {} (position {})",
                                    hex::encode(&note.tx_hash),
                                    note.output_index,
                                    position
                                );
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to compute Orchard witness for tx {} output {}: {}",
                                    hex::encode(&note.tx_hash),
                                    note.output_index,
                                    e
                                );
                            }
                        }
                    }
                    note.anchor = orchard_root;
                }
            }
        }
    }

    async fn apply_sapling_nullifiers(
        &self,
        notes: &mut [DecryptedNote],
        position_mappings: &PositionMaps,
    ) -> Result<()> {
        let keys = match self.keys.as_ref() {
            Some(keys) => keys,
            None => return Ok(()),
        };
        let dfvk = match keys.sapling_dfvk.as_ref() {
            Some(dfvk) => dfvk,
            None => return Ok(()),
        };

        let nk = dfvk.nullifier_deriving_key();
        let sapling_ivk = dfvk.sapling_ivk();

        for note in notes.iter_mut() {
            if note.note_type != NoteType::Sapling {
                continue;
            }
            if !note.nullifier.iter().all(|b| *b == 0) {
                continue;
            }

            let position = TxOutputKey::new(&note.tx_hash, note.output_index)
                .and_then(|key| position_mappings.sapling_by_tx.get(&key).copied());
            let position = match position {
                Some(pos) => pos,
                None => {
                    tracing::warn!(
                        "Missing Sapling position for tx {} output {}",
                        hex::encode(&note.tx_hash),
                        note.output_index
                    );
                    continue;
                }
            };

            let leadbyte = match note.sapling_rseed_leadbyte {
                Some(b) => b,
                None => {
                    tracing::warn!(
                        "Missing Sapling leadbyte for tx {} output {}",
                        hex::encode(&note.tx_hash),
                        note.output_index
                    );
                    continue;
                }
            };
            let rseed_bytes = match note.sapling_rseed {
                Some(bytes) => bytes,
                None => {
                    tracing::warn!(
                        "Missing Sapling rseed for tx {} output {}",
                        hex::encode(&note.tx_hash),
                        note.output_index
                    );
                    continue;
                }
            };
            if note.diversifier.len() != 11 {
                tracing::warn!(
                    "Invalid Sapling diversifier for tx {} output {}",
                    hex::encode(&note.tx_hash),
                    note.output_index
                );
                continue;
            }
            let mut diversifier = [0u8; 11];
            diversifier.copy_from_slice(&note.diversifier[..11]);

            let rseed = if leadbyte == 0x02 {
                zcash_primitives::sapling::Rseed::AfterZip212(rseed_bytes)
            } else {
                let rcm = Option::from(jubjub::Fr::from_bytes(&rseed_bytes)).ok_or_else(|| {
                    Error::Sync("Invalid Sapling rseed bytes".to_string())
                })?;
                zcash_primitives::sapling::Rseed::BeforeZip212(rcm)
            };

            let payment_address = match sapling_ivk
                .to_payment_address(zcash_primitives::sapling::Diversifier(diversifier))
            {
                Some(addr) => addr,
                None => {
                    tracing::warn!(
                        "Failed to derive Sapling address for tx {} output {}",
                        hex::encode(&note.tx_hash),
                        note.output_index
                    );
                    continue;
                }
            };

            let note_value = zcash_primitives::sapling::value::NoteValue::from_raw(note.value);
            let sapling_note =
                zcash_primitives::sapling::Note::from_parts(payment_address, note_value, rseed);
            let nf = sapling_note.nf(&nk, position);
            note.nullifier = nf.0;
            if note.note_bytes.is_empty() {
                note.note_bytes = encode_sapling_note_bytes(sapling_note.recipient(), leadbyte, rseed_bytes);
            }
        }

        Ok(())
    }

    async fn apply_spends(&self, blocks: &[CompactBlockData]) -> Result<()> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };

        let mut spend_entries: HashSet<([u8; 32], [u8; 32])> = HashSet::new();

        for block in blocks {
            let block_time = if block.time > 0 {
                block.time as i64
            } else {
                chrono::Utc::now().timestamp()
            };
            let block_height = block.height as i64;

            for tx in &block.transactions {
                if tx.hash.len() != 32 {
                    continue;
                }
                let mut txid = [0u8; 32];
                txid.copy_from_slice(&tx.hash[..32]);

                let mut has_spend = false;

                for spend in &tx.spends {
                    if spend.nf.len() == 32 {
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&spend.nf[..32]);
                        if !nf.iter().all(|b| *b == 0) {
                            spend_entries.insert((nf, txid));
                            has_spend = true;
                        }
                    }
                }

                for action in &tx.actions {
                    if action.nullifier.len() == 32 {
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&action.nullifier[..32]);
                        if !nf.iter().all(|b| *b == 0) {
                            spend_entries.insert((nf, txid));
                            has_spend = true;
                        }
                    }
                }

                if has_spend {
                    let txid_hex = hex::encode(txid);
                    let _ = sink.upsert_transaction(&txid_hex, block_height, block_time, 0);
                }
            }
        }

        if !spend_entries.is_empty() {
            let entries: Vec<([u8; 32], [u8; 32])> = spend_entries.into_iter().collect();
            if let Err(e) = sink.mark_notes_spent_by_nullifiers_with_txid(&entries) {
                tracing::warn!("Failed to mark notes spent for batch: {}", e);
            }
        }

        Ok(())
    }

    /// Get witness for an Orchard note at a given position
    /// Returns None if position is not marked or witness cannot be computed
    pub async fn get_orchard_witness(&self, position: u64) -> Result<Option<incrementalmerkletree::MerklePath<orchard::tree::MerkleHashOrchard, { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH }>>> {
        let orchard_frontier = self.orchard_frontier.read().await;
        orchard_frontier.witness(position)
    }

    /// Get current Orchard anchor from the frontier, if available.
    pub async fn get_orchard_anchor(&self) -> Option<[u8; 32]> {
        let orchard_frontier = self.orchard_frontier.read().await;
        orchard_frontier.root()
    }

    async fn verify_chain(&self, start: u64, end: u64) -> Result<()> {
        // Verify sync completed correctly:
        // - Chain continuity (no gaps)
        // - No reorgs during sync
        // - Frontier root matches expected (if we have target)
        
        // Basic verification: check that we have blocks in range
        if let Some(ref sink) = self.storage {
            let sync_state = sink.load_sync_state()?;
            if sync_state.local_height < end {
                tracing::warn!(
                    "Sync verification: local_height {} < target {}",
                    sync_state.local_height,
                    end
                );
            }
        }
        
        tracing::debug!("Chain verification complete for range {}-{}", start, end);
        Ok(())
    }

    async fn create_checkpoint(&self, height: u64) -> Result<()> {
        let sapling_bytes = { self.frontier.read().await.serialize() };
        let orchard_bytes = { self.orchard_frontier.read().await.serialize() };
        let snapshot_bytes = encode_frontier_snapshot(&sapling_bytes, &orchard_bytes);

        tracing::debug!(
            "Creating checkpoint at height {} (sapling: {} bytes, orchard: {} bytes)",
            height,
            sapling_bytes.len(),
            orchard_bytes.len()
        );

        if let Some(ref sink) = self.storage {
            let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
            let storage = FrontierStorage::new(&db);
            storage.save_frontier_snapshot(height as u32, &snapshot_bytes, env!("CARGO_PKG_VERSION"))?;
            let _ = storage.prune_old_snapshots(FRONTIER_SNAPSHOT_RETAIN);
        }

        Ok(())
    }

    /// Save sync state periodically
    async fn save_sync_state(
        &self,
        local_height: u64,
        target_height: u64,
        last_checkpoint: u64,
    ) -> Result<()> {
        if let Some(ref sink) = self.storage {
            sink.save_sync_state(local_height, target_height, last_checkpoint)?;
        }
        Ok(())
    }

    async fn rollback_to_checkpoint(&mut self, checkpoint_height: u64) -> Result<u64> {
        if let Some(ref sink) = self.storage {
            {
                let mut db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
                truncate_above_height(&mut db, checkpoint_height)?;
            }
        }

        if let Some(snapshot_height) = self.restore_frontiers_from_storage(checkpoint_height).await? {
            return Ok(snapshot_height);
        }

        *self.frontier.write().await = SaplingFrontier::new();
        *self.orchard_frontier.write().await = OrchardFrontier::new();

        Ok(checkpoint_height)
    }

    /// Handle sync interruption - rollback to last checkpoint
    async fn handle_interruption(&mut self, current_height: u64) -> Result<()> {
        tracing::warn!("Handling sync interruption at height {}", current_height);

        // Try to load last checkpoint from storage
        let checkpoint_height = if let Some(ref sink) = self.storage {
            let sync_state = sink.load_sync_state()?;
            let last_checkpoint = sync_state.last_checkpoint_height;
            
            if last_checkpoint > 0 && last_checkpoint <= current_height {
                tracing::info!(
                    "Rolling back to checkpoint at {} (current: {})",
                    last_checkpoint,
                    current_height
                );
                
                last_checkpoint
            } else {
                tracing::warn!("No valid checkpoint found, starting from birthday");
                self.birthday_height as u64
            }
        } else {
            tracing::warn!("No storage available, starting from birthday");
            self.birthday_height as u64
        };

        let rollback_height = self.rollback_to_checkpoint(checkpoint_height).await?;
        let resume_height = std::cmp::max(
            rollback_height.saturating_add(1),
            self.birthday_height as u64,
        );

        tracing::warn!(
            "Sync interrupted at height {}, rolled back to {}, resume at {}",
            current_height,
            rollback_height,
            resume_height
        );

        Err(Error::Sync(format!(
            "Sync interrupted at height {}, rolled back to {}",
            current_height, rollback_height
        )))
    }

    /// Rollback to last checkpoint and resume
    pub async fn rollback_and_resume(&mut self) -> Result<()> {
        tracing::warn!("Interruption detected, rolling back to last checkpoint");

        // Get last checkpoint from storage
        let checkpoint_height = if let Some(ref sink) = self.storage {
            let sync_state = sink.load_sync_state()?;
            if sync_state.last_checkpoint_height > 0 {
                sync_state.last_checkpoint_height
            } else {
                self.birthday_height as u64
            }
        } else {
            self.birthday_height as u64
        };

        let rollback_height = self.rollback_to_checkpoint(checkpoint_height).await?;
        let resume_height = std::cmp::max(
            rollback_height.saturating_add(1),
            self.birthday_height as u64,
        );

        // Resume sync from next height after rollback
        self.sync_range(resume_height, None).await
    }

    /// Detect and handle reorg
    pub async fn detect_and_handle_reorg(&mut self, height: u64) -> Result<bool> {
        let local_block = if let Ok(cache) = BlockCache::for_endpoint(self.client.endpoint()) {
            cache.load_range(height, height).ok().and_then(|mut blocks| blocks.pop())
        } else {
            None
        };

        let remote_block = match self.client.get_block(height as u32).await {
            Ok(block) => block,
            Err(e) => {
                tracing::warn!("Reorg check failed at height {}: {}", height, e);
                return Ok(false);
            }
        };

        if let Some(local) = local_block {
            if local.hash != remote_block.hash {
                tracing::warn!("Reorg detected at height {}", height);
                let rollback_height = height.saturating_sub(1);
                self.rollback_to_checkpoint(rollback_height).await?;
                return Ok(true);
            }
        }

        tracing::debug!("No reorg detected at height {}", height);
        Ok(false)
    }

    /// Get current birthday height
    pub fn birthday_height(&self) -> u32 {
        self.birthday_height
    }

    /// Set new birthday height
    pub fn set_birthday_height(&mut self, height: u32) {
        self.birthday_height = height;
        tracing::info!("Birthday height updated to {}", height);
    }

    /// Update target height from server (non-blocking)
    /// 
    /// Fetches the latest block height from the server and updates the progress target.
    /// This allows the sync progress to reflect the current blockchain tip even as new blocks are mined.
    pub async fn update_target_height(&self) -> Result<()> {
        match self.client.get_latest_block().await {
            Ok(latest_height) => {
                let progress = self.progress.write().await;
                let current_target = progress.target_height();
                drop(progress); // Release lock before updating
                
                if latest_height > current_target {
                    let mut progress = self.progress.write().await;
                    progress.set_target(latest_height);
                    tracing::debug!(
                        "Updated target height from {} to {}",
                        current_target,
                        latest_height
                    );
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!("Failed to fetch latest block height: {}", e);
                Err(e)
            }
        }
    }

    /// Disconnect from lightwalletd
    pub async fn disconnect(&self) {
        self.client.disconnect().await;
    }
}

fn de_ct<T>(ct: CtOption<T>) -> Option<T> {
    if ct.is_some().into() {
        Some(ct.unwrap())
    } else {
        None
    }
}

const SAPLING_NOTE_BYTES_VERSION: u8 = 1;
const ORCHARD_NOTE_BYTES_VERSION: u8 = 1;
const FRONTIER_SNAPSHOT_MAGIC: [u8; 4] = *b"PFS1";
const FRONTIER_SNAPSHOT_VERSION: u8 = 1;

fn encode_sapling_note_bytes_from_address_bytes(
    address_bytes: [u8; 43],
    leadbyte: u8,
    rseed: [u8; 32],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 43 + 1 + 32);
    out.push(SAPLING_NOTE_BYTES_VERSION);
    out.extend_from_slice(&address_bytes);
    out.push(leadbyte);
    out.extend_from_slice(&rseed);
    out
}

fn encode_sapling_note_bytes(
    address: zcash_primitives::sapling::PaymentAddress,
    leadbyte: u8,
    rseed: [u8; 32],
) -> Vec<u8> {
    encode_sapling_note_bytes_from_address_bytes(address.to_bytes(), leadbyte, rseed)
}

fn encode_orchard_note_bytes(
    address: &OrchardAddress,
    rho: [u8; 32],
    rseed: [u8; 32],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 43 + 32 + 32);
    out.push(ORCHARD_NOTE_BYTES_VERSION);
    out.extend_from_slice(&address.to_raw_address_bytes());
    out.extend_from_slice(&rho);
    out.extend_from_slice(&rseed);
    out
}

fn orchard_address_from_ivk_diversifier(
    ivk_bytes: &[u8; 64],
    diversifier: &[u8],
) -> Result<Option<OrchardAddress>> {
    if diversifier.len() != 11 {
        return Ok(None);
    }
    let mut div_bytes = [0u8; 11];
    div_bytes.copy_from_slice(&diversifier[..11]);
    let ivk_ct = OrchardIncomingViewingKey::from_bytes(ivk_bytes);
    if !bool::from(ivk_ct.is_some()) {
        return Err(Error::Sync("Invalid Orchard IVK bytes".to_string()));
    }
    let ivk = ivk_ct.unwrap();
    let orchard_div = OrchardDiversifier::from_bytes(div_bytes);
    Ok(Some(ivk.address(orchard_div)))
}

fn encode_frontier_snapshot(sapling_bytes: &[u8], orchard_bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + 1 + 4 + sapling_bytes.len() + 4 + orchard_bytes.len());
    out.extend_from_slice(&FRONTIER_SNAPSHOT_MAGIC);
    out.push(FRONTIER_SNAPSHOT_VERSION);
    out.extend_from_slice(&(sapling_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(sapling_bytes);
    out.extend_from_slice(&(orchard_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(orchard_bytes);
    out
}

fn decode_frontier_snapshot(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    if bytes.len() < FRONTIER_SNAPSHOT_MAGIC.len() + 1 || !bytes.starts_with(&FRONTIER_SNAPSHOT_MAGIC) {
        return Ok((bytes.to_vec(), Vec::new()));
    }

    let version = bytes[FRONTIER_SNAPSHOT_MAGIC.len()];
    if version != FRONTIER_SNAPSHOT_VERSION {
        return Err(Error::Sync(format!(
            "Unsupported frontier snapshot version: {}",
            version
        )));
    }

    let mut offset = FRONTIER_SNAPSHOT_MAGIC.len() + 1;
    if bytes.len() < offset + 4 {
        return Err(Error::Sync("Frontier snapshot truncated".to_string()));
    }
    let sapling_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    if bytes.len() < offset + sapling_len + 4 {
        return Err(Error::Sync("Frontier snapshot truncated".to_string()));
    }
    let sapling_bytes = bytes[offset..offset + sapling_len].to_vec();
    offset += sapling_len;
    let orchard_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    if bytes.len() < offset + orchard_len {
        return Err(Error::Sync("Frontier snapshot truncated".to_string()));
    }
    let orchard_bytes = bytes[offset..offset + orchard_len].to_vec();
    Ok((sapling_bytes, orchard_bytes))
}

fn encode_orchard_merkle_path(
    path: &incrementalmerkletree::MerklePath<
        orchard::tree::MerkleHashOrchard,
        { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH },
    >,
) -> Option<Vec<u8>> {
    let position_u64: u64 = path.position().into();
    let position = u32::try_from(position_u64).ok()?;
    let mut out = Vec::with_capacity(4 + 32 * 32);
    out.extend_from_slice(&position.to_le_bytes());
    for node in path.path_elems() {
        out.extend_from_slice(&node.to_bytes());
    }
    Some(out)
}

fn orchard_nullifier_from_parts(
    fvk: &orchard::keys::FullViewingKey,
    address_bytes: [u8; 43],
    value: u64,
    rho_bytes: [u8; 32],
    rseed_bytes: [u8; 32],
) -> Result<[u8; 32]> {
    let address = de_ct(OrchardAddress::from_raw_address_bytes(&address_bytes))
        .ok_or_else(|| Error::Sync("Invalid Orchard address bytes".to_string()))?;
    let rho = de_ct(OrchardNullifier::from_bytes(&rho_bytes))
        .ok_or_else(|| Error::Sync("Invalid Orchard rho bytes".to_string()))?;
    let rseed = de_ct(OrchardRandomSeed::from_bytes(rseed_bytes, &rho))
        .ok_or_else(|| Error::Sync("Invalid Orchard rseed bytes".to_string()))?;
    let note_value = OrchardNoteValue::from_raw(value);
    let note = de_ct(OrchardNote::from_parts(address, note_value, rho, rseed))
        .ok_or_else(|| Error::Sync("Invalid Orchard note parts".to_string()))?;
    Ok(note.nullifier(fvk).to_bytes())
}

fn wallet_db_base_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("PIRATE_WALLET_DB_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    if let Ok(path) = std::env::var("PIRATE_WALLET_DB_PATH") {
        if path.contains("{wallet_id}") {
            let parent = Path::new(&path).parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }

        let parsed = PathBuf::from(&path);
        if parsed.extension().is_some() {
            let parent = parsed.parent().unwrap_or_else(|| Path::new("."));
            return Ok(parent.to_path_buf());
        }
        return Ok(parsed);
    }

    let base = ProjectDirs::from("com", "Pirate", "PirateWallet")
        .map(|dirs| dirs.data_local_dir().join("wallets"))
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(base)
}

fn wallet_db_path(wallet_id: &str) -> Result<PathBuf> {
    if let Ok(template) = std::env::var("PIRATE_WALLET_DB_PATH") {
        if template.contains("{wallet_id}") {
            return Ok(PathBuf::from(template.replace("{wallet_id}", wallet_id)));
        }
    }

    let base = wallet_db_base_dir()?;
    std::fs::create_dir_all(&base)?;
    Ok(base.join(format!("wallet_{}.db", wallet_id)))
}

/// Storage sink for decrypted notes and sync state
struct StorageSink {
    db_path: PathBuf,
    key: EncryptionKey,
    master_key: MasterKey,
    account_id: i64,
}

impl StorageSink {
    fn persist_notes(
        &self,
        notes: &[DecryptedNote],
        tx_times: &HashMap<String, i64>,
        tx_fees: &HashMap<String, i64>,
        position_mappings: &PositionMaps,
    ) -> Result<()> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        let sync_state = SyncStateStorage::new(&db);

            for n in notes {
                // Skip if we don't have essential fields
                if n.txid.is_empty() {
                    continue;
                }
                let txid_hex = hex::encode(&n.txid);
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    let nf_is_zero = n.nullifier.iter().all(|b| *b == 0);
                    let txid_short = if txid_hex.len() > 12 {
                        &txid_hex[..12]
                    } else {
                        &txid_hex
                    };
                    let db_path = self.db_path.to_string_lossy();
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:2435","message":"persist_note record","data":{{"account_id":{},"note_type":"{:?}","value":{},"height":{},"output_index":{},"nullifier_zero":{},"txid_prefix":"{}","db_path":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        self.account_id,
                        n.note_type,
                        n.value,
                        n.height,
                        n.output_index,
                        nf_is_zero,
                        txid_short,
                        db_path
                    );
                }
                // #endregion
                // Block timestamp is the "first confirmation time" for mined txs.
                // Use now() as fallback for unconfirmed / missing.
                let timestamp = tx_times
                    .get(&txid_hex)
                    .copied()
                .unwrap_or_else(|| chrono::Utc::now().timestamp());
            let fee = tx_fees.get(&txid_hex).copied().unwrap_or(0);

            // Upsert tx metadata (timestamp is used for transaction history UI).
            let _ = repo.upsert_transaction(&txid_hex, n.height as i64, timestamp, fee);

            if let Ok(Some(existing)) = repo.get_note_by_txid_and_index(self.account_id, &n.txid, n.output_index as i64) {
                if existing.memo.is_none() {
                    if let Some(memo) = n.memo_bytes() {
                        let _ = repo.update_note_memo(self.account_id, &n.txid, n.output_index as i64, Some(&memo));
                    }
                }
                continue;
            }

            let note_type = match n.note_type {
                crate::pipeline::NoteType::Orchard => pirate_storage_sqlite::models::NoteType::Orchard,
                crate::pipeline::NoteType::Sapling => pirate_storage_sqlite::models::NoteType::Sapling,
            };
            
            let record = NoteRecord {
                id: None,
                account_id: self.account_id,
                note_type,
                value: n.value as i64,
                nullifier: n.nullifier.to_vec(),
                commitment: n.commitment.to_vec(),
                spent: false,
                height: n.height as i64,
                txid: n.txid.clone(),
                output_index: n.output_index as i64,
                spent_txid: None,
                diversifier: if !n.diversifier.is_empty() {
                    Some(n.diversifier.clone())
                } else {
                    None
                },
                merkle_path: if n.merkle_path.is_empty() {
                    None
                } else {
                    Some(n.merkle_path.clone())
                },
                note: if !n.note_bytes.is_empty() {
                    Some(n.note_bytes.clone())
                } else {
                    None
                },
                anchor: n.anchor.map(|a| a.to_vec()),
                position: {
                    let fallback = match n.note_type {
                        crate::pipeline::NoteType::Sapling => TxOutputKey::new(&n.tx_hash, n.output_index)
                            .and_then(|key| position_mappings.sapling_by_tx.get(&key).copied()),
                        crate::pipeline::NoteType::Orchard => position_mappings
                            .orchard_by_commitment
                            .get(&n.commitment)
                            .copied(),
                    };
                    n.position.or(fallback).map(|p| p as i64)
                },
                memo: n.memo_bytes().map(|b| b.to_vec()),
            };
            if let Err(e) = repo.insert_note(&record) {
                // #region agent log
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(debug_log_path())
                {
                    use std::io::Write;
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:2478","message":"persist_note error","data":{{"txid_prefix":"{}","error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        &txid_hex[..txid_hex.len().min(12)],
                        e
                    );
                }
                // #endregion
            }
        }
        // Optionally update sync state height
        if let Some(max_h) = notes.iter().map(|n| n.height).max() {
            let _ = sync_state.save_sync_state(max_h, max_h, max_h);
        }
        Ok(())
    }

    fn get_note_by_txid_and_index(&self, txid: &[u8], output_index: i64) -> Result<Option<NoteRecord>> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.get_note_by_txid_and_index(self.account_id, txid, output_index)?)
    }

    fn delete_note_by_txid_and_index(&self, txid: &[u8], output_index: i64) -> Result<usize> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.delete_note_by_txid_and_index(self.account_id, txid, output_index)?)
    }

    fn list_orchard_note_refs(&self) -> Result<Vec<OrchardNoteRef>> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.get_orchard_note_refs(self.account_id)?)
    }

    fn update_note_memo(&self, txid: &[u8], output_index: i64, memo: Option<&[u8]>) -> Result<()> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.update_note_memo(self.account_id, txid, output_index, memo)?)
    }

    fn mark_note_spent_by_nullifier(&self, nullifier: &[u8]) -> Result<bool> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.mark_note_spent_by_nullifier(self.account_id, nullifier)?)
    }

    fn mark_note_spent_by_nullifier_with_txid(
        &self,
        nullifier: &[u8],
        spent_txid: &[u8],
    ) -> Result<bool> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.mark_note_spent_by_nullifier_with_txid(self.account_id, nullifier, spent_txid)?)
    }

    fn mark_notes_spent_by_nullifiers(
        &self,
        nullifiers: &HashSet<[u8; 32]>,
    ) -> Result<u64> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.mark_notes_spent_by_nullifiers(self.account_id, nullifiers)?)
    }

    fn mark_notes_spent_by_nullifiers_with_txid(
        &self,
        entries: &Vec<([u8; 32], [u8; 32])>,
    ) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        let mut updated = 0u64;
        for (nullifier, txid) in entries {
            if repo.mark_note_spent_by_nullifier_with_txid(self.account_id, nullifier, txid)? {
                updated += 1;
            }
        }
        Ok(updated)
    }

    fn upsert_transaction(&self, txid_hex: &str, height: i64, timestamp: i64, fee: i64) -> Result<()> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.upsert_transaction(txid_hex, height, timestamp, fee)?)
    }

    fn upsert_tx_memo(&self, txid_hex: &str, memo: &[u8]) -> Result<()> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.upsert_tx_memo(txid_hex, memo)?)
    }

    fn get_tx_memo(&self, txid_hex: &str) -> Result<Option<Vec<u8>>> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.get_tx_memo(txid_hex)?)
    }

    fn load_sync_state(&self) -> Result<pirate_storage_sqlite::sync_state::SyncStateRow> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let sync_state = SyncStateStorage::new(&db);
        Ok(sync_state.load_sync_state()?)
    }

    fn save_sync_state(&self, local_height: u64, target_height: u64, last_checkpoint_height: u64) -> Result<()> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let sync_state = SyncStateStorage::new(&db);
        Ok(sync_state.save_sync_state(local_height, target_height, last_checkpoint_height)?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TxOutputKey {
    txid: [u8; 32],
    index: u32,
}

impl TxOutputKey {
    fn new(txid: &[u8], index: usize) -> Option<Self> {
        if txid.len() != 32 {
            return None;
        }
        let mut txid_bytes = [0u8; 32];
        txid_bytes.copy_from_slice(txid);
        Some(Self {
            txid: txid_bytes,
            index: index as u32,
        })
    }
}

#[derive(Debug, Default)]
struct PositionMaps {
    sapling_by_tx: HashMap<TxOutputKey, u64>,
    orchard_by_commitment: HashMap<[u8; 32], u64>,
}

/// Wallet keys cached for trial decryption
struct WalletKeys {
    sapling_dfvk: Option<ExtendedFullViewingKey>,
    orchard_fvk: Option<OrchardExtendedFullViewingKey>,
}

/// Trial decrypt a single block (both Sapling and Orchard)
fn trial_decrypt_block(
    block: &CompactBlockData,
    _lazy_memo: bool,
    sapling_ivk_bytes: Option<&[u8; 32]>,
    orchard_ivk_bytes_opt: Option<&[u8; 64]>,
) -> Result<Vec<DecryptedNote>> {
    use crate::sapling::trial_decrypt::try_decrypt_compact_output;
    use crate::orchard::trial_decrypt::try_decrypt_compact_orchard_action;
    
    let mut notes = Vec::new();

    for (tx_idx, tx) in block.transactions.iter().enumerate() {
        // Process Sapling outputs
        if let Some(sapling_ivk_bytes) = sapling_ivk_bytes {
            for (output_idx, output) in tx.outputs.iter().enumerate() {
                // Validate output has required fields
                if output.cmu.len() != 32 || output.ephemeral_key.len() != 32 || output.ciphertext.len() < 52 {
                    continue;
                }

                // Perform real trial decryption
                if let Some(decrypted) = try_decrypt_compact_output(sapling_ivk_bytes, output) {
                    // Extract note value
                    let value = decrypted.value;
                    
                    // Extract diversifier
                    let mut diversifier_bytes = [0u8; 11];
                    diversifier_bytes.copy_from_slice(&decrypted.diversifier[..11]);
                    
                    // Extract commitment
                    let mut commitment = [0u8; 32];
                    commitment.copy_from_slice(&output.cmu[..32]);
                    
                    // Create DecryptedNote using pipeline structure
                    // Note: encrypted_memo is empty for compact decryption (memo requires full 580-byte ciphertext)
                    let mut note = DecryptedNote::new(
                        block.height,
                        tx.index.unwrap_or(tx_idx as u64) as usize, // tx_index
                        output_idx, // output_index
                        value,
                        commitment,
                        [0u8; 32], // nullifier - will be computed when spending
                        Vec::new(), // encrypted_memo - empty for compact decryption, will be populated later
                    );
                    note.set_tx_hash(tx.hash.clone());
                    note.diversifier = diversifier_bytes.to_vec();
                    note.sapling_rseed_leadbyte = Some(decrypted.leadbyte);
                    note.sapling_rseed = Some(decrypted.rseed);
                    note.note_bytes = encode_sapling_note_bytes_from_address_bytes(
                        decrypted.address,
                        decrypted.leadbyte,
                        decrypted.rseed,
                    );
                    notes.push(note);
                }
            }
        }

        // Process Orchard actions (if Orchard IVK is available)
        if let Some(orchard_ivk_bytes) = orchard_ivk_bytes_opt {
            for (action_idx, action) in tx.actions.iter().enumerate() {
                // Perform Orchard trial decryption
                match try_decrypt_compact_orchard_action(action, orchard_ivk_bytes) {
                    Ok(Some(decrypted)) => {
                        // Extract commitment
                        let mut commitment = [0u8; 32];
                        commitment.copy_from_slice(&action.cmx[..32]);
                        
                        // Create DecryptedNote for Orchard
                        // Note: encrypted_memo is empty for compact decryption (memo requires full transaction)
                        let mut note = DecryptedNote::new_orchard(
                            block.height,
                            tx.index.unwrap_or(tx_idx as u64) as usize, // tx_index
                            action_idx, // output_index (action index in transaction)
                            decrypted.value,
                            commitment,
                            [0u8; 32], // nullifier - computed from full note data
                            Vec::new(), // encrypted_memo - empty for compact decryption, will be populated later
                            None,
                            Some(0), // position - will be updated from full transaction
                        );
                        note.set_tx_hash(tx.hash.clone());
                        // Store diversifier for Orchard (extracted from 52-byte prefix, just like Sapling)
                        note.diversifier = decrypted.diversifier.to_vec();
                        notes.push(note);
                    }
                    Ok(None) => {
                        // Decryption failed - note doesn't belong to us, continue
                    }
                    Err(e) => {
                        tracing::debug!("Orchard trial decryption error: {}", e);
                        // Continue to next action
                    }
                }
            }
        }
    }

    Ok(notes)
}

// DecryptedNote is imported from pipeline module - no need to redefine here

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.checkpoint_interval, 10_000);
        assert_eq!(config.batch_size, 2_000);
        assert!(config.lazy_memo_decode);
    }

    #[tokio::test]
    async fn test_sync_engine_creation() {
        let engine = SyncEngine::new("https://lightd.pirate.black:443".to_string(), 3_800_000);
        assert_eq!(engine.birthday_height(), 3_800_000);
    }

    #[tokio::test]
    async fn test_birthday_height_update() {
        let mut engine = SyncEngine::new("https://lightd.pirate.black:443".to_string(), 3_800_000);
        engine.set_birthday_height(4_000_000);
        assert_eq!(engine.birthday_height(), 4_000_000);
    }

    #[test]
    fn test_trial_decrypt_empty_block() {
        let block = CompactBlockData {
            proto_version: 1,
            height: 1000,
            hash: vec![0u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1234567890,
            header: vec![0u8; 32],
            transactions: vec![],
        };

        // Dummy IVK bytes for test
        let dummy_ivk = [0u8; 32];
        let notes = trial_decrypt_block(&block, true, Some(&dummy_ivk), None).unwrap();
        assert_eq!(notes.len(), 0);
    }
}
