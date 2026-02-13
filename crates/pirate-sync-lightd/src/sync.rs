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

use crate::block_cache::{acquire_inflight, BlockCache, InflightLease};
use crate::client::CompactBlockData;
use crate::frontier::SaplingFrontier;
use crate::orchard::full_decrypt::decrypt_orchard_memo_from_raw_tx_with_ivk_bytes;
use crate::orchard_frontier::OrchardFrontier;
use crate::pipeline::NoteType;
use crate::pipeline::{DecryptedNote, OrchardDecryptedNoteInit, PerfCounters};
use crate::progress::SyncStage;
use crate::sapling::full_decrypt::decrypt_memo_from_raw_tx_with_ivk_bytes;
use crate::{CancelToken, Error, LightClient, Result, SyncProgress};
use directories::ProjectDirs;
use group::ff::PrimeField;
use hex;
use orchard::keys::{
    Diversifier as OrchardDiversifier, IncomingViewingKey as OrchardIncomingViewingKey,
    PreparedIncomingViewingKey as OrchardPreparedIncomingViewingKey,
};
use orchard::note::{
    ExtractedNoteCommitment as OrchardExtractedNoteCommitment, Note as OrchardNote,
    Nullifier as OrchardNullifier, RandomSeed as OrchardRandomSeed,
};
use orchard::note_encryption::{CompactAction, OrchardDomain};
use orchard::tree::MerkleHashOrchard;
use orchard::value::NoteValue as OrchardNoteValue;
use orchard::Address as OrchardAddress;
use pirate_core::keys::{
    ExtendedFullViewingKey, ExtendedSpendingKey, OrchardExtendedFullViewingKey,
    OrchardExtendedSpendingKey, OrchardPaymentAddress as PirateOrchardPaymentAddress,
    PaymentAddress as PiratePaymentAddress,
};
use pirate_core::transaction::PirateNetwork;
use pirate_params::consensus::ConsensusParams;
use pirate_params::NetworkType;
use pirate_storage_sqlite::models::{AccountKey, AddressScope, KeyScope, KeyType};
use pirate_storage_sqlite::repository::OrchardNoteRef;
use pirate_storage_sqlite::security::MasterKey;
use pirate_storage_sqlite::{
    truncate_above_height, Database, EncryptionKey, FrontierStorage, NoteRecord, Repository,
    SyncStateStorage,
};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::CtOption;
use tokio::sync::RwLock;
use zcash_note_encryption::try_output_recovery_with_ovk;
use zcash_note_encryption::{
    batch as note_batch, EphemeralKeyBytes, ShieldedOutput, COMPACT_NOTE_SIZE,
};
use zcash_primitives::consensus::{BlockHeight, BranchId};
use zcash_primitives::merkle_tree::{read_frontier_v0, read_frontier_v1, write_merkle_path};
use zcash_primitives::sapling::keys::{
    OutgoingViewingKey as SaplingOutgoingViewingKey, PreparedIncomingViewingKey,
};
use zcash_primitives::sapling::note_encryption::{try_sapling_output_recovery, SaplingDomain};
use zcash_primitives::sapling::{PaymentAddress as SaplingPaymentAddress, Rseed, SaplingIvk};
use zcash_primitives::transaction::Transaction;

fn debug_log_path() -> PathBuf {
    let path = if let Ok(path) = env::var("PIRATE_DEBUG_LOG_PATH") {
        PathBuf::from(path)
    } else {
        ProjectDirs::from("com", "Pirate", "PirateWallet")
            .map(|dirs| dirs.data_local_dir().join("logs").join("debug.log"))
            .unwrap_or_else(|| {
                env::current_dir()
                    .map(|dir| dir.join(".cursor").join("debug.log"))
                    .unwrap_or_else(|_| PathBuf::from(".cursor").join("debug.log"))
            })
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    path
}

fn build_key_group_from_account_key(key: &AccountKey) -> Result<Option<WalletKeyGroup>> {
    let key_id = key.id.unwrap_or(0);

    let sapling_dfvk = if let Some(ref bytes) = key.sapling_dfvk {
        ExtendedFullViewingKey::from_bytes(bytes)
    } else if let Some(ref extsk_bytes) = key.sapling_extsk {
        let extsk = ExtendedSpendingKey::from_bytes(extsk_bytes)
            .map_err(|e| Error::Sync(format!("Invalid Sapling spending key bytes: {}", e)))?;
        Some(extsk.to_extended_fvk())
    } else {
        None
    };

    let orchard_fvk = if let Some(ref bytes) = key.orchard_fvk {
        OrchardExtendedFullViewingKey::from_bytes(bytes).ok()
    } else if let Some(ref extsk_bytes) = key.orchard_extsk {
        let extsk = OrchardExtendedSpendingKey::from_bytes(extsk_bytes)
            .map_err(|e| Error::Sync(format!("Invalid Orchard spending key bytes: {}", e)))?;
        Some(extsk.to_extended_fvk())
    } else {
        None
    };

    if sapling_dfvk.is_none() && orchard_fvk.is_none() {
        return Ok(None);
    }

    let sapling_ivk = sapling_dfvk
        .as_ref()
        .map(|dfvk| dfvk.to_ivk().to_sapling_ivk_bytes());
    let orchard_ivk = orchard_fvk.as_ref().map(|fvk| fvk.to_ivk_bytes());
    let sapling_ovk = sapling_dfvk
        .as_ref()
        .map(|dfvk| dfvk.outgoing_viewing_key());
    let orchard_ovk = orchard_fvk.as_ref().map(|fvk| fvk.to_ovk());

    Ok(Some(WalletKeyGroup {
        key_id,
        sapling_dfvk,
        orchard_fvk,
        sapling_ivk,
        orchard_ivk,
        sapling_ovk,
        orchard_ovk,
    }))
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
    /// Defer full transaction fetch/memo recovery to background
    pub defer_full_tx_fetch: bool,
    /// Target batch size in bytes (used to derive block count)
    pub target_batch_bytes: u64,
    /// Minimum batch size in bytes (during heavy/spam periods)
    pub min_batch_bytes: u64,
    /// Maximum batch size in bytes (cap for large batches)
    pub max_batch_bytes: u64,
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
const MIN_PARALLEL_OUTPUTS: usize = 256;

impl Default for SyncConfig {
    fn default() -> Self {
        let is_mobile = cfg!(target_os = "android") || cfg!(target_os = "ios");
        let (
            max_parallel_decrypt,
            max_batch_memory_bytes,
            target_batch_bytes,
            min_batch_bytes,
            max_batch_bytes,
        ) = if is_mobile {
            (8, Some(100_000_000), 8_000_000, 2_000_000, 16_000_000)
        } else {
            (32, Some(500_000_000), 32_000_000, 4_000_000, 64_000_000)
        };

        Self {
            checkpoint_interval: 10_000,
            batch_size: 2_000, // Match SimpleSync default (used when server recommendations disabled)
            min_batch_size: 100, // Minimum batch size for spam blocks
            max_batch_size: 2_000, // Maximum batch size (caps server batches to prevent OOM)
            use_server_batch_recommendations: true, // Use server's ~4MB chunk recommendations (typically ~199 blocks)
            mini_checkpoint_every: 5,               // Mini-checkpoint every 5 batches
            max_parallel_decrypt,
            lazy_memo_decode: true,
            defer_full_tx_fetch: true,
            target_batch_bytes,
            min_batch_bytes,
            max_batch_bytes,
            heavy_block_threshold_bytes: 500_000, // 500KB per block = heavy/spam (lowered for earlier detection)
            max_batch_memory_bytes,
        }
    }
}

/// Sync engine
pub struct SyncEngine {
    client: LightClient,
    progress: Arc<RwLock<SyncProgress>>,
    config: SyncConfig,
    birthday_height: u32,
    network_type: NetworkType,
    wallet_id: Option<String>,
    storage: Option<StorageSink>,
    keys: Vec<WalletKeyGroup>,
    nullifier_cache: HashMap<[u8; 32], i64>,
    nullifier_cache_loaded: bool,
    /// Sapling frontier for witness tree management
    frontier: Arc<RwLock<SaplingFrontier>>,
    /// Orchard frontier for witness tree management
    orchard_frontier: Arc<RwLock<OrchardFrontier>>,
    /// Performance counters
    perf: Arc<PerfCounters>,
    /// Parallel trial-decryption worker pool
    decrypt_pool: Arc<rayon::ThreadPool>,
    /// Cancellation token
    cancel: CancelToken,
    /// Background full-tx enrichment limiter
    enrich_semaphore: Arc<tokio::sync::Semaphore>,
}

#[allow(dead_code)]
fn _assert_sync_engine_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SyncEngine>();
}

struct PrefetchTask {
    start: u64,
    end: u64,
    handle: tokio::task::JoinHandle<Result<Vec<CompactBlockData>>>,
}

impl SyncEngine {
    /// Create new sync engine
    pub fn new(endpoint: String, birthday_height: u32) -> Self {
        let config = SyncConfig::default();
        let cpu_limit = num_cpus::get().max(1);
        let decrypt_threads = std::cmp::min(config.max_parallel_decrypt.max(1), cpu_limit);
        let decrypt_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(decrypt_threads)
            .thread_name(|i| format!("trial-decrypt-{}", i))
            .build()
            .expect("failed to build trial-decrypt thread pool");
        let enrich_limit = config.max_parallel_decrypt.clamp(1, 4);
        Self {
            client: LightClient::new(endpoint),
            progress: Arc::new(RwLock::new(SyncProgress::new())),
            config,
            birthday_height,
            network_type: NetworkType::Mainnet,
            wallet_id: None,
            storage: None,
            keys: Vec::new(),
            nullifier_cache: HashMap::new(),
            nullifier_cache_loaded: false,
            frontier: Arc::new(RwLock::new(SaplingFrontier::new())),
            orchard_frontier: Arc::new(RwLock::new(OrchardFrontier::new())),
            perf: Arc::new(PerfCounters::new()),
            decrypt_pool: Arc::new(decrypt_pool),
            cancel: CancelToken::new(),
            enrich_semaphore: Arc::new(tokio::sync::Semaphore::new(enrich_limit)),
        }
    }

    fn ensure_nullifier_cache(&mut self) -> Result<()> {
        if self.nullifier_cache_loaded {
            return Ok(());
        }
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };
        let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let repo = Repository::new(&db);
        let notes = repo.get_unspent_notes(sink.account_id)?;
        let mut loaded = 0u64;
        for note in notes {
            let id = match note.id {
                Some(v) => v,
                None => continue,
            };
            if note.nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&note.nullifier[..32]);
            self.nullifier_cache.insert(nf, id);
            loaded += 1;
        }
        self.nullifier_cache_loaded = true;
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:185","message":"nullifier_cache loaded","data":{{"count":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"N"}}"#,
                id, ts, loaded
            );
        }
        tracing::debug!("Loaded {} unspent nullifiers into cache", loaded);
        Ok(())
    }

    fn update_nullifier_cache(&mut self, entries: &[([u8; 32], i64)]) {
        for (nf, id) in entries {
            self.nullifier_cache.insert(*nf, *id);
        }
    }

    /// Create with custom configuration
    pub fn with_config(endpoint: String, birthday_height: u32, config: SyncConfig) -> Self {
        let cpu_limit = num_cpus::get().max(1);
        let decrypt_threads = std::cmp::min(config.max_parallel_decrypt.max(1), cpu_limit);
        let decrypt_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(decrypt_threads)
            .thread_name(|i| format!("trial-decrypt-{}", i))
            .build()
            .expect("failed to build trial-decrypt thread pool");
        let enrich_limit = config.max_parallel_decrypt.clamp(1, 4);
        Self {
            client: LightClient::new(endpoint),
            progress: Arc::new(RwLock::new(SyncProgress::new())),
            config,
            birthday_height,
            network_type: NetworkType::Mainnet,
            wallet_id: None,
            storage: None,
            keys: Vec::new(),
            nullifier_cache: HashMap::new(),
            nullifier_cache_loaded: false,
            frontier: Arc::new(RwLock::new(SaplingFrontier::new())),
            orchard_frontier: Arc::new(RwLock::new(OrchardFrontier::new())),
            perf: Arc::new(PerfCounters::new()),
            decrypt_pool: Arc::new(decrypt_pool),
            cancel: CancelToken::new(),
            enrich_semaphore: Arc::new(tokio::sync::Semaphore::new(enrich_limit)),
        }
    }

    /// Create with pre-configured client and custom sync config
    pub fn with_client_and_config(
        client: LightClient,
        birthday_height: u32,
        config: SyncConfig,
    ) -> Self {
        let cpu_limit = num_cpus::get().max(1);
        let decrypt_threads = std::cmp::min(config.max_parallel_decrypt.max(1), cpu_limit);
        let decrypt_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(decrypt_threads)
            .thread_name(|i| format!("trial-decrypt-{}", i))
            .build()
            .expect("failed to build trial-decrypt thread pool");
        let enrich_limit = config.max_parallel_decrypt.clamp(1, 4);
        Self {
            client,
            progress: Arc::new(RwLock::new(SyncProgress::new())),
            config,
            birthday_height,
            network_type: NetworkType::Mainnet,
            wallet_id: None,
            storage: None,
            keys: Vec::new(),
            nullifier_cache: HashMap::new(),
            nullifier_cache_loaded: false,
            frontier: Arc::new(RwLock::new(SaplingFrontier::new())),
            orchard_frontier: Arc::new(RwLock::new(OrchardFrontier::new())),
            perf: Arc::new(PerfCounters::new()),
            decrypt_pool: Arc::new(decrypt_pool),
            cancel: CancelToken::new(),
            enrich_semaphore: Arc::new(tokio::sync::Semaphore::new(enrich_limit)),
        }
    }

    /// Get performance counters reference
    pub fn perf_counters(&self) -> Arc<PerfCounters> {
        Arc::clone(&self.perf)
    }

    /// Cancel sync
    pub async fn cancel(&self) {
        self.cancel.cancel();
        tracing::info!("Sync cancellation requested");
    }

    /// Share cancellation flag without locking the engine.
    pub fn cancel_flag(&self) -> CancelToken {
        self.cancel.clone()
    }

    /// Check if cancelled
    async fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Attach wallet context and open encrypted storage (shared DB with FFI)
    pub fn with_wallet(
        mut self,
        wallet_id: String,
        key: EncryptionKey,
        master_key: MasterKey,
        network_type: NetworkType,
    ) -> Result<Self> {
        self.wallet_id = Some(wallet_id.clone());
        self.network_type = network_type;

        let db_path = wallet_db_path(&wallet_id)?;
        let db = Database::open(&db_path, &key, master_key.clone())?;
        let repo = Repository::new(&db);

        // Load wallet secret to know account id (if present)
        let secret = repo
            .get_wallet_secret(&wallet_id)?
            .ok_or_else(|| Error::Sync(format!("Wallet secret not found for {}", wallet_id)))?;

        let mut account_keys = repo.get_account_keys(secret.account_id)?;
        if account_keys.is_empty() {
            let sapling_dfvk_bytes = if let Some(ref bytes) = secret.dfvk {
                Some(bytes.clone())
            } else if !secret.extsk.is_empty() {
                let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
                    .map_err(|e| Error::Sync(format!("Invalid spending key bytes: {}", e)))?;
                Some(extsk.to_extended_fvk().to_bytes())
            } else {
                None
            };

            let orchard_fvk_bytes = if let Some(ref extsk_bytes) = secret.orchard_extsk {
                let extsk = OrchardExtendedSpendingKey::from_bytes(extsk_bytes).map_err(|e| {
                    Error::Sync(format!("Invalid Orchard spending key bytes: {}", e))
                })?;
                Some(extsk.to_extended_fvk().to_bytes())
            } else {
                secret
                    .orchard_ivk
                    .as_ref()
                    .filter(|b| b.len() == 137)
                    .cloned()
            };

            let fallback_key = AccountKey {
                id: None,
                account_id: secret.account_id,
                key_type: if secret.extsk.is_empty() {
                    KeyType::ImportView
                } else {
                    KeyType::Seed
                },
                key_scope: KeyScope::Account,
                label: None,
                birthday_height: 0,
                created_at: chrono::Utc::now().timestamp(),
                spendable: !secret.extsk.is_empty(),
                sapling_extsk: if secret.extsk.is_empty() {
                    None
                } else {
                    Some(secret.extsk.clone())
                },
                sapling_dfvk: sapling_dfvk_bytes,
                orchard_extsk: secret.orchard_extsk.clone(),
                orchard_fvk: orchard_fvk_bytes,
                encrypted_mnemonic: secret.encrypted_mnemonic.clone(),
            };
            let encrypted_key = repo.encrypt_account_key_fields(&fallback_key)?;
            let _ = repo.upsert_account_key(&encrypted_key)?;
            account_keys = repo.get_account_keys(secret.account_id)?;
        }

        let mut key_groups = Vec::new();
        for key in &account_keys {
            if let Some(group) = build_key_group_from_account_key(key)? {
                key_groups.push(group);
            }
        }

        let sink = StorageSink {
            db_path,
            key,
            master_key,
            account_id: secret.account_id,
            network_type: self.network_type,
        };
        self.storage = Some(sink);
        self.keys = key_groups;
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
                if let Some(snapshot_height) =
                    self.restore_frontiers_from_storage(stored_height).await?
                {
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
                                id, ts, stored_height, snapshot_height, start_height, rebuilt
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
    pub fn total_balance_at_height(
        &self,
        current_height: u64,
        min_depth: u64,
    ) -> Result<Option<u64>> {
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
                let h = t.height;
                h > from_height as i64 && h <= current_height as i64
            })
            .count() as u32;
        Ok(Some(count))
    }

    /// Sync specific range
    pub async fn sync_range(&mut self, start_height: u64, end_height: Option<u64>) -> Result<()> {
        tracing::info!(
            "sync_range called: start={}, end_height={:?}",
            start_height,
            end_height
        );

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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:275","message":"sync_range entry","data":{{"start":{},"end_height":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                id, ts, start_height, end_height
            );
        }
        // #endregion

        // Connect to lightwalletd
        tracing::debug!("Connecting to lightwalletd...");
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:280","message":"connect attempt","data":{{}},"sessionId":"debug-session","runId":"run1","hypothesisId":"A"}}"#,
                id, ts
            );
        }
        // #endregion
        let connect_result = self.client.connect().await;
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:283","message":"connect result","data":{{"success":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"A"}}"#,
                id,
                ts,
                connect_result.is_ok(),
                connect_result.as_ref().err()
            );
        }
        // #endregion
        connect_result.map_err(|e| {
            tracing::error!("Failed to connect to lightwalletd: {:?}", e);
            e
        })?;
        tracing::debug!("Connected to lightwalletd");

        let follow_tip = end_height.is_none();

        // Get latest block if end not specified
        let end = match end_height {
            Some(h) => {
                tracing::debug!("Using provided end height: {}", h);
                h
            }
            None => {
                tracing::debug!("Fetching latest block from server...");
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:294","message":"get_latest_block call","data":{{}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                        id, ts
                    );
                }
                // #endregion
                let latest_result = self.client.get_latest_block().await;
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:297","message":"get_latest_block result in sync","data":{{"success":{},"height":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                        id,
                        ts,
                        latest_result.is_ok(),
                        latest_result.as_ref().ok().copied().unwrap_or(0),
                        latest_result.as_ref().err()
                    );
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
            tracing::warn!(
                "Skipping sync: local height {} is ahead of server tip {}",
                start_height,
                end
            );
            {
                let progress = self.progress.write().await;
                progress.set_target(end);
                progress.set_current(end);
                progress.set_stage(SyncStage::Complete);
                progress.start();
            }
            return Ok(());
        }

        self.ensure_nullifier_cache()?;

        // Initialize progress
        {
            let progress = self.progress.write().await;
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:332","message":"sync_range_internal entry","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                id,
                ts,
                start_height,
                end,
                end - start_height + 1
            );
        }
        // #endregion
        let result = self
            .sync_range_internal(start_height, end, follow_tip)
            .await;
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:333","message":"sync_range_internal result","data":{{"success":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                id,
                ts,
                result.is_ok(),
                result.as_ref().err()
            );
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

    /// Reset in-memory frontiers so a bounded replay can rebuild witness state
    /// from authoritative tree state at `start_height - 1`.
    pub async fn reset_frontiers_for_replay(&mut self, start_height: u64) -> Result<()> {
        *self.frontier.write().await = SaplingFrontier::new();
        *self.orchard_frontier.write().await = OrchardFrontier::new();
        tracing::warn!(
            "Frontiers reset for bounded replay start_height={}",
            start_height
        );

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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:338","message":"frontiers reset for replay","data":{{"start_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                id, ts, start_height
            );
        }

        Ok(())
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
                id, ts, tree_height
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
        ) -> Result<
            bridgetree::Frontier<H, { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH }>,
        >
        where
            H: bridgetree::Hashable + zcash_primitives::merkle_tree::HashSer + Clone,
        {
            let bytes = hex::decode(hex_str)
                .map_err(|e| Error::Sync(format!("Failed to decode {} bytes: {}", label, e)))?;

            if let Ok(frontier) = read_frontier_v1::<H, _>(&bytes[..]) {
                return Ok(frontier);
            }

            read_frontier_v0::<H, _>(&bytes[..])
                .map_err(|e| Error::Sync(format!("Failed to parse {} frontier: {}", label, e)))
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

    async fn fetch_tree_state_with_retry(
        &self,
        tree_height: u64,
    ) -> Result<crate::client::TreeState> {
        let max_attempts = 3u32;
        let timeout = Duration::from_secs(120);
        let mut attempt = 0u32;

        loop {
            attempt += 1;
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
                    r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:tree_state_attempt","message":"tree state attempt","data":{{"tree_height":{},"attempt":{},"timeout_secs":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                    id,
                    ts,
                    tree_height,
                    attempt,
                    timeout.as_secs()
                );
            }
            // #endregion

            let bridge_future =
                tokio::time::timeout(timeout, self.client.get_bridge_tree_state(tree_height));
            let legacy_future =
                tokio::time::timeout(timeout, self.client.get_tree_state(tree_height));
            tokio::pin!(bridge_future);
            tokio::pin!(legacy_future);

            let (bridge_err, legacy_err) = tokio::select! {
                result = &mut bridge_future => {
                    let bridge_err = match result {
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
                            Some(format!("{:?}", e))
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
                            Some("timeout".to_string())
                        }
                    };

                    let legacy_err = match legacy_future.await {
                        Ok(Ok(state)) => return Ok(state),
                        Ok(Err(e)) => Some(format!("{:?}", e)),
                        Err(_) => Some("timeout".to_string()),
                    };
                    (bridge_err, legacy_err)
                }
                result = &mut legacy_future => {
                    let legacy_err = match result {
                        Ok(Ok(state)) => return Ok(state),
                        Ok(Err(e)) => Some(format!("{:?}", e)),
                        Err(_) => Some("timeout".to_string()),
                    };

                    let bridge_err = match bridge_future.await {
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
                            Some(format!("{:?}", e))
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
                            Some("timeout".to_string())
                        }
                    };
                    (bridge_err, legacy_err)
                }
            };

            if attempt >= max_attempts {
                return Err(Error::Sync(format!(
                    "Tree state fetch failed at {} after {} attempts (bridge: {}, legacy: {})",
                    tree_height,
                    attempt,
                    bridge_err.unwrap_or_else(|| "unknown".to_string()),
                    legacy_err.unwrap_or_else(|| "unknown".to_string())
                )));
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn sync_range_internal(
        &mut self,
        start: u64,
        mut end: u64,
        follow_tip: bool,
    ) -> Result<()> {
        let mut current_height = start;
        let mut last_checkpoint_height = start.saturating_sub(1);
        let mut last_major_checkpoint_height = start.saturating_sub(1);
        let mut batches_since_mini_checkpoint = 0u32;

        // Adaptive batch sizing for spam blocks (byte-based targets)
        let mut current_target_bytes = self.config.target_batch_bytes;
        let mut consecutive_heavy_batches = 0u32;
        let mut avg_block_size_estimate =
            (self.config.target_batch_bytes / self.config.batch_size.max(1)).max(1);
        let mut pending_fetch: Option<PrefetchTask> = None;

        // Reset perf counters
        self.perf.reset();

        // Reset cancellation token.
        self.cancel.reset();

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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:361","message":"sync loop start","data":{{"current":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                id, ts, current_height, end
            );
        }
        // #endregion

        // Outer loop: Keep syncing until we're fully caught up with no new blocks
        loop {
            // Main sync loop: sync from current_height to end
            while current_height <= end {
                // Check for cancellation
                if self.is_cancelled().await {
                    tracing::warn!("Sync cancelled at height {}", current_height);
                    return Err(Error::Cancelled);
                }

                let batch_start_time = Instant::now();
                let mut persist_ms: u128 = 0;
                let mut apply_spends_ms: u128 = 0;

                if pending_fetch.is_none() {
                    let (batch_end, _desired_blocks) = self
                        .compute_batch_end(
                            current_height,
                            end,
                            current_target_bytes,
                            avg_block_size_estimate,
                        )
                        .await?;
                    pending_fetch = Some(self.spawn_prefetch(current_height, batch_end));
                }

                let PrefetchTask {
                    start: batch_start,
                    end: batch_end,
                    handle,
                } = pending_fetch.take().unwrap();

                // Stage 1: Fetch blocks (with retry logic)
                self.progress.write().await.set_stage(SyncStage::Headers);
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:505","message":"fetch_blocks_with_retry start","data":{{"current_height":{},"batch_end":{},"batch_size":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        id,
                        ts,
                        batch_start,
                        batch_end,
                        batch_end - batch_start + 1
                    );
                }
                // #endregion

                let (blocks, fetch_wait_ms) = {
                    let mut backoff = Duration::from_secs(2);
                    let max_backoff = Duration::from_secs(60);
                    let mut prefetch_handle = Some(handle);

                    loop {
                        let (blocks_res, wait_ms): (Result<Vec<CompactBlockData>>, u128) =
                            if let Some(handle) = prefetch_handle.take() {
                                let fetch_wait_start = Instant::now();
                                let mut handle = handle;
                                let res = tokio::select! {
                                    joined = &mut handle => match joined {
                                        Ok(inner) => inner,
                                        Err(e) => Err(Error::Sync(e.to_string())),
                                    },
                                    _ = self.cancel.cancelled() => {
                                        handle.abort();
                                        return Err(Error::Cancelled);
                                    }
                                };
                                let wait_ms = fetch_wait_start.elapsed().as_millis();
                                (res, wait_ms)
                            } else {
                                let fetch_wait_start = Instant::now();
                                let res = SyncEngine::fetch_blocks_with_retry_inner(
                                    self.client.clone(),
                                    batch_start,
                                    batch_end,
                                    self.cancel.clone(),
                                )
                                .await;
                                let wait_ms = fetch_wait_start.elapsed().as_millis();
                                (res, wait_ms)
                            };

                        match blocks_res {
                            Ok(blocks) => break (blocks, wait_ms),
                            Err(Error::Cancelled) => return Err(Error::Cancelled),
                            Err(e) => {
                                tracing::warn!(
                                    "Block fetch failed for {}-{}: {}. Reconnecting and retrying in {:?}...",
                                    batch_start,
                                    batch_end,
                                    e,
                                    backoff
                                );
                                self.client.disconnect().await;
                                if let Err(conn_err) = self.client.connect().await {
                                    tracing::warn!("Reconnect failed: {}", conn_err);
                                }
                                tokio::select! {
                                    _ = tokio::time::sleep(backoff) => {},
                                    _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                                }
                                backoff = std::cmp::min(backoff.saturating_mul(2), max_backoff);
                                // Retry using a direct fetch (no prefetch).
                                continue;
                            }
                        }
                    }
                };

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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:506","message":"fetch_blocks_with_retry result","data":{{"current_height":{},"batch_end":{},"blocks_count":{},"wait_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        id,
                        ts,
                        batch_start,
                        batch_end,
                        blocks.len(),
                        fetch_wait_ms
                    );
                }
                // #endregion

                if blocks.is_empty() {
                    tracing::warn!("Empty block batch at {}-{}", batch_start, batch_end);
                    current_height = batch_end + 1;
                    continue;
                }

                // Detect heavy/spam blocks and adapt batch size
                // Count actual bytes in outputs and actions
                let total_block_size: u64 = blocks
                    .iter()
                    .map(|b| {
                        // Count actual bytes in Sapling outputs
                        let sapling_bytes: u64 = b
                            .transactions
                            .iter()
                            .map(|tx| {
                                tx.outputs
                                    .iter()
                                    .map(|out| {
                                        // Each Sapling output: cmu (32) + ephemeral_key (32) + ciphertext
                                        // Compact ciphertext is 52 bytes minimum
                                        32 + 32 + out.ciphertext.len().max(52) as u64
                                    })
                                    .sum::<u64>()
                            })
                            .sum();

                        // Count actual bytes in Orchard actions
                        let orchard_bytes: u64 = b
                            .transactions
                            .iter()
                            .map(|tx| {
                                tx.actions
                                    .iter()
                                    .map(|action| {
                                        // Each Orchard action: nullifier (32) + cmx (32) + ephemeral_key (32) +
                                        // enc_ciphertext (52+ minimum) + out_ciphertext (52+ minimum)
                                        32 + 32
                                            + 32
                                            + action.enc_ciphertext.len().max(52) as u64
                                            + action.out_ciphertext.len().max(52) as u64
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
                avg_block_size_estimate = avg_block_size.max(1);
                let is_heavy_batch = avg_block_size > self.config.heavy_block_threshold_bytes;

                if is_heavy_batch {
                    consecutive_heavy_batches += 1;
                    // Reduce target bytes significantly for spam blocks.
                    current_target_bytes =
                        std::cmp::max(self.config.min_batch_bytes, current_target_bytes / 4);
                    tracing::warn!(
                    "Heavy block detected at height {} (avg {} bytes/block), reducing target bytes to {} (consecutive: {})",
                    current_height,
                    avg_block_size,
                    current_target_bytes,
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
                            "Emergency checkpoint at {} due to spam blocks (target bytes: {})",
                            batch_end,
                            current_target_bytes
                        );
                    }
                } else {
                    // Reset counter and gradually increase batch size back to normal
                    consecutive_heavy_batches = 0;
                    if current_target_bytes < self.config.target_batch_bytes {
                        let bump = std::cmp::max(1, self.config.target_batch_bytes / 4);
                        current_target_bytes = std::cmp::min(
                            self.config.target_batch_bytes,
                            current_target_bytes + bump,
                        );
                        tracing::debug!(
                            "Normal blocks detected, increasing target bytes to {}",
                            current_target_bytes
                        );
                    }
                }

                // Prefetch next batch while we process this one.
                let next_start = batch_end + 1;
                if next_start <= end {
                    let (next_end, _desired_blocks) = self
                        .compute_batch_end(
                            next_start,
                            end,
                            current_target_bytes,
                            avg_block_size_estimate,
                        )
                        .await?;
                    pending_fetch = Some(self.spawn_prefetch(next_start, next_end));
                }

                // Stage 2: Trial decryption (batched with parallelism)
                self.progress.write().await.set_stage(SyncStage::Notes);
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:846","message":"trial_decrypt start","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        current_height,
                        batch_end,
                        blocks.len()
                    );
                }
                // #endregion
                let decrypt_start = Instant::now();
                let mut notes = self.trial_decrypt_batch(&blocks).await?;
                let decrypt_ms = decrypt_start.elapsed().as_millis();
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:852","message":"trial_decrypt done","data":{{"start":{},"end":{},"notes":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        current_height,
                        batch_end,
                        notes.len(),
                        decrypt_ms
                    );
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:862","message":"update_frontier start","data":{{"start":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id, ts, current_height, batch_end
                    );
                }
                // #endregion
                let frontier_start = Instant::now();
                let (commitments_applied, position_mappings) =
                    self.update_frontier(&blocks, &notes).await?;
                let frontier_ms = frontier_start.elapsed().as_millis();
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:866","message":"update_frontier done","data":{{"start":{},"end":{},"commitments":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id, ts, current_height, batch_end, commitments_applied, frontier_ms
                    );
                }
                // #endregion
                self.apply_positions(&mut notes, &position_mappings).await;
                self.apply_sapling_nullifiers(&mut notes, &position_mappings)
                    .await?;

                let require_memos = !self.config.lazy_memo_decode;
                if !notes.is_empty() && !self.config.defer_full_tx_fetch {
                    self.fetch_and_enrich_notes(&mut notes, require_memos)
                        .await?;
                }

                if !notes.is_empty() {
                    let max_money = ConsensusParams::mainnet().max_money;
                    let require_orchard_nullifier =
                        self.keys.iter().any(|keys| keys.orchard_fvk.is_some());
                    let mut filtered_value = 0usize;
                    let mut filtered_nullifier = 0usize;
                    notes.retain(|note| {
                        if note.value == 0 || note.value > max_money {
                            filtered_value += 1;
                            return false;
                        }
                        if require_orchard_nullifier
                            && note.note_type == NoteType::Orchard
                            && note.nullifier.iter().all(|b| *b == 0)
                        {
                            filtered_nullifier += 1;
                            return false;
                        }
                        true
                    });

                    let filtered = filtered_value + filtered_nullifier;
                    if filtered > 0 {
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
                                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:878","message":"filtered invalid notes","data":{{"filtered":{},"filtered_value":{},"filtered_nullifier":{},"remaining":{},"require_orchard_nullifier":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                id,
                                ts,
                                filtered,
                                filtered_value,
                                filtered_nullifier,
                                notes.len(),
                                require_orchard_nullifier
                            );
                        }
                    }
                }

                // Persist decrypted notes if storage is configured (after frontier update to get positions)
                if let Some(ref sink) = self.storage {
                    let persist_start = Instant::now();
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:881","message":"persist_notes start","data":{{"start":{},"end":{},"notes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                            id,
                            ts,
                            current_height,
                            batch_end,
                            notes.len()
                        );
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

                    let inserted =
                        sink.persist_notes(&notes, &tx_times, &tx_fees, &position_mappings)?;
                    if !inserted.is_empty() {
                        self.update_nullifier_cache(&inserted);
                    }
                    persist_ms = persist_start.elapsed().as_millis();
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:900","message":"persist_notes done","data":{{"start":{},"end":{},"notes":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                            id,
                            ts,
                            current_height,
                            batch_end,
                            notes.len(),
                            persist_ms
                        );
                    }
                    // #endregion
                }

                if !blocks.is_empty() {
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:906","message":"apply_spends start","data":{{"start":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                            id, ts, current_height, batch_end
                        );
                    }
                    // #endregion
                    let apply_start = Instant::now();
                    self.apply_spends(&blocks).await?;
                    apply_spends_ms = apply_start.elapsed().as_millis();
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:909","message":"apply_spends done","data":{{"start":{},"end":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                            id, ts, current_height, batch_end, apply_spends_ms
                        );
                    }
                    // #endregion
                }

                if self.config.defer_full_tx_fetch && !notes.is_empty() {
                    self.spawn_background_enrich(notes.clone(), require_memos);
                }

                // Record batch performance
                let batch_duration = batch_start_time.elapsed();
                self.perf.record_batch(
                    blocks.len() as u64,
                    notes.len() as u64,
                    commitments_applied,
                    batch_duration.as_millis() as u64,
                );

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
                    let avg_block_size = total_block_size / blocks.len().max(1) as u64;
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:915","message":"batch_stage_timing","data":{{"start":{},"end":{},"blocks":{},"notes":{},"total_bytes":{},"avg_block_bytes":{},"fetch_wait_ms":{},"decrypt_ms":{},"frontier_ms":{},"persist_ms":{},"apply_spends_ms":{},"batch_total_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        current_height,
                        batch_end,
                        blocks.len(),
                        notes.len(),
                        total_block_size,
                        avg_block_size,
                        fetch_wait_ms,
                        decrypt_ms,
                        frontier_ms,
                        persist_ms,
                        apply_spends_ms,
                        batch_duration.as_millis()
                    );
                }
                // #endregion

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
                    let progress = self.progress.read().await;
                    let wallet_id = self.wallet_id.as_deref().unwrap_or("unknown");
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:664","message":"progress updated","data":{{"current_height":{},"target_height":{},"percent":{:.2},"stage":"{:?}","wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"F"}}"#,
                        id,
                        ts,
                        progress.current_height(),
                        progress.target_height(),
                        progress.percentage(),
                        progress.stage(),
                        wallet_id
                    );
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
                self.save_sync_state(batch_end, end, last_checkpoint_height)
                    .await?;

                current_height = batch_end + 1;
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:709","message":"current_height updated","data":{{"new_current_height":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        id, ts, current_height, end
                    );
                }
                // #endregion
            }

            // For bounded ranges (e.g. witness repair replay), stop once we reach the
            // requested end height instead of entering continuous tip monitoring.
            if !follow_tip {
                return Ok(());
            }

            // After main sync loop completes, check if there are more blocks to sync
            // This handles the case where sync completed the initial range but blockchain moved forward
            // Keep checking and syncing until we're fully caught up, then keep monitoring for new blocks
            let current = {
                let progress = self.progress.read().await;
                progress.current_height()
            };

            if self.is_cancelled().await {
                tracing::warn!("Sync cancelled while monitoring at height {}", current);
                return Err(Error::Cancelled);
            }

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
                            let progress = self.progress.write().await;
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
                        tracing::debug!(
                            "Caught up to blockchain tip ({}), waiting for new blocks...",
                            current
                        );
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                            _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                        }
                        // Continue the outer loop to check again
                        continue;
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to check for new blocks after sync: {}, reconnecting and retrying in 30s",
                        e
                    );
                    self.client.disconnect().await;
                    if let Err(conn_err) = self.client.connect().await {
                        tracing::warn!("Reconnect failed: {}", conn_err);
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                        _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                    }
                    continue; // Retry
                }
            }
        }
    }

    async fn fetch_blocks_with_retry_inner(
        client: LightClient,
        start: u64,
        end: u64,
        cancel: CancelToken,
    ) -> Result<Vec<CompactBlockData>> {
        if start > end {
            return Ok(Vec::new());
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let expected_blocks = end.saturating_sub(start).saturating_add(1) as usize;

        if let Ok(cache) = BlockCache::for_endpoint(client.endpoint()) {
            match cache.load_range(start, end) {
                Ok(blocks) if blocks.len() == expected_blocks => {
                    tracing::debug!(
                        "Block cache hit for {}-{} ({} blocks)",
                        start,
                        end,
                        expected_blocks
                    );
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache hit","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                            id,
                            ts,
                            start,
                            end,
                            blocks.len()
                        );
                    }
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache partial","data":{{"start":{},"end":{},"blocks":{},"expected":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                            id,
                            ts,
                            start,
                            end,
                            blocks.len(),
                            expected_blocks
                        );
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("Block cache read failed for {}-{}: {}", start, end, e);
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache read error","data":{{"start":{},"end":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                            id, ts, start, end, e
                        );
                    }
                }
            }
        }

        loop {
            let inflight = acquire_inflight(client.endpoint(), start, end);

            match inflight {
                InflightLease::Follower(notify) => {
                    tokio::select! {
                        _ = notify.notified() => {}
                        _ = cancel.cancelled() => return Err(Error::Cancelled),
                    }
                    if let Ok(cache) = BlockCache::for_endpoint(client.endpoint()) {
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
                        let fetch = tokio::select! {
                            res = client.get_compact_block_range(start as u32..(end + 1) as u32) => res,
                            _ = cancel.cancelled() => Err(Error::Cancelled),
                        };

                        match fetch {
                            Ok(blocks) => {
                                if let Ok(cache) = BlockCache::for_endpoint(client.endpoint()) {
                                    if let Err(e) = cache.store_blocks(&blocks) {
                                        tracing::debug!(
                                            "Block cache store failed for {}-{}: {}",
                                            start,
                                            end,
                                            e
                                        );
                                    } else if let Ok(mut file) = std::fs::OpenOptions::new()
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
                                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache store","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                                            id,
                                            ts,
                                            start,
                                            end,
                                            blocks.len()
                                        );
                                    }
                                }
                                break Ok(blocks);
                            }
                            Err(e) if matches!(e, Error::Cancelled) => break Err(e),
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
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_millis(backoff)) => {}
                                    _ = cancel.cancelled() => break Err(Error::Cancelled),
                                }
                            }
                            Err(e) => break Err(e),
                        }
                    };
                    token.complete();
                    return result;
                }
            }
        }
    }

    async fn trial_decrypt_batch(&self, blocks: &[CompactBlockData]) -> Result<Vec<DecryptedNote>> {
        // Build IVK bundles from all key groups.
        let mut sapling_ivks = Vec::new();
        let mut sapling_key_ids = Vec::new();
        let mut sapling_scopes = Vec::new();
        let mut orchard_ivks = Vec::new();
        let mut orchard_key_ids = Vec::new();
        let mut orchard_scopes = Vec::new();
        let mut orchard_fvks = Vec::new();

        for key in &self.keys {
            if let Some(ivk_bytes) = key.sapling_ivk {
                if let Some(ivk_fr) = Option::from(jubjub::Fr::from_bytes(&ivk_bytes)) {
                    let sapling_ivk = SaplingIvk(ivk_fr);
                    sapling_ivks.push(PreparedIncomingViewingKey::new(&sapling_ivk));
                    sapling_key_ids.push(key.key_id);
                    sapling_scopes.push(AddressScope::External);
                }
            }

            if let Some(dfvk) = key.sapling_dfvk.as_ref() {
                let internal_ivk_bytes = dfvk.to_internal_ivk_bytes();
                if let Some(ivk_fr) = Option::from(jubjub::Fr::from_bytes(&internal_ivk_bytes)) {
                    let sapling_ivk = SaplingIvk(ivk_fr);
                    sapling_ivks.push(PreparedIncomingViewingKey::new(&sapling_ivk));
                    sapling_key_ids.push(key.key_id);
                    sapling_scopes.push(AddressScope::Internal);
                }
            }

            if let (Some(ivk_bytes), Some(fvk)) = (key.orchard_ivk, key.orchard_fvk.as_ref()) {
                let ivk_ct = OrchardIncomingViewingKey::from_bytes(&ivk_bytes);
                if bool::from(ivk_ct.is_some()) {
                    let ivk = ivk_ct.unwrap();
                    orchard_ivks.push(OrchardPreparedIncomingViewingKey::new(&ivk));
                    orchard_key_ids.push(key.key_id);
                    orchard_scopes.push(AddressScope::External);
                    orchard_fvks.push(fvk.inner.clone());
                }
            }

            if let Some(fvk) = key.orchard_fvk.as_ref() {
                let internal_ivk_bytes = fvk.to_internal_ivk_bytes();
                let ivk_ct = OrchardIncomingViewingKey::from_bytes(&internal_ivk_bytes);
                if bool::from(ivk_ct.is_some()) {
                    let ivk = ivk_ct.unwrap();
                    orchard_ivks.push(OrchardPreparedIncomingViewingKey::new(&ivk));
                    orchard_key_ids.push(key.key_id);
                    orchard_scopes.push(AddressScope::Internal);
                    orchard_fvks.push(fvk.inner.clone());
                }
            }
        }

        let has_sapling_ivk = !sapling_ivks.is_empty();
        let has_orchard_ivk = !orchard_ivks.is_empty();
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:trial_decrypt_batch","message":"trial_decrypt ivk availability","data":{{"sapling":{},"orchard":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id, ts, has_sapling_ivk, has_orchard_ivk
            );
        }

        if !has_sapling_ivk && !has_orchard_ivk {
            tracing::warn!("No Sapling or Orchard IVK available for trial decryption");
            return Ok(Vec::new());
        }

        let mut orchard_actions_total = 0usize;
        let mut sapling_outputs_total = 0usize;
        let mut min_height: Option<u64> = None;
        let mut max_height: u64 = 0;
        for block in blocks {
            let height = block.height;
            min_height = Some(min_height.map_or(height, |current| current.min(height)));
            max_height = max_height.max(height);
            for tx in &block.transactions {
                orchard_actions_total += tx.actions.len();
                sapling_outputs_total += tx.outputs.len();
            }
        }

        let all_notes = trial_decrypt_batch_impl(TrialDecryptBatchInputs {
            blocks,
            sapling_ivks: &sapling_ivks,
            sapling_key_ids: &sapling_key_ids,
            sapling_scopes: &sapling_scopes,
            orchard_ivks: &orchard_ivks,
            orchard_key_ids: &orchard_key_ids,
            orchard_scopes: &orchard_scopes,
            orchard_fvks: &orchard_fvks,
            decrypt_pool: self.decrypt_pool.as_ref(),
            max_parallel: self.config.max_parallel_decrypt,
        })?;

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
            let orchard_notes = all_notes
                .iter()
                .filter(|note| note.note_type == crate::pipeline::NoteType::Orchard)
                .count();
            let sapling_notes = all_notes
                .iter()
                .filter(|note| note.note_type == crate::pipeline::NoteType::Sapling)
                .count();
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:trial_decrypt_batch","message":"trial_decrypt batch summary","data":{{"start":{},"end":{},"blocks":{},"sapling_outputs":{},"orchard_actions":{},"sapling_notes":{},"orchard_notes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                min_height.unwrap_or(0),
                max_height,
                blocks.len(),
                sapling_outputs_total,
                orchard_actions_total,
                sapling_notes,
                orchard_notes
            );
        }

        Ok(all_notes)
    }

    fn spawn_prefetch(&self, start: u64, end: u64) -> PrefetchTask {
        let client = self.client.clone();
        let cancel = self.cancel.clone();
        let handle = tokio::spawn(async move {
            SyncEngine::fetch_blocks_with_retry_inner(client, start, end, cancel).await
        });
        PrefetchTask { start, end, handle }
    }

    async fn compute_batch_end(
        &self,
        current_height: u64,
        end: u64,
        current_target_bytes: u64,
        avg_block_size_estimate: u64,
    ) -> Result<(u64, u64)> {
        let mut target_bytes =
            current_target_bytes.clamp(self.config.min_batch_bytes, self.config.max_batch_bytes);
        if let Some(max_memory) = self.config.max_batch_memory_bytes {
            target_bytes = target_bytes.min(max_memory);
        }

        let estimated_block_bytes = avg_block_size_estimate.max(1);
        let mut desired_blocks = target_bytes / estimated_block_bytes;
        if desired_blocks == 0 {
            desired_blocks = 1;
        }
        desired_blocks =
            desired_blocks.clamp(self.config.min_batch_size, self.config.max_batch_size);

        let desired_end = std::cmp::min(current_height + desired_blocks - 1, end);

        if !self.config.use_server_batch_recommendations {
            return Ok((desired_end, desired_blocks));
        }

        match self
            .client
            .get_lite_wallet_block_group(current_height)
            .await
        {
            Ok(server_end) => {
                let optimal_end = std::cmp::min(server_end, end);
                let server_batch_size =
                    optimal_end.saturating_sub(current_height).saturating_add(1);
                let max_capped_end =
                    std::cmp::min(optimal_end, current_height + self.config.max_batch_size - 1);
                let batch_end = std::cmp::max(desired_end, max_capped_end);

                if max_capped_end > current_height {
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:compute_batch_end","message":"server batch recommendation","data":{{"server_batch_size":{},"desired_blocks":{},"max_batch_size":{},"chosen_blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"G"}}"#,
                            id,
                            ts,
                            server_batch_size,
                            desired_blocks,
                            self.config.max_batch_size,
                            batch_end - current_height + 1
                        );
                    }
                    // #endregion
                    tracing::debug!(
                        "Batch sizing: server {} blocks, desired {} blocks, chosen {} blocks",
                        server_batch_size,
                        desired_blocks,
                        batch_end - current_height + 1
                    );
                }

                Ok((batch_end, desired_blocks))
            }
            Err(e) => {
                tracing::debug!(
                    "Server batch grouping unavailable ({}), using byte-based batch size: {} blocks",
                    e,
                    desired_blocks
                );
                Ok((desired_end, desired_blocks))
            }
        }
    }

    fn spawn_background_enrich(&self, notes: Vec<DecryptedNote>, require_memos: bool) {
        let sink = match self.storage.clone() {
            Some(s) => s,
            None => return,
        };
        let client = self.client.clone();
        let keys = self.keys.clone();
        let wallet_id = self.wallet_id.clone();
        let max_parallel = self.config.max_parallel_decrypt.max(1);
        let semaphore = Arc::clone(&self.enrich_semaphore);

        tokio::spawn(async move {
            let _permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => return,
            };
            let mut notes = notes;
            if let Err(e) = SyncEngine::fetch_and_enrich_notes_with_context(
                client,
                sink,
                wallet_id,
                keys,
                max_parallel,
                &mut notes,
                require_memos,
            )
            .await
            {
                tracing::warn!("Background full-tx enrich failed: {}", e);
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
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:spawn_background_enrich","message":"background enrich failed","data":{{"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id, ts, e
                    );
                }
            }
        });
    }

    async fn fetch_and_enrich_notes(
        &self,
        notes: &mut [DecryptedNote],
        require_memos: bool,
    ) -> Result<()> {
        let sink = match self.storage.clone() {
            Some(s) => s,
            None => return Ok(()),
        };
        let client = self.client.clone();
        let keys = self.keys.clone();
        let wallet_id = self.wallet_id.clone();
        let max_parallel = self.config.max_parallel_decrypt.max(1);

        Self::fetch_and_enrich_notes_with_context(
            client,
            sink,
            wallet_id,
            keys,
            max_parallel,
            notes,
            require_memos,
        )
        .await
    }

    /// Fetch full transactions to enrich notes (memos, Orchard nullifiers, outgoing memo recovery).
    async fn fetch_and_enrich_notes_with_context(
        client: LightClient,
        sink: StorageSink,
        wallet_id: Option<String>,
        keys: Vec<WalletKeyGroup>,
        max_parallel: usize,
        notes: &mut [DecryptedNote],
        require_memos: bool,
    ) -> Result<()> {
        let mut key_index_by_id: HashMap<i64, usize> = HashMap::new();
        for (idx, key) in keys.iter().enumerate() {
            key_index_by_id.insert(key.key_id, idx);
        }

        let mut fallback_group = keys.first().cloned();
        if fallback_group.is_none() {
            let secret = {
                let db = Database::open(&sink.db_path, &sink.key, sink.master_key.clone())?;
                let repo = Repository::new(&db);
                let wallet_id = wallet_id
                    .as_ref()
                    .ok_or_else(|| Error::Sync("Wallet ID not set".to_string()))?;
                repo.get_wallet_secret(wallet_id)?
                    .ok_or_else(|| Error::Sync("Wallet secret not found".to_string()))?
            };

            let mut fallback = WalletKeyGroup {
                key_id: 0,
                sapling_dfvk: None,
                orchard_fvk: None,
                sapling_ivk: None,
                orchard_ivk: None,
                sapling_ovk: None,
                orchard_ovk: None,
            };

            if let Some(ivk) = secret.sapling_ivk {
                if ivk.len() == 32 {
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(&ivk[..32]);
                    fallback.sapling_ivk = Some(bytes);
                }
            }

            if let Some(ivk) = secret.orchard_ivk {
                if ivk.len() == 64 {
                    let mut bytes = [0u8; 64];
                    bytes.copy_from_slice(&ivk[..64]);
                    fallback.orchard_ivk = Some(bytes);
                } else if ivk.len() == 137 {
                    if let Ok(fvk) = OrchardExtendedFullViewingKey::from_bytes(&ivk) {
                        fallback.orchard_ivk = Some(fvk.to_ivk_bytes());
                        fallback.orchard_ovk = Some(fvk.to_ovk());
                        fallback.orchard_fvk = Some(fvk);
                    }
                }
            }

            if fallback.sapling_ivk.is_some()
                || fallback.orchard_ivk.is_some()
                || fallback.orchard_fvk.is_some()
            {
                fallback_group = Some(fallback);
            }
        }

        let total_notes = notes.len();
        let sapling_notes_total = notes
            .iter()
            .filter(|note| note.note_type == NoteType::Sapling)
            .count();
        let orchard_notes_total = notes
            .iter()
            .filter(|note| note.note_type == NoteType::Orchard)
            .count();
        let has_sapling_ivk = keys.iter().any(|key| key.sapling_ivk.is_some())
            || fallback_group
                .as_ref()
                .map(|key| key.sapling_ivk.is_some())
                .unwrap_or(false);
        let has_orchard_ivk = keys.iter().any(|key| key.orchard_ivk.is_some())
            || fallback_group
                .as_ref()
                .map(|key| key.orchard_ivk.is_some())
                .unwrap_or(false);
        let has_sapling_ovk = keys.iter().any(|key| key.sapling_ovk.is_some())
            || fallback_group
                .as_ref()
                .map(|key| key.sapling_ovk.is_some())
                .unwrap_or(false);
        let has_orchard_ovk = keys.iter().any(|key| key.orchard_ovk.is_some())
            || fallback_group
                .as_ref()
                .map(|key| key.orchard_ovk.is_some())
                .unwrap_or(false);
        let has_orchard_fvk = keys.iter().any(|key| key.orchard_fvk.is_some())
            || fallback_group
                .as_ref()
                .map(|key| key.orchard_fvk.is_some())
                .unwrap_or(false);

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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"fetch_and_enrich input","data":{{"total_notes":{},"sapling_notes":{},"orchard_notes":{},"require_memos":{},"has_sapling_ivk":{},"has_orchard_ivk":{},"has_sapling_ovk":{},"has_orchard_ovk":{},"has_orchard_fvk":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                total_notes,
                sapling_notes_total,
                orchard_notes_total,
                require_memos,
                has_sapling_ivk,
                has_orchard_ivk,
                has_sapling_ovk,
                has_orchard_ovk,
                has_orchard_fvk
            );
        }

        if !has_sapling_ivk && !has_orchard_ivk {
            return Ok(());
        }

        #[derive(Default, Clone)]
        struct TxWork {
            indices: Vec<usize>,
            block: Option<u64>,
            index: Option<u64>,
        }

        let mut tx_work: HashMap<[u8; 32], TxWork> = HashMap::new();
        let mut failed_txids: HashSet<[u8; 32]> = HashSet::new();
        let mut invalid_orchard_indices: HashSet<usize> = HashSet::new();
        let mut sapling_needs_tx = 0usize;
        let mut orchard_needs_tx = 0usize;
        let mut memo_needed = 0usize;
        let mut orchard_nullifier_zero = 0usize;
        let mut orchard_nullifier_missing_fvk = 0usize;
        let mut skipped_txid_len = 0usize;

        for (note_idx, note) in notes.iter_mut().enumerate() {
            let key_group = note
                .key_id
                .and_then(|key_id| key_index_by_id.get(&key_id).and_then(|idx| keys.get(*idx)))
                .or(fallback_group.as_ref());
            let orchard_nullifier_zero_local =
                note.note_type == NoteType::Orchard && note.nullifier.iter().all(|b| *b == 0);
            if orchard_nullifier_zero_local {
                orchard_nullifier_zero += 1;
                if key_group
                    .and_then(|group| group.orchard_fvk.as_ref())
                    .is_none()
                {
                    orchard_nullifier_missing_fvk += 1;
                }
            }

            if note.tx_hash.len() != 32 {
                skipped_txid_len += 1;
                continue;
            }

            let mut txid = [0u8; 32];
            txid.copy_from_slice(&note.tx_hash[..32]);

            let mut needs_tx = false;
            let mut needs_memo_tx = false;

            if require_memos && note.memo_bytes().is_none() {
                match sink.get_note_by_txid_and_index(&note.tx_hash, note.output_index as i64) {
                    Ok(Some(db_note)) => {
                        if let Some(memo) = db_note.memo {
                            note.set_memo_bytes(memo);
                        } else {
                            needs_tx = true;
                            needs_memo_tx = true;
                        }
                    }
                    Ok(None) => {
                        needs_tx = true;
                        needs_memo_tx = true;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load memo from database for tx {} output {}: {}",
                            hex::encode(&note.tx_hash),
                            note.output_index,
                            e
                        );
                        needs_tx = true;
                        needs_memo_tx = true;
                    }
                }
            }

            let needs_orchard_nullifier = orchard_nullifier_zero_local
                && key_group
                    .and_then(|group| group.orchard_fvk.as_ref())
                    .is_some();
            if needs_orchard_nullifier {
                needs_tx = true;
            }

            if needs_tx {
                if needs_memo_tx {
                    memo_needed += 1;
                }
                match note.note_type {
                    NoteType::Sapling => sapling_needs_tx += 1,
                    NoteType::Orchard => orchard_needs_tx += 1,
                }
                let entry = tx_work.entry(txid).or_default();
                entry.indices.push(note_idx);
                entry.block.get_or_insert(note.height);
                entry.index.get_or_insert(note.tx_index as u64);
            }
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"fetch_and_enrich work summary","data":{{"total_notes":{},"sapling_notes":{},"orchard_notes":{},"skipped_txid_len":{},"memo_needed":{},"orchard_nullifier_zero":{},"orchard_nullifier_missing_fvk":{},"sapling_needs_tx":{},"orchard_needs_tx":{},"txids":{},"require_memos":{},"has_sapling_ivk":{},"has_orchard_ivk":{},"has_orchard_fvk":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                total_notes,
                sapling_notes_total,
                orchard_notes_total,
                skipped_txid_len,
                memo_needed,
                orchard_nullifier_zero,
                orchard_nullifier_missing_fvk,
                sapling_needs_tx,
                orchard_needs_tx,
                tx_work.len(),
                require_memos,
                has_sapling_ivk,
                has_orchard_ivk,
                has_orchard_fvk
            );
        }

        let max_parallel = max_parallel.max(1);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_parallel));
        let fetch_start = Instant::now();
        let sapling_ovk = keys
            .iter()
            .find_map(|key| key.sapling_ovk.as_ref())
            .or_else(|| {
                fallback_group
                    .as_ref()
                    .and_then(|key| key.sapling_ovk.as_ref())
            });
        let orchard_ovk = keys
            .iter()
            .find_map(|key| key.orchard_ovk.as_ref())
            .or_else(|| {
                fallback_group
                    .as_ref()
                    .and_then(|key| key.orchard_ovk.as_ref())
            });

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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:1693","message":"fetch_and_enrich start","data":{{"txids":{},"max_parallel":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                tx_work.len(),
                max_parallel
            );
        }

        let txid_count = tx_work.len();
        let mut tasks = Vec::with_capacity(txid_count);
        for (txid, work) in tx_work {
            let client = client.clone();
            let sem = Arc::clone(&semaphore);
            let work_clone = work.clone();
            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await.ok();
                let raw = client
                    .get_transaction_with_fallback(&txid, work_clone.block, work_clone.index)
                    .await;
                (txid, work_clone, raw)
            }));
        }

        for task in tasks {
            let (txid, work, raw_result) = match task.await {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!("Full tx fetch task failed: {}", e);
                    continue;
                }
            };

            let raw_tx_bytes = match raw_result {
                Ok(raw) => raw,
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch full transaction {}: {}",
                        hex::encode(txid),
                        e
                    );
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
                        let txid_prefix = hex::encode(&txid[..4]);
                        let _ = writeln!(
                            file,
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"full tx fetch failed","data":{{"txid_prefix":"{}","block":{},"index":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                            id,
                            ts,
                            txid_prefix,
                            work.block.unwrap_or(0),
                            work.index.unwrap_or(0),
                            e
                        );
                    }
                    failed_txids.insert(txid);
                    continue;
                }
            };
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
                let txid_prefix = hex::encode(&txid[..4]);
                let _ = writeln!(
                    file,
                    r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"full tx fetch ok","data":{{"txid_prefix":"{}","block":{},"index":{},"bytes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    id,
                    ts,
                    txid_prefix,
                    work.block.unwrap_or(0),
                    work.index.unwrap_or(0),
                    raw_tx_bytes.len()
                );
            }

            for note_idx in work.indices {
                let note = &mut notes[note_idx];
                let key_group = note
                    .key_id
                    .and_then(|key_id| key_index_by_id.get(&key_id).and_then(|idx| keys.get(*idx)))
                    .or(fallback_group.as_ref());

                match note.note_type {
                    NoteType::Sapling => {
                        if !require_memos || note.memo_bytes().is_some() {
                            continue;
                        }

                        if let Some(sapling_ivk) =
                            key_group.and_then(|group| group.sapling_ivk.as_ref())
                        {
                            match decrypt_memo_from_raw_tx_with_ivk_bytes(
                                &raw_tx_bytes,
                                note.output_index,
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
                        let orchard_ivk =
                            match key_group.and_then(|group| group.orchard_ivk.as_ref()) {
                                Some(ivk) => ivk,
                                None => continue,
                            };
                        let txid_prefix = if note.tx_hash.len() >= 4 {
                            hex::encode(&note.tx_hash[..4])
                        } else {
                            hex::encode(&note.tx_hash)
                        };
                        let cmx_prefix = hex::encode(&note.commitment[..4]);

                        match decrypt_orchard_memo_from_raw_tx_with_ivk_bytes(
                            &raw_tx_bytes,
                            note.output_index,
                            orchard_ivk,
                            Some(&note.commitment),
                        ) {
                            Ok(Some(decrypted)) => {
                                note.orchard_rho = Some(decrypted.rho);
                                note.orchard_rseed = Some(decrypted.rseed);
                                if note.note_bytes.is_empty() {
                                    match orchard_address_from_ivk_diversifier(
                                        orchard_ivk,
                                        &note.diversifier,
                                    ) {
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
                                    if let Some(fvk) =
                                        key_group.and_then(|group| group.orchard_fvk.as_ref())
                                    {
                                        match orchard_nullifier_from_parts(
                                            &fvk.inner,
                                            decrypted.address,
                                            decrypted.value,
                                            decrypted.rho,
                                            decrypted.rseed,
                                        ) {
                                            Ok(nf) => note.nullifier = nf,
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Failed to compute Orchard nullifier: {}",
                                                    e
                                                );
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
                                                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"orchard nullifier compute failed","data":{{"txid_prefix":"{}","cmx_prefix":"{}","output_index":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                                        id,
                                                        ts,
                                                        txid_prefix,
                                                        cmx_prefix,
                                                        note.output_index,
                                                        e
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }

                                let nullifier_zero = note.nullifier.iter().all(|b| *b == 0);
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
                                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"orchard full decrypt ok","data":{{"txid_prefix":"{}","cmx_prefix":"{}","output_index":{},"nullifier_zero":{},"memo_present":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                        id,
                                        ts,
                                        txid_prefix,
                                        cmx_prefix,
                                        note.output_index,
                                        nullifier_zero,
                                        note.memo_bytes().is_some()
                                    );
                                }
                            }
                            Ok(None) => {
                                invalid_orchard_indices.insert(note_idx);
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
                                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"orchard full decrypt none","data":{{"txid_prefix":"{}","cmx_prefix":"{}","output_index":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                        id, ts, txid_prefix, cmx_prefix, note.output_index
                                    );
                                }
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
                                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_and_enrich_notes","message":"orchard full decrypt error","data":{{"txid_prefix":"{}","cmx_prefix":"{}","output_index":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                        id, ts, txid_prefix, cmx_prefix, note.output_index, e
                                    );
                                }
                            }
                        }
                    }
                }
            }

            let txid_hex = hex::encode(txid);
            let has_memo = sink.get_tx_memo(&txid_hex).ok().flatten().is_some();
            if !has_memo {
                if let Err(e) = Self::recover_outgoing_memos(
                    &raw_tx_bytes,
                    work.block.unwrap_or(0),
                    &txid_hex,
                    &sink,
                    sapling_ovk,
                    orchard_ovk,
                ) {
                    tracing::warn!("Outgoing memo recovery failed: {}", e);
                }
            }
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:1816","message":"fetch_and_enrich done","data":{{"txids":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                txid_count,
                fetch_start.elapsed().as_millis()
            );
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
        if let Some(keys) = self.keys.first() {
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

            let action_index = match usize::try_from(note_ref.output_index) {
                Ok(index) => index,
                Err(_) => continue,
            };
            match decrypt_orchard_memo_from_raw_tx_with_ivk_bytes(
                &raw_tx,
                action_index,
                orchard_ivk,
                Some(&note_ref.commitment),
            ) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ =
                        sink.delete_note_by_txid_and_index(&note_ref.txid, note_ref.output_index);
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
        if self.keys.is_empty() {
            return Ok(());
        }

        let mut dfvk_by_id: HashMap<i64, ExtendedFullViewingKey> = HashMap::new();
        for key in &self.keys {
            if let Some(ref dfvk) = key.sapling_dfvk {
                dfvk_by_id.insert(key.key_id, dfvk.clone());
            }
        }
        if dfvk_by_id.is_empty() {
            return Ok(());
        }

        let default_key_id = *dfvk_by_id.keys().next().unwrap_or(&0);

        for note in notes.iter_mut() {
            if note.note_type != NoteType::Sapling {
                continue;
            }
            if !note.nullifier.iter().all(|b| *b == 0) {
                continue;
            }

            let key_id = note.key_id.unwrap_or(default_key_id);
            let dfvk = match dfvk_by_id.get(&key_id) {
                Some(dfvk) => dfvk,
                None => match dfvk_by_id.get(&default_key_id) {
                    Some(dfvk) => dfvk,
                    None => continue,
                },
            };

            let nk = dfvk.nullifier_deriving_key();
            let sapling_ivk = if note.address_scope == AddressScope::Internal {
                let internal_ivk_bytes = dfvk.to_internal_ivk_bytes();
                match Option::from(jubjub::Fr::from_bytes(&internal_ivk_bytes)) {
                    Some(ivk_fr) => SaplingIvk(ivk_fr),
                    None => {
                        tracing::warn!(
                            "Invalid internal Sapling IVK for tx {} output {}",
                            hex::encode(&note.tx_hash),
                            note.output_index
                        );
                        continue;
                    }
                }
            } else {
                dfvk.sapling_ivk()
            };

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
                let rcm = Option::from(jubjub::Fr::from_bytes(&rseed_bytes))
                    .ok_or_else(|| Error::Sync("Invalid Sapling rseed bytes".to_string()))?;
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
                note.note_bytes =
                    encode_sapling_note_bytes(sapling_note.recipient(), leadbyte, rseed_bytes);
            }
        }

        Ok(())
    }

    async fn apply_spends(&mut self, blocks: &[CompactBlockData]) -> Result<()> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };
        let mut spend_updates: Vec<(i64, [u8; 32])> = Vec::new();
        let mut spend_nullifiers: Vec<([u8; 32], [u8; 32])> = Vec::new();
        let mut matched_nullifiers: std::collections::HashSet<[u8; 32]> =
            std::collections::HashSet::new();
        let mut sapling_spends = 0u64;
        let mut orchard_spends = 0u64;
        let mut matched_spends = 0u64;
        let mut min_height: Option<u64> = None;
        let mut max_height: Option<u64> = None;

        for block in blocks {
            min_height = Some(min_height.map_or(block.height, |h| h.min(block.height)));
            max_height = Some(max_height.map_or(block.height, |h| h.max(block.height)));
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
                        sapling_spends += 1;
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&spend.nf[..32]);
                        if !nf.iter().all(|b| *b == 0) {
                            spend_nullifiers.push((nf, txid));
                            if let Some(id) = self.nullifier_cache.remove(&nf) {
                                spend_updates.push((id, txid));
                                matched_nullifiers.insert(nf);
                                has_spend = true;
                                matched_spends += 1;
                            }
                        }
                    }
                }

                for action in &tx.actions {
                    if action.nullifier.len() == 32 {
                        orchard_spends += 1;
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&action.nullifier[..32]);
                        if !nf.iter().all(|b| *b == 0) {
                            spend_nullifiers.push((nf, txid));
                            if let Some(id) = self.nullifier_cache.remove(&nf) {
                                spend_updates.push((id, txid));
                                matched_nullifiers.insert(nf);
                                has_spend = true;
                                matched_spends += 1;
                            }
                        }
                    }
                }

                if has_spend {
                    let txid_hex = hex::encode(txid);
                    let tx_fee = tx.fee.unwrap_or(0) as i64;
                    let _ = sink.upsert_transaction(&txid_hex, block_height, block_time, tx_fee);
                }
            }
        }

        let mut updated_count = 0u64;
        let mut fallback_updates = 0u64;
        if !spend_updates.is_empty() {
            let start = Instant::now();
            match sink.mark_notes_spent_by_ids_with_txid(&spend_updates) {
                Ok(updated) => {
                    updated_count = updated;
                    tracing::debug!(
                        "Marked {} notes spent ({} nullifiers) in {}ms",
                        updated,
                        spend_updates.len(),
                        start.elapsed().as_millis()
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to mark notes spent for batch: {}", e);
                }
            }
        }
        if !spend_nullifiers.is_empty() {
            let mut fallback_entries: Vec<([u8; 32], [u8; 32])> = Vec::new();
            for (nf, txid) in &spend_nullifiers {
                if !matched_nullifiers.contains(nf) {
                    fallback_entries.push((*nf, *txid));
                }
            }
            if !fallback_entries.is_empty() {
                match sink.mark_notes_spent_by_nullifiers_with_txid(&fallback_entries) {
                    Ok(updated) => {
                        if updated > 0 {
                            fallback_updates = updated;
                            self.nullifier_cache.clear();
                            self.nullifier_cache_loaded = false;
                            let _ = self.ensure_nullifier_cache();
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Fallback spend match failed: {}", e);
                    }
                }
            }
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
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:apply_spends","message":"apply_spends summary","data":{{"start":{},"end":{},"sapling_spends":{},"orchard_spends":{},"matched_spends":{},"updates":{},"fallback_updates":{},"cache_size":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                min_height.unwrap_or(0),
                max_height.unwrap_or(0),
                sapling_spends,
                orchard_spends,
                matched_spends,
                updated_count,
                fallback_updates,
                self.nullifier_cache.len()
            );
        }

        Ok(())
    }

    /// Get witness for a Sapling note at a given position.
    /// Returns None if position is not marked or witness cannot be computed.
    pub async fn get_sapling_witness(
        &self,
        position: u64,
    ) -> Result<
        Option<
            incrementalmerkletree::MerklePath<
                zcash_primitives::sapling::Node,
                { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH },
            >,
        >,
    > {
        let mut sapling_frontier = self.frontier.write().await;
        if let Some(path) = sapling_frontier.witness(position)? {
            return Ok(Some(path));
        }

        if sapling_frontier.recover_mark(position)? {
            tracing::info!(
                "Recovered missing Sapling mark metadata for position {}",
                position
            );
            return sapling_frontier.witness(position);
        }

        tracing::warn!(
            "Sapling witness unavailable for position {} (not marked and recovery failed)",
            position
        );
        Ok(None)
    }

    /// Get witness for an Orchard note at a given position
    /// Returns None if position is not marked or witness cannot be computed
    pub async fn get_orchard_witness(
        &self,
        position: u64,
    ) -> Result<
        Option<
            incrementalmerkletree::MerklePath<
                orchard::tree::MerkleHashOrchard,
                { zcash_primitives::sapling::NOTE_COMMITMENT_TREE_DEPTH },
            >,
        >,
    > {
        let mut orchard_frontier = self.orchard_frontier.write().await;
        if let Some(path) = orchard_frontier.witness(position)? {
            return Ok(Some(path));
        }

        if orchard_frontier.recover_mark(position)? {
            tracing::info!(
                "Recovered missing Orchard mark metadata for position {}",
                position
            );
            return orchard_frontier.witness(position);
        }

        tracing::warn!(
            "Orchard witness unavailable for position {} (not marked and recovery failed)",
            position
        );
        Ok(None)
    }

    /// Get current Orchard anchor from the frontier, if available.
    pub async fn get_orchard_anchor(&self) -> Option<[u8; 32]> {
        let orchard_frontier = self.orchard_frontier.read().await;
        orchard_frontier.root()
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
            storage.save_frontier_snapshot(
                height as u32,
                &snapshot_bytes,
                env!("CARGO_PKG_VERSION"),
            )?;
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

        if let Some(snapshot_height) = self
            .restore_frontiers_from_storage(checkpoint_height)
            .await?
        {
            return Ok(snapshot_height);
        }

        *self.frontier.write().await = SaplingFrontier::new();
        *self.orchard_frontier.write().await = OrchardFrontier::new();

        Ok(checkpoint_height)
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
            cache
                .load_range(height, height)
                .ok()
                .and_then(|mut blocks| blocks.pop())
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
                    let progress = self.progress.write().await;
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

fn encode_orchard_note_bytes(address: &OrchardAddress, rho: [u8; 32], rseed: [u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 43 + 32 + 32);
    out.push(ORCHARD_NOTE_BYTES_VERSION);
    out.extend_from_slice(&address.to_raw_address_bytes());
    out.extend_from_slice(&rho);
    out.extend_from_slice(&rseed);
    out
}

fn decode_sapling_address_bytes_from_note_bytes(note_bytes: &[u8]) -> Option<[u8; 43]> {
    if note_bytes.is_empty() {
        return None;
    }
    let expected = 1 + 43;
    if note_bytes.len() >= expected && note_bytes[0] == SAPLING_NOTE_BYTES_VERSION {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[1..44]);
        return Some(address);
    }
    if note_bytes.len() >= 43 {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[0..43]);
        return Some(address);
    }
    None
}

fn decode_orchard_address_bytes_from_note_bytes(note_bytes: &[u8]) -> Option<[u8; 43]> {
    if note_bytes.is_empty() {
        return None;
    }
    let expected = 1 + 43;
    if note_bytes.len() >= expected && note_bytes[0] == ORCHARD_NOTE_BYTES_VERSION {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[1..44]);
        return Some(address);
    }
    if note_bytes.len() >= 43 {
        let mut address = [0u8; 43];
        address.copy_from_slice(&note_bytes[0..43]);
        return Some(address);
    }
    None
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
    if bytes.len() < FRONTIER_SNAPSHOT_MAGIC.len() + 1
        || !bytes.starts_with(&FRONTIER_SNAPSHOT_MAGIC)
    {
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
    network_type: NetworkType,
}

impl Clone for StorageSink {
    fn clone(&self) -> Self {
        let key_bytes = *self.key.as_bytes();
        Self {
            db_path: self.db_path.clone(),
            key: EncryptionKey::from_bytes(key_bytes),
            master_key: self.master_key.clone(),
            account_id: self.account_id,
            network_type: self.network_type,
        }
    }
}

impl StorageSink {
    fn persist_notes(
        &self,
        notes: &[DecryptedNote],
        tx_times: &HashMap<String, i64>,
        tx_fees: &HashMap<String, i64>,
        position_mappings: &PositionMaps,
    ) -> Result<Vec<([u8; 32], i64)>> {
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        let sync_state = SyncStateStorage::new(&db);
        let mut inserted: Vec<([u8; 32], i64)> = Vec::new();

        let derive_address_id = |note: &DecryptedNote, timestamp: i64| -> Result<Option<i64>> {
            if note.note_bytes.is_empty() {
                return Ok(None);
            }
            let address_string = match note.note_type {
                crate::pipeline::NoteType::Sapling => {
                    decode_sapling_address_bytes_from_note_bytes(&note.note_bytes)
                        .and_then(|bytes| SaplingPaymentAddress::from_bytes(&bytes))
                        .map(|addr| {
                            PiratePaymentAddress { inner: addr }
                                .encode_for_network(self.network_type)
                        })
                }
                crate::pipeline::NoteType::Orchard => {
                    decode_orchard_address_bytes_from_note_bytes(&note.note_bytes)
                        .and_then(|bytes| {
                            Option::from(OrchardAddress::from_raw_address_bytes(&bytes))
                        })
                        .and_then(|addr| {
                            PirateOrchardPaymentAddress { inner: addr }
                                .encode_for_network(self.network_type)
                                .ok()
                        })
                }
            };

            let Some(address_string) = address_string else {
                return Ok(None);
            };

            let address_type = match note.note_type {
                crate::pipeline::NoteType::Sapling => {
                    pirate_storage_sqlite::models::AddressType::Sapling
                }
                crate::pipeline::NoteType::Orchard => {
                    pirate_storage_sqlite::models::AddressType::Orchard
                }
            };

            let address_record = pirate_storage_sqlite::Address {
                id: None,
                key_id: note.key_id,
                account_id: self.account_id,
                diversifier_index: 0,
                address: address_string.clone(),
                address_type,
                label: None,
                created_at: timestamp,
                color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
                address_scope: note.address_scope,
            };
            let _ = repo.upsert_address(&address_record);
            Ok(repo
                .get_address_by_string(self.account_id, &address_string)?
                .and_then(|addr| addr.id))
        };

        for n in notes {
            // Skip if we don't have essential fields
            if n.txid.is_empty() {
                continue;
            }
            let note_type = match n.note_type {
                crate::pipeline::NoteType::Orchard => {
                    pirate_storage_sqlite::models::NoteType::Orchard
                }
                crate::pipeline::NoteType::Sapling => {
                    pirate_storage_sqlite::models::NoteType::Sapling
                }
            };
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

            let address_id = derive_address_id(n, timestamp)?;

            if let Ok(Some(existing)) =
                repo.get_note_by_txid_and_index(self.account_id, &n.txid, n.output_index as i64)
            {
                let mut updated = existing.clone();
                let mut changed = false;
                let incoming_nullifier = n.nullifier.to_vec();
                let incoming_commitment = n.commitment.to_vec();
                let incoming_value = n.value as i64;

                if existing.note_type != note_type {
                    updated.note_type = note_type;
                    changed = true;
                }

                if existing.value != incoming_value {
                    updated.value = incoming_value;
                    changed = true;
                }

                if existing.nullifier != incoming_nullifier {
                    updated.nullifier = incoming_nullifier;
                    changed = true;
                }

                if existing.commitment != incoming_commitment {
                    updated.commitment = incoming_commitment;
                    changed = true;
                }

                if existing.memo.is_none() {
                    if let Some(memo) = n.memo_bytes() {
                        updated.memo = Some(memo.to_vec());
                        changed = true;
                    }
                }

                if n.height > 0 && existing.height != n.height as i64 {
                    updated.height = n.height as i64;
                    changed = true;
                }

                if existing.address_id.is_none() {
                    if let Some(id) = address_id {
                        updated.address_id = Some(id);
                        changed = true;
                    }
                }

                if existing.note.is_none() && !n.note_bytes.is_empty() {
                    updated.note = Some(n.note_bytes.clone());
                    changed = true;
                }
                if !n.note_bytes.is_empty() && existing.note.as_ref() != Some(&n.note_bytes) {
                    updated.note = Some(n.note_bytes.clone());
                    changed = true;
                }

                if existing.merkle_path.is_none() && !n.merkle_path.is_empty() {
                    updated.merkle_path = Some(n.merkle_path.clone());
                    changed = true;
                }
                if !n.merkle_path.is_empty()
                    && existing.merkle_path.as_ref() != Some(&n.merkle_path)
                {
                    updated.merkle_path = Some(n.merkle_path.clone());
                    changed = true;
                }

                if existing.anchor.is_none() {
                    if let Some(anchor) = n.anchor {
                        updated.anchor = Some(anchor.to_vec());
                        changed = true;
                    }
                }
                if let Some(anchor) = n.anchor {
                    let anchor_vec = anchor.to_vec();
                    if existing.anchor.as_ref() != Some(&anchor_vec) {
                        updated.anchor = Some(anchor_vec);
                        changed = true;
                    }
                }

                if existing.position.is_none() {
                    if let Some(position) = n.position {
                        updated.position = Some(position as i64);
                        changed = true;
                    }
                }
                if let Some(position) = n.position {
                    let pos = position as i64;
                    if existing.position != Some(pos) {
                        updated.position = Some(pos);
                        changed = true;
                    }
                }

                if existing.key_id.is_none() && n.key_id.is_some() {
                    updated.key_id = n.key_id;
                    changed = true;
                }

                if !n.diversifier.is_empty() {
                    let diversifier = n.diversifier.clone();
                    if existing.diversifier.as_ref() != Some(&diversifier) {
                        updated.diversifier = Some(diversifier);
                        changed = true;
                    }
                }

                if changed {
                    repo.update_note_by_id(&updated)?;
                }
                continue;
            }

            let record = NoteRecord {
                id: None,
                account_id: self.account_id,
                key_id: n.key_id,
                note_type,
                value: n.value as i64,
                nullifier: n.nullifier.to_vec(),
                commitment: n.commitment.to_vec(),
                spent: false,
                height: n.height as i64,
                txid: n.txid.clone(),
                output_index: n.output_index as i64,
                address_id,
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
                        crate::pipeline::NoteType::Sapling => {
                            TxOutputKey::new(&n.tx_hash, n.output_index)
                                .and_then(|key| position_mappings.sapling_by_tx.get(&key).copied())
                        }
                        crate::pipeline::NoteType::Orchard => position_mappings
                            .orchard_by_commitment
                            .get(&n.commitment)
                            .copied(),
                    };
                    n.position.or(fallback).map(|p| p as i64)
                },
                memo: n.memo_bytes().map(|b| b.to_vec()),
            };
            match repo.insert_note(&record) {
                Ok(id) => {
                    if record.nullifier.len() == 32 {
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&record.nullifier[..32]);
                        inserted.push((nf, id));
                    }
                }
                Err(e) => {
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
        }
        // Optionally update sync state height
        if let Some(max_h) = notes.iter().map(|n| n.height).max() {
            let _ = sync_state.save_sync_state(max_h, max_h, max_h);
        }
        Ok(inserted)
    }

    fn get_note_by_txid_and_index(
        &self,
        txid: &[u8],
        output_index: i64,
    ) -> Result<Option<NoteRecord>> {
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

    fn mark_notes_spent_by_nullifiers_with_txid(
        &self,
        entries: &[([u8; 32], [u8; 32])],
    ) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.mark_notes_spent_by_nullifiers_with_txid(self.account_id, entries)?)
    }

    fn mark_notes_spent_by_ids_with_txid(&self, entries: &[(i64, [u8; 32])]) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }
        let db = Database::open(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.mark_notes_spent_by_ids_with_txid(entries)?)
    }

    fn upsert_transaction(
        &self,
        txid_hex: &str,
        height: i64,
        timestamp: i64,
        fee: i64,
    ) -> Result<()> {
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

    fn save_sync_state(
        &self,
        local_height: u64,
        target_height: u64,
        last_checkpoint_height: u64,
    ) -> Result<()> {
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
#[derive(Clone)]
struct WalletKeyGroup {
    key_id: i64,
    sapling_dfvk: Option<ExtendedFullViewingKey>,
    orchard_fvk: Option<OrchardExtendedFullViewingKey>,
    sapling_ivk: Option<[u8; 32]>,
    orchard_ivk: Option<[u8; 64]>,
    sapling_ovk: Option<SaplingOutgoingViewingKey>,
    orchard_ovk: Option<orchard::keys::OutgoingViewingKey>,
}

#[derive(Clone, Debug)]
struct SaplingOutputMeta {
    height: u64,
    tx_index: usize,
    output_index: usize,
    tx_hash: Vec<u8>,
}

#[derive(Clone, Debug)]
struct OrchardOutputMeta {
    height: u64,
    tx_index: usize,
    output_index: usize,
    tx_hash: Vec<u8>,
    commitment: [u8; 32],
}

#[derive(Clone, Debug)]
struct SaplingBatchOutput {
    epk: [u8; 32],
    cmu: [u8; 32],
    ciphertext: [u8; 52],
}

impl ShieldedOutput<SaplingDomain<PirateNetwork>, COMPACT_NOTE_SIZE> for SaplingBatchOutput {
    fn ephemeral_key(&self) -> EphemeralKeyBytes {
        EphemeralKeyBytes(self.epk)
    }

    fn cmstar_bytes(
        &self,
    ) -> <SaplingDomain<PirateNetwork> as zcash_note_encryption::Domain>::ExtractedCommitmentBytes
    {
        self.cmu
    }

    fn enc_ciphertext(&self) -> &[u8; COMPACT_NOTE_SIZE] {
        &self.ciphertext
    }
}

fn sapling_rseed_to_bytes(note: &zcash_primitives::sapling::Note) -> (u8, [u8; 32]) {
    match note.rseed() {
        Rseed::BeforeZip212(rcm) => {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&rcm.to_repr());
            (0x01, bytes)
        }
        Rseed::AfterZip212(rseed) => (0x02, *rseed),
    }
}

type CompactDecryptResult<D> = Option<(
    (
        <D as zcash_note_encryption::Domain>::Note,
        <D as zcash_note_encryption::Domain>::Recipient,
    ),
    usize,
)>;

struct TrialDecryptBatchInputs<'a> {
    blocks: &'a [CompactBlockData],
    sapling_ivks: &'a [PreparedIncomingViewingKey],
    sapling_key_ids: &'a [i64],
    sapling_scopes: &'a [AddressScope],
    orchard_ivks: &'a [OrchardPreparedIncomingViewingKey],
    orchard_key_ids: &'a [i64],
    orchard_scopes: &'a [AddressScope],
    orchard_fvks: &'a [orchard::keys::FullViewingKey],
    decrypt_pool: &'a rayon::ThreadPool,
    max_parallel: usize,
}

fn try_compact_note_decryption_parallel<D, Output>(
    pool: &rayon::ThreadPool,
    ivks: &[D::IncomingViewingKey],
    outputs: &[(D, Output)],
    max_parallel: usize,
) -> Vec<CompactDecryptResult<D>>
where
    D: zcash_note_encryption::BatchDomain + Sync,
    Output: ShieldedOutput<D, COMPACT_NOTE_SIZE> + Sync,
    D::IncomingViewingKey: Sync,
    D::Note: Send,
    D::Recipient: Send,
{
    if ivks.is_empty() {
        return (0..outputs.len()).map(|_| None).collect();
    }

    let outputs_len = outputs.len();
    if outputs_len == 0 {
        return Vec::new();
    }

    let max_parallel = max_parallel.max(1);
    if max_parallel == 1 || outputs_len < MIN_PARALLEL_OUTPUTS {
        return note_batch::try_compact_note_decryption(ivks, outputs);
    }

    let mut chunk_size = outputs_len.div_ceil(max_parallel);
    if chunk_size < MIN_PARALLEL_OUTPUTS {
        chunk_size = MIN_PARALLEL_OUTPUTS;
    }
    let chunk_count = outputs_len.div_ceil(chunk_size);
    if chunk_count <= 1 {
        return note_batch::try_compact_note_decryption(ivks, outputs);
    }

    pool.install(|| {
        outputs
            .par_chunks(chunk_size)
            .map(|chunk| note_batch::try_compact_note_decryption(ivks, chunk))
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect()
    })
}

fn trial_decrypt_batch_impl(inputs: TrialDecryptBatchInputs<'_>) -> Result<Vec<DecryptedNote>> {
    let TrialDecryptBatchInputs {
        blocks,
        sapling_ivks,
        sapling_key_ids,
        sapling_scopes,
        orchard_ivks,
        orchard_key_ids,
        orchard_scopes,
        orchard_fvks,
        decrypt_pool,
        max_parallel,
    } = inputs;

    let mut sapling_outputs: Vec<(SaplingDomain<PirateNetwork>, SaplingBatchOutput)> = Vec::new();
    let mut sapling_meta: Vec<SaplingOutputMeta> = Vec::new();
    let mut orchard_outputs: Vec<(OrchardDomain, CompactAction)> = Vec::new();
    let mut orchard_meta: Vec<OrchardOutputMeta> = Vec::new();

    for block in blocks {
        let height = block.height;
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let tx_index = tx.index.unwrap_or(tx_idx as u64) as usize;
            let tx_hash = tx.hash.clone();

            if !sapling_ivks.is_empty() {
                for (output_idx, output) in tx.outputs.iter().enumerate() {
                    if output.cmu.len() != 32
                        || output.ephemeral_key.len() != 32
                        || output.ciphertext.len() < 52
                    {
                        continue;
                    }

                    let mut cmu = [0u8; 32];
                    cmu.copy_from_slice(&output.cmu[..32]);
                    let mut epk = [0u8; 32];
                    epk.copy_from_slice(&output.ephemeral_key[..32]);
                    let mut ciphertext = [0u8; 52];
                    ciphertext.copy_from_slice(&output.ciphertext[..52]);

                    let domain = SaplingDomain::for_height(
                        PirateNetwork::default(),
                        BlockHeight::from_u32(height as u32),
                    );
                    sapling_outputs.push((
                        domain,
                        SaplingBatchOutput {
                            epk,
                            cmu,
                            ciphertext,
                        },
                    ));
                    sapling_meta.push(SaplingOutputMeta {
                        height,
                        tx_index,
                        output_index: output_idx,
                        tx_hash: tx_hash.clone(),
                    });
                }
            }

            if !orchard_ivks.is_empty() {
                for (action_idx, action) in tx.actions.iter().enumerate() {
                    if action.cmx.len() != 32
                        || action.nullifier.len() != 32
                        || action.ephemeral_key.len() != 32
                        || action.enc_ciphertext.len() < 52
                    {
                        continue;
                    }

                    let mut cmx_bytes = [0u8; 32];
                    cmx_bytes.copy_from_slice(&action.cmx[..32]);
                    let cmx_ct = OrchardExtractedNoteCommitment::from_bytes(&cmx_bytes);
                    if !bool::from(cmx_ct.is_some()) {
                        continue;
                    }
                    let cmx = cmx_ct.unwrap();

                    let mut nf_bytes = [0u8; 32];
                    nf_bytes.copy_from_slice(&action.nullifier[..32]);
                    let nf_ct = OrchardNullifier::from_bytes(&nf_bytes);
                    if !bool::from(nf_ct.is_some()) {
                        continue;
                    }
                    let nullifier = nf_ct.unwrap();

                    let mut epk = [0u8; 32];
                    epk.copy_from_slice(&action.ephemeral_key[..32]);
                    let mut enc_ciphertext = [0u8; 52];
                    enc_ciphertext.copy_from_slice(&action.enc_ciphertext[..52]);

                    let compact_action = CompactAction::from_parts(
                        nullifier,
                        cmx,
                        EphemeralKeyBytes(epk),
                        enc_ciphertext,
                    );
                    let domain = OrchardDomain::for_nullifier(nullifier);
                    orchard_outputs.push((domain, compact_action));
                    orchard_meta.push(OrchardOutputMeta {
                        height,
                        tx_index,
                        output_index: action_idx,
                        tx_hash: tx_hash.clone(),
                        commitment: cmx.to_bytes(),
                    });
                }
            }
        }
    }

    let mut notes = Vec::new();

    if !sapling_ivks.is_empty() && !sapling_outputs.is_empty() {
        let sapling_results = try_compact_note_decryption_parallel(
            decrypt_pool,
            sapling_ivks,
            &sapling_outputs,
            max_parallel,
        );

        for (idx, result) in sapling_results.into_iter().enumerate() {
            if let Some(((note, address), ivk_index)) = result {
                let meta = &sapling_meta[idx];
                let (leadbyte, rseed_bytes) = sapling_rseed_to_bytes(&note);
                let value = note.value().inner();
                let commitment = sapling_outputs[idx].1.cmu;
                let key_id = sapling_key_ids.get(ivk_index).copied();
                let scope = sapling_scopes
                    .get(ivk_index)
                    .copied()
                    .unwrap_or(AddressScope::External);

                let mut note_rec = DecryptedNote::new(
                    meta.height,
                    meta.tx_index,
                    meta.output_index,
                    value,
                    commitment,
                    [0u8; 32],
                    Vec::new(),
                );
                note_rec.set_tx_hash(meta.tx_hash.clone());
                note_rec.key_id = key_id;
                note_rec.address_scope = scope;
                note_rec.diversifier = address.diversifier().0.to_vec();
                note_rec.sapling_rseed_leadbyte = Some(leadbyte);
                note_rec.sapling_rseed = Some(rseed_bytes);
                note_rec.note_bytes = encode_sapling_note_bytes(address, leadbyte, rseed_bytes);
                notes.push(note_rec);
            }
        }
    }

    if !orchard_ivks.is_empty() && !orchard_outputs.is_empty() {
        let orchard_results = try_compact_note_decryption_parallel(
            decrypt_pool,
            orchard_ivks,
            &orchard_outputs,
            max_parallel,
        );

        for (idx, result) in orchard_results.into_iter().enumerate() {
            if let Some(((note, address), ivk_index)) = result {
                let meta = &orchard_meta[idx];
                let value = note.value().inner();
                let rho = note.rho().to_bytes();
                let rseed = *note.rseed().as_bytes();
                let commitment = meta.commitment;
                let key_id = orchard_key_ids.get(ivk_index).copied();
                let fvk = orchard_fvks.get(ivk_index);
                let scope = orchard_scopes
                    .get(ivk_index)
                    .copied()
                    .unwrap_or(AddressScope::External);

                let mut note_rec = DecryptedNote::new_orchard(OrchardDecryptedNoteInit {
                    height: meta.height,
                    tx_index: meta.tx_index,
                    output_index: meta.output_index,
                    value,
                    commitment,
                    nullifier: [0u8; 32],
                    encrypted_memo: Vec::new(),
                    anchor: None,
                    position: Some(0),
                });
                note_rec.set_tx_hash(meta.tx_hash.clone());
                note_rec.key_id = key_id;
                note_rec.address_scope = scope;
                note_rec.diversifier = address.diversifier().as_array().to_vec();
                note_rec.orchard_rho = Some(rho);
                note_rec.orchard_rseed = Some(rseed);
                note_rec.note_bytes = encode_orchard_note_bytes(&address, rho, rseed);
                if let Some(fvk) = fvk {
                    note_rec.nullifier = note.nullifier(fvk).to_bytes();
                }
                notes.push(note_rec);
            }
        }
    }

    Ok(notes)
}

/// Trial decrypt a single block (Sapling/Orchard) for tests.
#[cfg(test)]
fn trial_decrypt_block(
    block: &CompactBlockData,
    sapling_ivk_bytes: Option<&[u8; 32]>,
    orchard_ivk_bytes_opt: Option<&[u8; 64]>,
) -> Result<Vec<DecryptedNote>> {
    let decrypt_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .expect("failed to build trial-decrypt thread pool");
    let mut sapling_ivks = Vec::new();
    let mut sapling_key_ids = Vec::new();
    let mut sapling_scopes = Vec::new();
    let mut orchard_ivks = Vec::new();
    let mut orchard_key_ids = Vec::new();
    let mut orchard_scopes = Vec::new();
    let orchard_fvks = Vec::new();

    if let Some(ivk_bytes) = sapling_ivk_bytes {
        if let Some(ivk_fr) = Option::from(jubjub::Fr::from_bytes(ivk_bytes)) {
            let sapling_ivk = SaplingIvk(ivk_fr);
            sapling_ivks.push(PreparedIncomingViewingKey::new(&sapling_ivk));
            sapling_key_ids.push(0);
            sapling_scopes.push(AddressScope::External);
        }
    }
    if let Some(ivk_bytes) = orchard_ivk_bytes_opt {
        let ivk_ct = OrchardIncomingViewingKey::from_bytes(ivk_bytes);
        if bool::from(ivk_ct.is_some()) {
            let ivk = ivk_ct.unwrap();
            orchard_ivks.push(OrchardPreparedIncomingViewingKey::new(&ivk));
            orchard_key_ids.push(0);
            orchard_scopes.push(AddressScope::External);
        }
    }
    trial_decrypt_batch_impl(TrialDecryptBatchInputs {
        blocks: std::slice::from_ref(block),
        sapling_ivks: &sapling_ivks,
        sapling_key_ids: &sapling_key_ids,
        sapling_scopes: &sapling_scopes,
        orchard_ivks: &orchard_ivks,
        orchard_key_ids: &orchard_key_ids,
        orchard_scopes: &orchard_scopes,
        orchard_fvks: &orchard_fvks,
        decrypt_pool: &decrypt_pool,
        max_parallel: 1,
    })
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
        let engine = SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
        assert_eq!(engine.birthday_height(), 3_800_000);
    }

    #[tokio::test]
    async fn test_birthday_height_update() {
        let mut engine =
            SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
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
        let notes = trial_decrypt_block(&block, Some(&dummy_ivk), None).unwrap();
        assert_eq!(notes.len(), 0);
    }

    #[tokio::test]
    async fn test_cancel_flag_reflects_engine_cancellation() {
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 3_800_000);
        let cancel = engine.cancel_flag();
        assert!(!cancel.is_cancelled());
        engine.cancel().await;
        assert!(cancel.is_cancelled());
    }

    #[tokio::test]
    async fn test_fetch_blocks_with_retry_short_circuits_cancelled() {
        let client = LightClient::new("http://127.0.0.1:1".to_string());
        let cancel = CancelToken::new();
        cancel.cancel();

        let result = SyncEngine::fetch_blocks_with_retry_inner(client, 10, 20, cancel).await;
        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[tokio::test]
    async fn test_fetch_blocks_with_retry_empty_range() {
        let client = LightClient::new("http://127.0.0.1:1".to_string());
        let cancel = CancelToken::new();

        let blocks = SyncEngine::fetch_blocks_with_retry_inner(client, 20, 10, cancel)
            .await
            .unwrap();
        assert!(blocks.is_empty());
    }
}
