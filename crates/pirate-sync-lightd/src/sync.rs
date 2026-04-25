//! Sync engine with batched trial decryption, checkpoints, and auto-rollback
//!
//! Production-ready sync with:
//! - Retry logic with exponential backoff
//! - Cancellation handling and interruption recovery
//! - Performance counters
//! - Mini-checkpoints every N batches
//! - ShardTree for witness tree management (single source of truth)
//! - Checkpoint loading and restoration
//! - Rollback on interruption/corruption/reorg

use crate::block_cache::{acquire_inflight, BlockCache, InflightLease};
use crate::client::{CompactBlockData, TransportMode};
use crate::orchard::full_decrypt::decrypt_orchard_memo_from_raw_tx_with_ivk_bytes;
use crate::pipeline::NoteType;
use crate::pipeline::{DecryptedNote, OrchardDecryptedNoteInit, PerfCounters};
use crate::progress::SyncStage;
use crate::sapling::full_decrypt::decrypt_memo_from_raw_tx_with_ivk_bytes;
use crate::{CancelToken, Error, LightClient, Result, SyncProgress};
use directories::ProjectDirs;
use group::ff::PrimeField;
use hex;
use incrementalmerkletree::frontier::CommitmentTree;
use incrementalmerkletree::Hashable;
use incrementalmerkletree::{Position, Retention};
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
use pirate_params::{Network as PirateParamsNetwork, NetworkType};
use pirate_storage_sqlite::models::{AccountKey, AddressScope, KeyScope, KeyType};
use pirate_storage_sqlite::repository::OrchardNoteRef;
use pirate_storage_sqlite::security::MasterKey;
use pirate_storage_sqlite::shardtree_store::{
    put_shard_roots, PersistedSubtreeRoot, SqliteShardStore,
};
use pirate_storage_sqlite::{
    truncate_above_height, ChainBlockRow, Database, EncryptionKey, NoteRecord, Repository,
    ScanQueueStorage, SpendabilityStateStorage, SyncStateStorage,
};
use rayon::prelude::*;
use shardtree::store::caching::CachingShardStore;
use shardtree::ShardTree;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use subtle::CtOption;
use tokio::sync::RwLock;
use tonic::Code;
use zcash_note_encryption::try_output_recovery_with_ovk;
use zcash_note_encryption::{
    batch as note_batch, EphemeralKeyBytes, ShieldedOutput, COMPACT_NOTE_SIZE,
};
use zcash_primitives::consensus::{BlockHeight, BranchId};
use zcash_primitives::merkle_tree::{
    read_commitment_tree, read_frontier_v0, read_frontier_v1, HashSer,
};
use zcash_primitives::sapling::keys::{
    OutgoingViewingKey as SaplingOutgoingViewingKey, PreparedIncomingViewingKey,
};
use zcash_primitives::sapling::note_encryption::{try_sapling_output_recovery, SaplingDomain};
use zcash_primitives::sapling::{
    note::ExtractedNoteCommitment as SaplingExtractedNoteCommitment, Node as SaplingNode,
    PaymentAddress as SaplingPaymentAddress, Rseed, SaplingIvk, NOTE_COMMITMENT_TREE_DEPTH,
};
use zcash_primitives::transaction::Transaction;
use zcash_primitives::zip32::Scope as SaplingScope;

mod shardtree_support;

use self::shardtree_support::{
    append_orchard_leaf, append_sapling_leaf, apply_shardtree_batches_to_trees,
    drain_historical_skip_state, merge_emitted_batches, prefill_historical_subtree_roots,
    process_historical_leaf, warm_shardtree_cache_with_subtrees_enabled, HistoricalLeafSink,
    HistoricalPrefillState, ShardtreeBatch, ShardtreePersistResult, SyncWarmTrees,
};

type StorageNoteType = pirate_storage_sqlite::models::NoteType;
type NullifierBytes = [u8; 32];
type TxidBytes = [u8; 32];
type TypedSpendEntry = (StorageNoteType, NullifierBytes, TxidBytes);
type RecoveredSpend = (i64, NullifierBytes, TxidBytes);
type TypedRecoveredSpend = (i64, StorageNoteType, NullifierBytes, TxidBytes);

fn verbose_note_logging_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        if cfg!(debug_assertions) {
            return true;
        }
        match env::var("PIRATE_VERBOSE_NOTE_LOGS") {
            Ok(v) => {
                let v = v.trim();
                v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
            }
            Err(_) => false,
        }
    })
}

fn verbose_sync_batch_logging_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match env::var("PIRATE_VERBOSE_SYNC_LOGS") {
        Ok(v) => {
            let v = v.trim();
            if v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("no") {
                return false;
            }
            if v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes") {
                return true;
            }
            true
        }
        // Keep detailed sync timings on by default so field logs always contain
        // enough data to diagnose frontier/witness bottlenecks. Set
        // PIRATE_VERBOSE_SYNC_LOGS=0 to disable.
        Err(_) => true,
    })
}

fn height_to_u32(height: u64) -> Result<u32> {
    u32::try_from(height)
        .map_err(|_| Error::Sync(format!("Block height {} exceeds u32::MAX", height)))
}

fn append_debug_log_line(line: &str) {
    pirate_core::debug_log::append_line(line);
}

fn append_sync_decision_log(location: &str, message: &str, data_fields: String) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = format!("{:08x}", ts);
    append_debug_log_line(&format!(
        r#"{{"id":"log_{}","timestamp":{},"location":"{}","message":"{}","data":{{{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
        id, ts, location, message, data_fields
    ));
}

// BridgeTree frontier cache replay constants removed -- ShardTree is persistent.
const SHARDTREE_PRUNING_DEPTH: usize = 1000;
const SAPLING_SHARD_HEIGHT: u8 = NOTE_COMMITMENT_TREE_DEPTH / 2;
const ORCHARD_SHARD_HEIGHT: u8 = NOTE_COMMITMENT_TREE_DEPTH / 2;
const SAPLING_TABLE_PREFIX: &str = "sapling";
const ORCHARD_TABLE_PREFIX: &str = "orchard";
/// Shard height used for subtree-addressed spendability/repair scheduling.
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
    /// Force a mini-checkpoint once this many blocks pass without any checkpoint.
    pub mini_checkpoint_max_block_gap: u64,
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
    /// Persist sync_state at least every N processed batches (unless checkpoint/end flushes first).
    pub sync_state_flush_every_batches: u32,
    /// Persist sync_state at least every N milliseconds while syncing.
    pub sync_state_flush_interval_ms: u64,
    /// Maximum number of prefetched batches to keep queued.
    pub prefetch_queue_depth: usize,
    /// Approximate byte cap for queued prefetched batches.
    pub prefetch_queue_max_bytes: u64,
}

/// Constants for retry logic
const MAX_RETRY_ATTEMPTS: u32 = 3;
const RETRY_BACKOFF_MS: u64 = 100;
// BridgeTree snapshot retention removed -- ShardTree is persistent in SQLite.
const MIN_PARALLEL_OUTPUTS: usize = 256;
const SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED: &str = "ERR_WITNESS_REPAIR_QUEUED";
const SPENDABILITY_MIN_CONFIRMATIONS: u32 = 1;
const LOW_HEIGHT_BATCH_CAP_HEIGHT: u64 = 10_000;
const LOW_HEIGHT_BATCH_MAX_BLOCKS: u64 = 1_024;
const HISTORIC_AUX_FLUSH_BLOCK_INTERVAL: u64 = 25_000;
const HISTORIC_AUX_FLUSH_INTERVAL_MS: u64 = 30_000;
const HISTORIC_SPARSE_CHECKPOINT_INTERVAL: u64 = 50_000;
const MAX_REORG_SEARCH_DEPTH: u64 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TipWitnessValidationOutcome {
    Clean,
    RepairQueued { start: u64, end_exclusive: u64 },
    Error,
}

impl Default for SyncConfig {
    fn default() -> Self {
        let is_mobile = cfg!(target_os = "android") || cfg!(target_os = "ios");
        let (
            max_parallel_decrypt,
            max_batch_memory_bytes,
            target_batch_bytes,
            min_batch_bytes,
            max_batch_bytes,
            prefetch_queue_depth,
            prefetch_queue_max_bytes,
            batch_size,
            max_batch_size,
            sync_state_flush_every_batches,
            sync_state_flush_interval_ms,
        ) = if is_mobile {
            (
                8,
                Some(100_000_000),
                8_000_000,
                2_000_000,
                16_000_000,
                2,
                16_000_000,
                2_000,
                2_000,
                3,
                1_500,
            )
        } else {
            (
                32,
                Some(500_000_000),
                128_000_000,
                16_000_000,
                256_000_000,
                4,
                384_000_000,
                4_000,
                4_000,
                6,
                5_000,
            )
        };

        Self {
            checkpoint_interval: 10_000,
            batch_size,          // Used when server recommendations disabled/unavailable.
            min_batch_size: 100, // Minimum batch size for spam blocks
            max_batch_size,      // Maximum batch size (caps server batches to prevent OOM)
            use_server_batch_recommendations: true, // Use server's ~4MB chunk recommendations (typically ~199 blocks)
            mini_checkpoint_every: 5,               // Mini-checkpoint every 5 batches
            mini_checkpoint_max_block_gap: 20_000,  // Always checkpoint at least every 20k blocks
            max_parallel_decrypt,
            lazy_memo_decode: true,
            defer_full_tx_fetch: true,
            target_batch_bytes,
            min_batch_bytes,
            max_batch_bytes,
            heavy_block_threshold_bytes: 500_000, // 500KB per block = heavy/spam (lowered for earlier detection)
            max_batch_memory_bytes,
            sync_state_flush_every_batches,
            sync_state_flush_interval_ms,
            prefetch_queue_depth,
            prefetch_queue_max_bytes,
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
    tracked_wallet_txids: HashSet<[u8; 32]>,
    /// Next Sapling commitment position (sequential counter, init from frontier)
    sapling_tree_position: Arc<RwLock<u64>>,
    /// Next Orchard commitment position (sequential counter, init from frontier)
    orchard_tree_position: Arc<RwLock<u64>>,
    /// Performance counters
    perf: Arc<PerfCounters>,
    /// Parallel trial-decryption worker pool
    decrypt_pool: Arc<rayon::ThreadPool>,
    /// Cancellation token
    cancel: CancelToken,
    /// Background full-tx enrichment limiter
    enrich_semaphore: Arc<tokio::sync::Semaphore>,
    /// Last tip height where queue-based witness integrity check completed.
    last_witness_check_height: Arc<RwLock<u64>>,
}

struct PrefetchTask {
    start: u64,
    end: u64,
    estimated_bytes: u64,
    handle: tokio::task::JoinHandle<Result<Vec<CompactBlockData>>>,
}

struct ServerBatchHintTask {
    start: u64,
    handle: tokio::task::JoinHandle<Option<u64>>,
}

#[derive(Clone, Copy, Debug)]
struct BatchTuning {
    target_bytes: u64,
    avg_block_size_estimate: u64,
    max_batch_blocks: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontierCheckpointMode {
    /// Persist a checkpoint for every processed block.
    PerBlock,
    /// Persist checkpoints only for blocks containing wallet-owned commitments.
    OwnedOnly,
}

#[derive(Clone, Copy)]
struct TreeStateRetryProfile {
    max_attempts: u32,
    base_timeout: Duration,
    timeout_step: Duration,
    max_timeout: Duration,
    initial_backoff: Duration,
    max_backoff: Duration,
    bridge_timeout_cap: Duration,
    hash_timeout_cap: Duration,
    enable_hash_fallback: bool,
    extended_timeout: Duration,
    extended_hash_timeout: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FrontierInitSource {
    None,
    LocalSnapshot,
    RemoteTreeState,
}

impl SyncEngine {
    fn is_non_retryable_fetch_error(error: &Error) -> bool {
        match error {
            Error::Status(status) => matches!(
                status.code(),
                Code::InvalidArgument
                    | Code::Unimplemented
                    | Code::FailedPrecondition
                    | Code::PermissionDenied
            ),
            Error::Sync(msg) | Error::Network(msg) | Error::Connection(msg) => {
                msg.starts_with("NON_RETRYABLE:")
            }
            _ => false,
        }
    }

    async fn server_compact_floor_hint(&self) -> Option<u64> {
        let info = tokio::time::timeout(Duration::from_secs(4), self.client.get_lightd_info())
            .await
            .ok()?
            .ok()?;
        if info.sapling_activation_height > 0 {
            Some(info.sapling_activation_height)
        } else {
            None
        }
    }

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
            tracked_wallet_txids: HashSet::new(),
            sapling_tree_position: Arc::new(RwLock::new(0)),
            orchard_tree_position: Arc::new(RwLock::new(0)),
            perf: Arc::new(PerfCounters::new()),
            decrypt_pool: Arc::new(decrypt_pool),
            cancel: CancelToken::new(),
            enrich_semaphore: Arc::new(tokio::sync::Semaphore::new(enrich_limit)),
            last_witness_check_height: Arc::new(RwLock::new(0)),
        }
    }

    fn ensure_nullifier_cache(&mut self) -> Result<()> {
        if self.nullifier_cache_loaded {
            return Ok(());
        }
        let sink = match self.storage.as_ref() {
            Some(s) => s.clone(),
            None => return Ok(()),
        };
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let repo = Repository::new(&db);
        let notes = repo.get_spend_reconciliation_notes(sink.account_id)?;
        let mut loaded = 0u64;
        for note in notes {
            let id = match note.id {
                Some(v) => v,
                None => continue,
            };
            if note.spent && note.spent_txid.is_some() {
                continue;
            }
            if note.nullifier.len() != 32 {
                continue;
            }
            let mut nf = [0u8; 32];
            nf.copy_from_slice(&note.nullifier[..32]);
            if nf.iter().all(|b| *b == 0) {
                continue;
            }
            self.nullifier_cache.insert(nf, id);
            loaded += 1;
        }
        self.nullifier_cache_loaded = true;
        pirate_core::debug_log::with_locked_file(|file| {
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
        });
        tracing::debug!("Loaded {} unspent nullifiers into cache", loaded);
        Ok(())
    }

    fn update_nullifier_cache(&mut self, entries: &[([u8; 32], i64)]) {
        for (nf, id) in entries {
            self.nullifier_cache.insert(*nf, *id);
        }
    }

    fn track_wallet_txids_from_notes(&mut self, notes: &[DecryptedNote]) {
        for note in notes {
            if note.txid.len() == 32 {
                let mut txid = [0u8; 32];
                txid.copy_from_slice(&note.txid[..32]);
                self.tracked_wallet_txids.insert(txid);
            }
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
            tracked_wallet_txids: HashSet::new(),
            sapling_tree_position: Arc::new(RwLock::new(0)),
            orchard_tree_position: Arc::new(RwLock::new(0)),
            perf: Arc::new(PerfCounters::new()),
            decrypt_pool: Arc::new(decrypt_pool),
            cancel: CancelToken::new(),
            enrich_semaphore: Arc::new(tokio::sync::Semaphore::new(enrich_limit)),
            last_witness_check_height: Arc::new(RwLock::new(0)),
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
            tracked_wallet_txids: HashSet::new(),
            sapling_tree_position: Arc::new(RwLock::new(0)),
            orchard_tree_position: Arc::new(RwLock::new(0)),
            perf: Arc::new(PerfCounters::new()),
            decrypt_pool: Arc::new(decrypt_pool),
            cancel: CancelToken::new(),
            enrich_semaphore: Arc::new(tokio::sync::Semaphore::new(enrich_limit)),
            last_witness_check_height: Arc::new(RwLock::new(0)),
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
        address_network_type: NetworkType,
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
            address_network_type,
        };
        self.storage = Some(sink);
        self.keys = key_groups;
        if let Ok(mut last) = self.last_witness_check_height.try_write() {
            *last = 0;
        }
        Ok(self)
    }

    /// Get progress reference
    pub fn progress(&self) -> Arc<RwLock<SyncProgress>> {
        Arc::clone(&self.progress)
    }

    /// Resolve the next block height that should be scanned for background work.
    pub fn background_resume_height(&self) -> Result<u64> {
        let mut start_height = self.birthday_height as u64;

        if let Some(ref sink) = self.storage {
            let stored_height = sink.load_sync_state()?.local_height;
            if stored_height > 0 {
                start_height = stored_height.saturating_add(1);
            }
        }

        Ok(start_height.max(self.birthday_height as u64))
    }

    /// Prepare background sync bounds by loading the local resume height and
    /// refreshing the remote target height immediately before the sync starts.
    pub async fn prepare_background_sync(&self) -> Result<(u64, u64)> {
        let start_height = self.background_resume_height()?;
        {
            let progress = self.progress.write().await;
            progress.set_current(start_height.saturating_sub(1));
            progress.set_stage(SyncStage::Headers);
        }
        self.update_target_height().await?;
        let target_height = self.progress.read().await.target_height();
        Ok((start_height, target_height))
    }

    /// Start sync from birthday height.
    ///
    /// ShardTree state is persistent in SQLite, so we just need the stored
    /// local height to know where to resume. Position counters are recovered
    /// in `initialize_shardtrees_for_sync`.
    pub async fn sync_from_birthday(&mut self) -> Result<()> {
        let mut start_height = self.birthday_height as u64;

        if let Some(ref sink) = self.storage {
            let stored_height = {
                let db =
                    Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
                let sync_state = SyncStateStorage::new(&db).load_sync_state()?;
                sync_state.local_height
            };

            if stored_height > 0 {
                start_height = stored_height.saturating_add(1);
            }
        }

        if start_height < self.birthday_height as u64 {
            start_height = self.birthday_height as u64;
        }

        self.sync_range(start_height, None).await
    }

    async fn validate_resume_chain(
        &mut self,
        requested_start_height: u64,
        remote_tip_height: u64,
    ) -> Result<u64> {
        let Some(sink) = self.storage.clone() else {
            return Ok(requested_start_height);
        };

        let expected_tip_height = requested_start_height.saturating_sub(1);
        if expected_tip_height == 0 {
            return Ok(requested_start_height);
        }
        if expected_tip_height > remote_tip_height {
            tracing::warn!(
                "Local resume tip {} is ahead of server tip {}; postponing reorg validation",
                expected_tip_height,
                remote_tip_height
            );
            return Ok(requested_start_height);
        }

        let (local_tip, metadata_gap) = match sink.load_chain_block(expected_tip_height)? {
            Some(block) => (block, false),
            None => match sink.load_latest_chain_block()? {
                Some(block) if block.height < expected_tip_height => {
                    tracing::warn!(
                        "Canonical block metadata stops at {}, but sync_state resumes after {}; replaying from metadata tip",
                        block.height,
                        expected_tip_height
                    );
                    (block, true)
                }
                _ => {
                    tracing::debug!(
                        "No canonical block metadata at resume tip {}; continuing without resume reorg check",
                        expected_tip_height
                    );
                    return Ok(requested_start_height);
                }
            },
        };

        if local_tip.height == 0 || local_tip.height > remote_tip_height {
            return Ok(requested_start_height);
        }

        let remote_tip_block = self
            .client
            .get_block(height_to_u32(local_tip.height)?)
            .await?;
        if remote_tip_block.hash == local_tip.hash {
            if metadata_gap {
                self.rollback_to_checkpoint(local_tip.height).await?;
                self.invalidate_block_cache_above(local_tip.height);
                return Ok(local_tip.height.saturating_add(1).max(1));
            }
            return Ok(requested_start_height);
        }

        tracing::warn!(
            "Reorg detected on resume at height {} (local={}, remote={})",
            local_tip.height,
            hex::encode(&local_tip.hash),
            hex::encode(&remote_tip_block.hash)
        );
        self.rollback_to_common_ancestor(local_tip.height).await
    }

    async fn rollback_to_common_ancestor(&mut self, divergent_height: u64) -> Result<u64> {
        let rollback_height = self
            .find_common_ancestor(divergent_height)
            .await?
            .unwrap_or_else(|| (self.birthday_height as u64).saturating_sub(1));

        self.rollback_to_checkpoint(rollback_height).await?;
        self.invalidate_block_cache_above(rollback_height);

        Ok(rollback_height.saturating_add(1).max(1))
    }

    async fn find_common_ancestor(&self, divergent_height: u64) -> Result<Option<u64>> {
        let Some(sink) = self.storage.clone() else {
            return Ok(None);
        };
        let birthday_floor = (self.birthday_height as u64).saturating_sub(1);
        let stop_height = divergent_height
            .saturating_sub(MAX_REORG_SEARCH_DEPTH)
            .max(birthday_floor);

        let mut height = divergent_height;
        loop {
            if let Some(local) = sink.load_chain_block(height)? {
                let remote = self.client.get_block(height_to_u32(height)?).await?;
                if local.hash == remote.hash {
                    tracing::info!("Found common chain ancestor at height {}", height);
                    return Ok(Some(height));
                }
            }

            if height == 0 || height <= stop_height {
                break;
            }
            height = height.saturating_sub(1);
        }

        tracing::warn!(
            "No common chain ancestor found between heights {} and {}; rolling back to wallet birthday floor",
            divergent_height,
            stop_height
        );
        Ok(None)
    }

    fn invalidate_block_cache_above(&self, height: u64) {
        match BlockCache::for_endpoint(self.client.endpoint()) {
            Ok(cache) => {
                if let Err(e) = cache.delete_above(height) {
                    tracing::debug!("Failed to invalidate block cache above {}: {}", height, e);
                }
            }
            Err(e) => tracing::debug!("Failed to open block cache for invalidation: {}", e),
        }
    }

    fn validate_batch_boundary(
        &self,
        batch_start: u64,
        blocks: &[CompactBlockData],
    ) -> Result<bool> {
        if batch_start <= 1 || blocks.is_empty() {
            return Ok(true);
        }
        let Some(sink) = self.storage.as_ref() else {
            return Ok(true);
        };
        let Some(previous) = sink.load_chain_block(batch_start.saturating_sub(1))? else {
            return Ok(true);
        };
        let first = &blocks[0];
        if first.prev_hash.len() != 32 {
            return Err(Error::Sync(format!(
                "Block {} has invalid prev_hash length {}",
                first.height,
                first.prev_hash.len()
            )));
        }
        Ok(first.prev_hash == previous.hash)
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
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
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
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
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
        pirate_core::debug_log::with_locked_file(|file| {
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
        });
        // #endregion

        // New sync ranges (including rescans) must re-run witness integrity checks
        // when they catch tip, even if the target height matches a prior session.
        {
            let mut last = self.last_witness_check_height.write().await;
            *last = 0;
        }

        // Connect to lightwalletd
        tracing::debug!("Connecting to lightwalletd...");
        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
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
        });
        // #endregion
        let connect_result = self.client.connect().await;
        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
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
        });
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
                pirate_core::debug_log::with_locked_file(|file| {
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
                });
                // #endregion
                let latest_result = self.client.get_latest_block().await;
                // #region agent log
                pirate_core::debug_log::with_locked_file(|file| {
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
                });
                // #endregion
                let latest = latest_result.map_err(|e| {
                    tracing::error!("Failed to get latest block: {:?}", e);
                    e
                })?;
                tracing::info!("Latest block height from server: {}", latest);
                latest
            }
        };

        // Validate end height.
        //
        // In follow-tip mode we cannot early-return when local resume height is ahead of
        // current server tip, because that can leave queued FoundNote repairs unprocessed.
        // Clamp to tip so the normal monitor/repair loop remains active.
        let mut effective_start_height = start_height;
        if end < start_height {
            if follow_tip {
                // The server tip hasn't advanced past our resume height yet.
                // Keep effective_start_height at resume height so the batch-fetch
                // loop is a no-op (start > end → nothing to fetch). The follow-tip
                // monitoring loop will then wait for new blocks and handle repairs.
                //
                // CRITICAL: do NOT clamp start down to `end` — that would re-fetch
                // and re-process the last block from the previous sync, double-
                // appending its commitments to the ShardTree and corrupting roots.
                tracing::info!(
                    "Local resume height {} is ahead of server tip {}; entering follow-tip monitoring without re-fetching",
                    start_height,
                    end
                );
            } else {
                tracing::warn!(
                    "Bounded sync start {} is ahead of server tip {}; entering queue/validation pass without block fetch",
                    start_height,
                    end
                );
            }
        }

        effective_start_height = self
            .validate_resume_chain(effective_start_height, end)
            .await?;

        self.ensure_nullifier_cache()?;

        // Initialize progress
        {
            let progress = self.progress.write().await;
            progress.set_target(end);
            progress.set_current(effective_start_height);
            progress.set_stage(SyncStage::Headers);
            progress.start();
            tracing::debug!(
                "Progress initialized: current={}, target={}, stage={:?}",
                effective_start_height,
                end,
                SyncStage::Headers
            );
        }

        tracing::info!(
            "Starting sync: {} -> {} ({} blocks)",
            effective_start_height,
            end,
            end.saturating_sub(effective_start_height).saturating_add(1)
        );

        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
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
                effective_start_height,
                end,
                end.saturating_sub(effective_start_height).saturating_add(1)
            );
        });
        // #endregion
        let result = self
            .sync_range_internal(effective_start_height, end, follow_tip)
            .await;
        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
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
        });
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

    /// Check whether the ShardTree already has checkpoints at or above a given height.
    fn shardtree_has_checkpoints_at_or_above(&self, height: u64) -> Result<bool> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(false);
        };
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let height_u32 = u32::try_from(height).unwrap_or(u32::MAX);
        let has: bool = db
            .conn()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sapling_tree_checkpoints WHERE checkpoint_id >= ?1)",
                [height_u32],
                |row| row.get(0),
            )
            .unwrap_or(false);
        Ok(has)
    }

    /// Initialize ShardTrees for a sync starting at `start_height`.
    ///
    /// If the ShardTree already has checkpoint data (from a previous sync), the existing
    /// state is reused and position counters are recovered from it. Otherwise the remote
    /// lightwalletd tree-state is fetched and used to seed both trees via
    /// `insert_frontier_nodes()`.
    async fn initialize_shardtrees_for_sync(
        &self,
        start_height: u64,
    ) -> Result<FrontierInitSource> {
        if start_height <= 1 {
            *self.sapling_tree_position.write().await = 0;
            *self.orchard_tree_position.write().await = 0;
            return Ok(FrontierInitSource::None);
        }

        let tree_height = start_height.saturating_sub(1);

        pirate_core::debug_log::with_locked_file(|file| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            let _ = writeln!(
                file,
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:init_shardtrees","message":"initialize shardtrees for sync","data":{{"tree_height":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                id, ts, tree_height
            );
        });

        if self.shardtree_has_checkpoints_at_or_above(tree_height)? {
            self.recover_position_counters_from_shardtree().await?;
            tracing::debug!(
                "ShardTree already has checkpoints at height {}; reusing existing state",
                tree_height
            );
            return Ok(FrontierInitSource::LocalSnapshot);
        }

        self.seed_shardtrees_from_remote(tree_height).await?;
        Ok(FrontierInitSource::RemoteTreeState)
    }

    /// Seed both ShardTrees from the remote lightwalletd tree state at the given height.
    async fn seed_shardtrees_from_remote(&self, tree_height: u64) -> Result<()> {
        let tree_state = self.fetch_tree_state_with_retry(tree_height).await?;

        let sapling_frontier = if !tree_state.sapling_frontier.is_empty() {
            match Self::parse_frontier_hex::<SaplingNode>(
                "sapling_frontier",
                &tree_state.sapling_frontier,
            ) {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!(
                        "Sapling frontier parse failed at height {}: {} -- treating as empty tree",
                        tree_height,
                        e
                    );
                    None
                }
            }
        } else if !tree_state.sapling_tree.is_empty() {
            match Self::parse_frontier_hex::<SaplingNode>("sapling_tree", &tree_state.sapling_tree)
            {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!(
                        "Sapling tree parse failed at height {}: {} -- treating as empty tree",
                        tree_height,
                        e
                    );
                    None
                }
            }
        } else {
            tracing::info!(
                "No Sapling tree data from server at height {} -- empty tree",
                tree_height
            );
            None
        };

        let orchard_hex_len = tree_state.orchard_tree.len();
        let orchard_frontier = if tree_state.orchard_tree.is_empty() {
            tracing::info!(
                "No Orchard tree data from server at height {} -- empty tree",
                tree_height
            );
            None
        } else {
            match Self::parse_frontier_hex::<MerkleHashOrchard>(
                "orchard_tree",
                &tree_state.orchard_tree,
            ) {
                Ok(f) => {
                    let root_hex = hex::encode(f.root().to_bytes());
                    tracing::info!(
                        "Orchard frontier parsed OK at height {}: hex_len={}, root={}",
                        tree_height,
                        orchard_hex_len,
                        root_hex
                    );
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_orchard_frontier_parsed","timestamp":{},"location":"sync.rs:seed_shardtrees_from_remote","message":"Orchard frontier parsed OK","data":{{"tree_height":{},"hex_len":{},"root":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis(),
                        tree_height,
                        orchard_hex_len,
                        root_hex
                    ));
                    Some(f)
                }
                Err(e) => {
                    tracing::warn!(
                        "Orchard tree parse failed at height {}: {} (hex_len={}) -- treating as empty tree",
                        tree_height, e, orchard_hex_len
                    );
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_orchard_frontier_parse_failed","timestamp":{},"location":"sync.rs:seed_shardtrees_from_remote","message":"Orchard frontier parse failed","data":{{"tree_height":{},"hex_len":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis(),
                        tree_height,
                        orchard_hex_len,
                        e
                    ));
                    None
                }
            }
        };

        let checkpoint_height = u32::try_from(tree_height).map_err(|_| {
            Error::Sync(format!(
                "Shardtree seed height {} exceeds u32::MAX",
                tree_height
            ))
        })?;
        let checkpoint_id = BlockHeight::from(checkpoint_height);

        let Some(sink) = self.storage.as_ref() else {
            return Ok(());
        };
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let tx = db.conn().unchecked_transaction().map_err(|e| {
            Error::Sync(format!("Failed to start shardtree seed transaction: {}", e))
        })?;

        // Sapling
        {
            tx.execute("DELETE FROM sapling_tree_checkpoint_marks_removed", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;
            tx.execute("DELETE FROM sapling_tree_checkpoints", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;
            tx.execute("DELETE FROM sapling_tree_shards", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;
            tx.execute("DELETE FROM sapling_tree_cap", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;

            let store = SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                &tx,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open Sapling shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);

            let sapling_nonempty = sapling_frontier.as_ref().and_then(|f| f.value());
            if let Some(nonempty) = sapling_nonempty {
                tree.insert_frontier_nodes(
                    nonempty.clone(),
                    Retention::Checkpoint {
                        id: checkpoint_id,
                        is_marked: false,
                    },
                )
                .map_err(|e| Error::Sync(format!("Failed to seed Sapling shardtree: {}", e)))?;
                let pos = u64::from(nonempty.position()) + 1;
                *self.sapling_tree_position.write().await = pos;
            } else {
                tree.checkpoint(checkpoint_id)
                    .map_err(|e| Error::Sync(format!("Sapling checkpoint failed: {}", e)))?;
                *self.sapling_tree_position.write().await = 0;
            }
        }

        // Orchard
        {
            tx.execute("DELETE FROM orchard_tree_checkpoint_marks_removed", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;
            tx.execute("DELETE FROM orchard_tree_checkpoints", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;
            tx.execute("DELETE FROM orchard_tree_shards", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;
            tx.execute("DELETE FROM orchard_tree_cap", [])
                .map_err(|e| Error::Sync(format!("Seed clear failed: {}", e)))?;

            let store =
                SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                    &tx,
                    ORCHARD_TABLE_PREFIX,
                )
                .map_err(|e| Error::Sync(format!("Failed to open Orchard shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);

            if let Some(ref frontier) = orchard_frontier {
                if let Some(nonempty) = frontier.value() {
                    tree.insert_frontier_nodes(
                        nonempty.clone(),
                        Retention::Checkpoint {
                            id: checkpoint_id,
                            is_marked: false,
                        },
                    )
                    .map_err(|e| Error::Sync(format!("Failed to seed Orchard shardtree: {}", e)))?;
                    let pos = u64::from(nonempty.position()) + 1;
                    *self.orchard_tree_position.write().await = pos;
                } else {
                    tree.checkpoint(checkpoint_id)
                        .map_err(|e| Error::Sync(format!("Orchard checkpoint failed: {}", e)))?;
                    *self.orchard_tree_position.write().await = 0;
                }
            } else {
                tree.checkpoint(checkpoint_id)
                    .map_err(|e| Error::Sync(format!("Orchard checkpoint failed: {}", e)))?;
                *self.orchard_tree_position.write().await = 0;
            }
        }

        tx.commit()
            .map_err(|e| Error::Sync(format!("Shardtree seed commit failed: {}", e)))?;

        tracing::info!(
            "Seeded ShardTrees from remote tree state at height {} (sapling_pos={}, orchard_pos={})",
            tree_height,
            *self.sapling_tree_position.read().await,
            *self.orchard_tree_position.read().await,
        );
        Ok(())
    }

    /// Recover position counters from the existing ShardTree checkpoint state.
    async fn recover_position_counters_from_shardtree(&self) -> Result<()> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(());
        };
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let conn = db.conn();

        let sapling_pos: Option<i64> = conn
            .query_row(
                "SELECT MAX(position) FROM sapling_tree_checkpoints WHERE position IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(None);
        *self.sapling_tree_position.write().await = sapling_pos
            .map(|p| (p as u64).saturating_add(1))
            .unwrap_or(0);

        let orchard_pos: Option<i64> = conn
            .query_row(
                "SELECT MAX(position) FROM orchard_tree_checkpoints WHERE position IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap_or(None);
        *self.orchard_tree_position.write().await = orchard_pos
            .map(|p| (p as u64).saturating_add(1))
            .unwrap_or(0);

        Ok(())
    }

    fn parse_frontier_hex<H>(
        label: &str,
        hex_str: &str,
    ) -> Result<incrementalmerkletree::frontier::Frontier<H, { NOTE_COMMITMENT_TREE_DEPTH }>>
    where
        H: Hashable + zcash_primitives::merkle_tree::HashSer + Clone,
    {
        let bytes = hex::decode(hex_str)
            .map_err(|e| Error::Sync(format!("Failed to decode {} bytes: {}", label, e)))?;

        // Root-only encodings (32 bytes) are not sufficient to construct a frontier.
        // Fail closed so callers can fall back to root-only handling when applicable.
        if bytes.len() == 32 {
            return Err(Error::Sync(format!(
                "{} returned root-only encoding (frontier required)",
                label
            )));
        }

        // `z_gettreestate{legacy}` returns a legacy `CommitmentTree` serialization in `finalState`.
        // Decode it via `read_commitment_tree` and then derive a `Frontier`.
        if let Ok(tree) = read_commitment_tree::<H, _, { NOTE_COMMITMENT_TREE_DEPTH }>(&bytes[..]) {
            return Ok(CommitmentTree::to_frontier(&tree));
        }

        // Fallback: some servers may provide serialized `Frontier` (v0/v1) instead.
        if let Ok(frontier) = read_frontier_v1::<H, _>(&bytes[..]) {
            return Ok(frontier);
        }

        read_frontier_v0::<H, _>(&bytes[..])
            .map_err(|e| Error::Sync(format!("Failed to parse {} frontier: {}", label, e)))
    }

    async fn check_witnesses_and_queue_rescans(
        &self,
        current_height: u64,
        db_session: Option<&Database>,
    ) -> Result<Option<(u64, u64)>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(None),
        };

        let already_checked = {
            let last = self.last_witness_check_height.read().await;
            *last >= current_height
        };

        let owned_db;
        let db = if let Some(db) = db_session {
            db
        } else {
            owned_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &owned_db
        };
        let repo = Repository::new(db);
        let spendability = SpendabilityStateStorage::new(db);
        let scan_queue = ScanQueueStorage::new(db);
        let wallet_birthday = repo
            .get_wallet_birthday_height(sink.account_id)?
            .unwrap_or(1)
            .max(1);

        let Some((target_height, computed_anchor_height)) = spendability
            .get_target_and_anchor_heights_for_account(
                SPENDABILITY_MIN_CONFIRMATIONS,
                sink.account_id,
            )?
            .map(|(target, anchor_height)| (target.max(1), anchor_height.max(1)))
        else {
            spendability.mark_sync_finalizing(0, 0)?;
            tracing::debug!(
                "Skipping witness integrity check at tip {}: scan queue extrema not available yet",
                current_height
            );
            return Ok(None);
        };

        let state = spendability.load_state().unwrap_or_default();
        if state.rescan_required {
            return Ok(None);
        }

        // If tip didn't advance and state is already validated for this anchor epoch,
        // avoid redundant checks.
        if already_checked {
            let state_ok = state.spendable
                && !state.rescan_required
                && !state.repair_queued
                && state.reason_code == "OK"
                && state.validated_anchor_height >= computed_anchor_height;
            if state_ok {
                return Ok(None);
            }
        }

        // Queue-first flow:
        // - ask storage for witness/material gaps at the fixed anchor epoch
        // - queue FoundNote ranges for normal replay worker
        // - mark spendability validated only when queue is clean
        let witness_check =
            repo.check_witnesses(sink.account_id, computed_anchor_height, wallet_birthday)?;
        if witness_check.repair_ranges.is_empty() {
            let done_through = computed_anchor_height
                .saturating_add(1)
                .max(current_height.saturating_add(1));
            let _ = scan_queue.mark_found_note_done_through(done_through);
            if let Some(next_row) = scan_queue.next_found_note_range()? {
                spendability.mark_repair_pending_without_enqueue(
                    next_row.range_start.max(1),
                    SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED,
                )?;
            } else {
                spendability.mark_validated(target_height, computed_anchor_height)?;
            }
            let mut last = self.last_witness_check_height.write().await;
            *last = (*last).max(current_height);
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            append_debug_log_line(&format!(
                r#"{{"id":"log_check_witnesses_complete","timestamp":{},"location":"sync.rs:check_witnesses_and_queue_rescans","message":"witness check complete","data":{{"current_height":{},"target_height":{},"anchor_height":{},"considered_notes":{},"done_through_exclusive":{},"next_repair_row":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts,
                current_height,
                target_height,
                computed_anchor_height,
                witness_check.considered_notes,
                done_through,
                if scan_queue.next_found_note_range()?.is_some() {
                    1
                } else {
                    0
                }
            ));
            tracing::debug!(
                "check_witnesses complete at tip {}: anchor={} considered={} missing=0",
                current_height,
                computed_anchor_height,
                witness_check.considered_notes
            );
            return Ok(None);
        }

        let mut queued_start = u64::MAX;
        let mut queued_end = computed_anchor_height.max(current_height).saturating_add(1);
        for (from_height, range_end_exclusive) in &witness_check.repair_ranges {
            let from = (*from_height).max(wallet_birthday).max(1);
            let end = (*range_end_exclusive).max(from.saturating_add(1));
            queued_start = queued_start.min(from);
            queued_end = queued_end.max(end);
            spendability.queue_repair_range(
                from,
                end,
                SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED,
            )?;
        }
        let queued_start = if queued_start == u64::MAX {
            wallet_birthday.max(1)
        } else {
            queued_start
        };
        spendability.mark_repair_pending_without_enqueue(
            queued_start,
            SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED,
        )?;
        let mut last = self.last_witness_check_height.write().await;
        *last = (*last).max(current_height);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        append_debug_log_line(&format!(
            r#"{{"id":"log_check_witnesses_queued","timestamp":{},"location":"sync.rs:check_witnesses_and_queue_rescans","message":"witness check queued repair ranges","data":{{"current_height":{},"target_height":{},"anchor_height":{},"queued_start":{},"queued_end_exclusive":{},"ranges":{},"considered_notes":{},"sapling_missing":{},"orchard_missing":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
            ts,
            current_height,
            target_height,
            computed_anchor_height,
            queued_start,
            queued_end,
            witness_check.repair_ranges.len(),
            witness_check.considered_notes,
            witness_check.sapling_missing,
            witness_check.orchard_missing
        ));
        tracing::warn!(
            "check_witnesses queued repairs at tip {}: anchor={} considered={} sapling_missing={} orchard_missing={} ranges={}",
            current_height,
            computed_anchor_height,
            witness_check.considered_notes,
            witness_check.sapling_missing,
            witness_check.orchard_missing,
            witness_check.repair_ranges.len()
        );
        Ok(Some((queued_start, queued_end)))
    }

    /// Run a tip-level witness validation pass without failing the sync task.
    ///
    /// This is used when sync exits without entering the follow-tip monitoring
    /// loop (for example bounded rescans). In those cases we still need one
    /// deterministic integrity pass so `validated_anchor_height` can advance at
    /// the current tip.
    async fn run_tip_witness_validation(
        &self,
        tip_height: u64,
        context: &'static str,
    ) -> TipWitnessValidationOutcome {
        let mut queued_start = 0u64;
        let mut queued_end_exclusive = 0u64;
        let outcome: &'static str;
        let mut error_detail = String::new();
        let result;
        match self
            .check_witnesses_and_queue_rescans(tip_height, None)
            .await
        {
            Ok(Some((repair_from_height, repair_end_exclusive))) => {
                queued_start = repair_from_height;
                queued_end_exclusive = repair_end_exclusive;
                outcome = "repair_queued";
                result = TipWitnessValidationOutcome::RepairQueued {
                    start: repair_from_height,
                    end_exclusive: repair_end_exclusive,
                };
                tracing::warn!(
                    "Tip witness validation queued FoundNote repair range {}..{} at tip {} (context={})",
                    repair_from_height,
                    repair_end_exclusive,
                    tip_height,
                    context
                );
            }
            Ok(None) => {
                outcome = "clean";
                result = TipWitnessValidationOutcome::Clean;
            }
            Err(e) => {
                outcome = "error";
                error_detail = e.to_string();
                result = TipWitnessValidationOutcome::Error;
                tracing::warn!(
                    "Tip witness validation failed at {} (context={}): {}",
                    tip_height,
                    context,
                    e
                );
            }
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        append_debug_log_line(&format!(
            r#"{{"id":"log_tip_witness_validation","timestamp":{},"location":"sync.rs:run_tip_witness_validation","message":"tip witness validation pass","data":{{"tip_height":{},"context":"{}","outcome":"{}","queued_start":{},"queued_end_exclusive":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
            ts,
            tip_height,
            context,
            outcome,
            queued_start,
            queued_end_exclusive,
            error_detail.replace('"', "'")
        ));
        result
    }

    async fn activate_queued_found_note_range(&self) -> Result<Option<(u64, u64)>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(None),
        };
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let scan_queue = ScanQueueStorage::new(&db);
        let spendability = SpendabilityStateStorage::new(&db);
        let Some(row) = scan_queue.next_found_note_range()? else {
            return Ok(None);
        };
        if row.status == "pending" {
            scan_queue.mark_in_progress(row.id)?;
        }
        let range_start = row.range_start.max(1);
        let range_end_exclusive = row.range_end.max(range_start.saturating_add(1));
        spendability.mark_repair_pending_without_enqueue(
            range_start,
            SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED,
        )?;
        // Force one post-repair integrity pass at the same tip after replay
        // finishes, so spendability can return to validated without waiting for
        // a new block.
        {
            let mut last = self.last_witness_check_height.write().await;
            let force_height = range_start.saturating_sub(1);
            if *last > force_height {
                *last = force_height;
            }
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        append_debug_log_line(&format!(
            r#"{{"id":"log_activate_repair_range","timestamp":{},"location":"sync.rs:activate_queued_found_note_range","message":"activated witness repair range","data":{{"range_start":{},"range_end_exclusive":{},"row_status":"{}","row_id":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
            ts, range_start, range_end_exclusive, row.status, row.id
        ));
        Ok(Some((range_start, range_end_exclusive)))
    }

    fn tree_state_retry_profile(&self) -> TreeStateRetryProfile {
        match self.client.transport_mode() {
            // Tor and I2P are privacy-preserving but can have higher latency and
            // occasional circuit/bootstrap jitter. Keep retries bounded so rescan
            // startup doesn't sit in Headers for multiple minutes.
            TransportMode::Tor => TreeStateRetryProfile {
                max_attempts: 3,
                base_timeout: Duration::from_secs(8),
                timeout_step: Duration::from_secs(8),
                max_timeout: Duration::from_secs(32),
                initial_backoff: Duration::from_millis(500),
                max_backoff: Duration::from_secs(2),
                bridge_timeout_cap: Duration::from_secs(24),
                hash_timeout_cap: Duration::from_secs(12),
                enable_hash_fallback: true,
                extended_timeout: Duration::from_secs(75),
                extended_hash_timeout: Duration::from_secs(45),
            },
            TransportMode::I2p => TreeStateRetryProfile {
                max_attempts: 3,
                base_timeout: Duration::from_secs(8),
                timeout_step: Duration::from_secs(8),
                max_timeout: Duration::from_secs(32),
                initial_backoff: Duration::from_millis(500),
                max_backoff: Duration::from_secs(2),
                bridge_timeout_cap: Duration::from_secs(24),
                hash_timeout_cap: Duration::from_secs(12),
                enable_hash_fallback: true,
                extended_timeout: Duration::from_secs(75),
                extended_hash_timeout: Duration::from_secs(45),
            },
            TransportMode::Socks5 => TreeStateRetryProfile {
                max_attempts: 3,
                base_timeout: Duration::from_secs(8),
                timeout_step: Duration::from_secs(8),
                max_timeout: Duration::from_secs(32),
                initial_backoff: Duration::from_millis(500),
                max_backoff: Duration::from_secs(2),
                bridge_timeout_cap: Duration::from_secs(24),
                hash_timeout_cap: Duration::from_secs(12),
                enable_hash_fallback: true,
                extended_timeout: Duration::from_secs(75),
                extended_hash_timeout: Duration::from_secs(45),
            },
            // Direct mode should remain responsive while retaining enough margin
            // for transient lightwalletd load.
            TransportMode::Direct => TreeStateRetryProfile {
                max_attempts: 3,
                base_timeout: Duration::from_secs(6),
                timeout_step: Duration::from_secs(6),
                max_timeout: Duration::from_secs(24),
                initial_backoff: Duration::from_millis(250),
                max_backoff: Duration::from_secs(1),
                bridge_timeout_cap: Duration::from_secs(14),
                hash_timeout_cap: Duration::from_secs(8),
                enable_hash_fallback: true,
                extended_timeout: Duration::from_secs(60),
                extended_hash_timeout: Duration::from_secs(30),
            },
        }
    }

    fn orchard_tree_required(&self, tree_height: u64) -> bool {
        let Ok(height_u32) = u32::try_from(tree_height) else {
            // If we somehow exceed u32 range, prefer requiring Orchard tree data.
            return true;
        };
        PirateParamsNetwork::from_type(self.network_type).is_orchard_active(height_u32)
    }

    async fn fetch_tree_state_with_retry(
        &self,
        tree_height: u64,
    ) -> Result<crate::client::TreeState> {
        let profile = self.tree_state_retry_profile();
        let max_attempts = profile.max_attempts;
        let base_timeout = profile.base_timeout;
        let timeout_step = profile.timeout_step;
        let max_timeout = profile.max_timeout;
        let max_backoff = profile.max_backoff;
        let bridge_timeout_cap = profile.bridge_timeout_cap;
        let hash_timeout_cap = profile.hash_timeout_cap;
        let enable_hash_fallback = profile.enable_hash_fallback;
        let extended_timeout = profile.extended_timeout;
        let extended_hash_timeout = profile.extended_hash_timeout;
        let mut attempt = 0u32;
        let mut backoff = profile.initial_backoff;
        let mut last_block_hash: Option<Vec<u8>> = None;
        let orchard_required = self.orchard_tree_required(tree_height);

        loop {
            attempt += 1;
            if self.is_cancelled().await {
                return Err(Error::Cancelled);
            }

            let timeout = std::cmp::min(
                base_timeout.saturating_add(timeout_step.saturating_mul(attempt.saturating_sub(1))),
                max_timeout,
            );
            let bridge_timeout = std::cmp::min(timeout, bridge_timeout_cap);
            let hash_timeout = std::cmp::min(timeout, hash_timeout_cap);

            // #region agent log
            pirate_core::debug_log::with_locked_file(|file| {
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
            });
            // #endregion

            // Try bridge first, then legacy. Running both in parallel can overload some
            // servers and cause both RPCs to time out simultaneously.
            let mut hash_err: Option<String> = None;
            let mut bridge_hash_err: Option<String> = None;
            let mut legacy_hash_err: Option<String> = None;

            let bridge_err = if orchard_required {
                let bridge_result = tokio::select! {
                    _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                    result = tokio::time::timeout(bridge_timeout, self.client.get_bridge_tree_state(tree_height)) => result,
                };

                match bridge_result {
                    Ok(Ok(state)) => return Ok(state),
                    Ok(Err(e)) => {
                        pirate_core::debug_log::with_locked_file(|file| {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let id = format!("{:08x}", ts);
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:535","message":"bridge tree state failed","data":{{"tree_height":{},"attempt":{},"error":"{:?}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                                id, ts, tree_height, attempt, e
                            );
                        });
                        Some(format!("{:?}", e))
                    }
                    Err(_) => {
                        pirate_core::debug_log::with_locked_file(|file| {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let id = format!("{:08x}", ts);
                            let _ = writeln!(
                                file,
                                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:552","message":"bridge tree state timeout","data":{{"tree_height":{},"attempt":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                                id, ts, tree_height, attempt
                            );
                        });
                        Some("timeout".to_string())
                    }
                }
            } else {
                Some("not_required".to_string())
            };

            let legacy_result = tokio::select! {
                _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                result = tokio::time::timeout(timeout, self.client.get_tree_state(tree_height)) => result,
            };

            let legacy_err = match legacy_result {
                Ok(Ok(state)) => {
                    if orchard_required && state.orchard_tree.is_empty() {
                        Some("missing_orchard_tree".to_string())
                    } else {
                        return Ok(state);
                    }
                }
                Ok(Err(e)) => Some(format!("{:?}", e)),
                Err(_) => Some("timeout".to_string()),
            };

            // Fallback: resolve block hash and retry tree-state by hash. Some servers
            // handle hash-based lookups more reliably than height-based lookups.
            let hash_lookup_attempted = enable_hash_fallback;
            if hash_lookup_attempted {
                let block_hash_result = tokio::select! {
                    _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                    result = tokio::time::timeout(hash_timeout, async {
                        let height_u32 = u32::try_from(tree_height)
                            .map_err(|_| Error::Sync(format!("Tree height {} exceeds u32 range", tree_height)))?;
                        let block = self.client.get_block(height_u32).await?;
                        Ok::<Vec<u8>, Error>(block.hash)
                    }) => result,
                };

                let block_hash = match block_hash_result {
                    Ok(Ok(hash)) if hash.len() == 32 => {
                        last_block_hash = Some(hash.clone());
                        Some(hash)
                    }
                    Ok(Ok(hash)) => {
                        hash_err = Some(format!("unexpected_hash_len_{}", hash.len()));
                        None
                    }
                    Ok(Err(e)) => {
                        hash_err = Some(format!("{:?}", e));
                        None
                    }
                    Err(_) => {
                        hash_err = Some("timeout".to_string());
                        None
                    }
                };

                if let Some(hash) = block_hash {
                    if orchard_required {
                        let bridge_hash_result = tokio::select! {
                            _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                            result = tokio::time::timeout(hash_timeout, self.client.get_bridge_tree_state_by_hash(hash.clone())) => result,
                        };

                        match bridge_hash_result {
                            Ok(Ok(state)) => return Ok(state),
                            Ok(Err(e)) => bridge_hash_err = Some(format!("{:?}", e)),
                            Err(_) => bridge_hash_err = Some("timeout".to_string()),
                        }
                    } else {
                        bridge_hash_err = Some("not_required".to_string());
                    }

                    let legacy_hash_result = tokio::select! {
                        _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                        result = tokio::time::timeout(hash_timeout, self.client.get_tree_state_by_hash(hash)) => result,
                    };

                    match legacy_hash_result {
                        Ok(Ok(state)) => {
                            if orchard_required && state.orchard_tree.is_empty() {
                                legacy_hash_err = Some("missing_orchard_tree".to_string());
                            } else {
                                return Ok(state);
                            }
                        }
                        Ok(Err(e)) => legacy_hash_err = Some(format!("{:?}", e)),
                        Err(_) => legacy_hash_err = Some("timeout".to_string()),
                    }
                }
            }

            if attempt >= max_attempts {
                // One final extended pass for slow/lightly-loaded servers at old heights.
                // This avoids endless short-timeout loops while keeping normal startup fast.
                let mut extended_bridge_hash_err: Option<String> = None;
                let mut extended_legacy_hash_err: Option<String> = None;
                let mut extended_hash_err: Option<String> = None;

                pirate_core::debug_log::with_locked_file(|file| {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    let _ = writeln!(
                        file,
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:tree_state_extended","message":"extended tree state attempt","data":{{"tree_height":{},"timeout_secs":{},"hash_timeout_secs":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                        id,
                        ts,
                        tree_height,
                        extended_timeout.as_secs(),
                        extended_hash_timeout.as_secs()
                    );
                });

                let extended_bridge_err = if orchard_required {
                    let extended_bridge = tokio::select! {
                        _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                        result = tokio::time::timeout(extended_timeout, self.client.get_bridge_tree_state(tree_height)) => result,
                    };
                    match extended_bridge {
                        Ok(Ok(state)) => return Ok(state),
                        Ok(Err(e)) => Some(format!("{:?}", e)),
                        Err(_) => Some("timeout".to_string()),
                    }
                } else {
                    Some("not_required".to_string())
                };

                let extended_legacy = tokio::select! {
                    _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                    result = tokio::time::timeout(extended_timeout, self.client.get_tree_state(tree_height)) => result,
                };
                let extended_legacy_err = match extended_legacy {
                    Ok(Ok(state)) => {
                        if orchard_required && state.orchard_tree.is_empty() {
                            Some("missing_orchard_tree".to_string())
                        } else {
                            return Ok(state);
                        }
                    }
                    Ok(Err(e)) => Some(format!("{:?}", e)),
                    Err(_) => Some("timeout".to_string()),
                };

                if enable_hash_fallback {
                    let block_hash = if let Some(hash) = last_block_hash.clone() {
                        Some(hash)
                    } else {
                        let block_hash_result = tokio::select! {
                            _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                            result = tokio::time::timeout(extended_hash_timeout, async {
                                let height_u32 = u32::try_from(tree_height)
                                    .map_err(|_| Error::Sync(format!("Tree height {} exceeds u32 range", tree_height)))?;
                                let block = self.client.get_block(height_u32).await?;
                                Ok::<Vec<u8>, Error>(block.hash)
                            }) => result,
                        };
                        match block_hash_result {
                            Ok(Ok(hash)) if hash.len() == 32 => Some(hash),
                            Ok(Ok(hash)) => {
                                extended_hash_err =
                                    Some(format!("unexpected_hash_len_{}", hash.len()));
                                None
                            }
                            Ok(Err(e)) => {
                                extended_hash_err = Some(format!("{:?}", e));
                                None
                            }
                            Err(_) => {
                                extended_hash_err = Some("timeout".to_string());
                                None
                            }
                        }
                    };

                    if let Some(hash) = block_hash {
                        if orchard_required {
                            let extended_bridge_hash = tokio::select! {
                                _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                                result = tokio::time::timeout(extended_timeout, self.client.get_bridge_tree_state_by_hash(hash.clone())) => result,
                            };
                            match extended_bridge_hash {
                                Ok(Ok(state)) => return Ok(state),
                                Ok(Err(e)) => extended_bridge_hash_err = Some(format!("{:?}", e)),
                                Err(_) => extended_bridge_hash_err = Some("timeout".to_string()),
                            }
                        } else {
                            extended_bridge_hash_err = Some("not_required".to_string());
                        }

                        let extended_legacy_hash = tokio::select! {
                            _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                            result = tokio::time::timeout(extended_timeout, self.client.get_tree_state_by_hash(hash)) => result,
                        };
                        match extended_legacy_hash {
                            Ok(Ok(state)) => {
                                if orchard_required && state.orchard_tree.is_empty() {
                                    extended_legacy_hash_err =
                                        Some("missing_orchard_tree".to_string());
                                } else {
                                    return Ok(state);
                                }
                            }
                            Ok(Err(e)) => extended_legacy_hash_err = Some(format!("{:?}", e)),
                            Err(_) => extended_legacy_hash_err = Some("timeout".to_string()),
                        }
                    }
                }

                return Err(Error::Sync(format!(
                    "Tree state fetch failed at {} after {} attempts + extended fallback (bridge: {}, legacy: {}, hash: {}, bridge_hash: {}, legacy_hash: {}, ext_bridge: {}, ext_legacy: {}, ext_hash: {}, ext_bridge_hash: {}, ext_legacy_hash: {})",
                    tree_height,
                    attempt,
                    bridge_err.unwrap_or_else(|| "unknown".to_string()),
                    legacy_err.unwrap_or_else(|| "unknown".to_string()),
                    hash_err.unwrap_or_else(|| {
                        if hash_lookup_attempted {
                            "ok".to_string()
                        } else {
                            "not_attempted".to_string()
                        }
                    }),
                    bridge_hash_err.unwrap_or_else(|| "not_attempted".to_string()),
                    legacy_hash_err.unwrap_or_else(|| "not_attempted".to_string()),
                    extended_bridge_err.unwrap_or_else(|| "not_attempted".to_string()),
                    extended_legacy_err.unwrap_or_else(|| "not_attempted".to_string()),
                    extended_hash_err.unwrap_or_else(|| "not_attempted".to_string()),
                    extended_bridge_hash_err.unwrap_or_else(|| "not_attempted".to_string()),
                    extended_legacy_hash_err.unwrap_or_else(|| "not_attempted".to_string())
                )));
            }

            // Rebuild channel/transport state before retrying to recover from transient
            // transport readiness and edge network errors.
            self.client.disconnect().await;
            let _ = self.client.connect().await;

            tokio::select! {
                _ = tokio::time::sleep(backoff) => {},
                _ = self.cancel.cancelled() => return Err(Error::Cancelled),
            }

            backoff = std::cmp::min(backoff.saturating_mul(2), max_backoff);
        }
    }

    async fn sync_range_internal(
        &mut self,
        start: u64,
        mut end: u64,
        follow_tip: bool,
    ) -> Result<()> {
        let run_db = match self.storage.as_ref() {
            Some(sink) => Some(Database::open_existing(
                &sink.db_path,
                &sink.key,
                sink.master_key.clone(),
            )?),
            None => None,
        };
        let mut warm_trees: Option<SyncWarmTrees<'_>> = None;
        let mut aux_state = Some(SyncAuxState::new(start));
        let mut historical_prefill_state: Option<HistoricalPrefillState> = None;
        let mut current_height = start;
        let mut last_checkpoint_height = start.saturating_sub(1);
        let mut last_major_checkpoint_height = start.saturating_sub(1);
        let mut batches_since_mini_checkpoint = 0u32;

        // Adaptive batch sizing for spam blocks (byte-based targets)
        let mut current_target_bytes = self.config.target_batch_bytes;
        let mut current_max_batch_blocks =
            self.config.max_batch_size.max(self.config.min_batch_size);
        let mut consecutive_fetch_failures = 0u32;
        let mut consecutive_heavy_batches = 0u32;
        let mut avg_block_size_estimate =
            (self.config.target_batch_bytes / self.config.batch_size.max(1)).max(1);
        let mut prefetch_queue: VecDeque<PrefetchTask> = VecDeque::new();
        let mut queued_prefetch_bytes: u64 = 0;
        let mut server_group_end_hint: Option<u64> = None;
        let mut pending_server_group_hint: Option<ServerBatchHintTask> = None;
        let mut batches_since_sync_state_flush: u32 = 0;
        let mut last_sync_state_flush = Instant::now();
        // Resume deterministic FoundNote repairs queued by previous runs.
        if follow_tip {
            if let Some(db) = run_db.as_ref() {
                let scan_queue = ScanQueueStorage::new(db);
                if let Ok(Some(row)) = scan_queue.next_found_note_range() {
                    if row.status == "pending" {
                        let _ = scan_queue.mark_in_progress(row.id);
                    }
                    let queued_start = row.range_start.max(1);
                    if queued_start < current_height {
                        tracing::info!(
                            "Resuming queued FoundNote repair range from {} (requested start={})",
                            queued_start,
                            start
                        );
                        current_height = queued_start;
                    }
                    let spendability = SpendabilityStateStorage::new(db);
                    let _ = spendability.mark_repair_pending_without_enqueue(
                        queued_start,
                        SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED,
                    );
                }
            }
        }

        // Reset perf counters
        self.perf.reset();

        // Reset cancellation token.
        self.cancel.reset();

        if start > 0 {
            let init_source = self.initialize_shardtrees_for_sync(start).await?;
            if matches!(init_source, FrontierInitSource::RemoteTreeState) {
                tracing::info!(
                    "ShardTrees seeded from remote tree state for sync start {}",
                    start
                );
            }
        }

        if let Some(db) = run_db.as_ref() {
            let sapling_position = *self.sapling_tree_position.read().await;
            let orchard_position = *self.orchard_tree_position.read().await;
            historical_prefill_state = Some(
                prefill_historical_subtree_roots(
                    &self.client,
                    db.conn(),
                    sapling_position,
                    orchard_position,
                    end,
                )
                .await?,
            );

            let sapling_root_backed_subtrees = historical_prefill_state
                .as_ref()
                .map(|state| state.sapling.roots_by_index.len())
                .unwrap_or(0);
            let orchard_root_backed_subtrees = historical_prefill_state
                .as_ref()
                .map(|state| state.orchard.roots_by_index.len())
                .unwrap_or(0);
            let subtree_roots_used = historical_prefill_state
                .as_ref()
                .map(HistoricalPrefillState::prefetched_any)
                .unwrap_or(false);
            let warm_cache_opt_in = warm_shardtree_cache_with_subtrees_enabled();

            if subtree_roots_used && warm_cache_opt_in {
                // The sync DB may have been rewritten from remote tree-state seeding and/or
                // subtree-root prefill above. Load the warm shardtree cache only after those
                // mutations so the in-memory cache matches persisted subtree ranges.
                warm_trees = Some(SyncWarmTrees::load(db.conn())?);
                append_sync_decision_log(
                    "sync.rs:sync_range_internal",
                    "warm shardtree cache enabled",
                    format!(
                        "\"reason\":\"subtree_roots_prefetched_and_opted_in\",\"sapling_root_backed_subtrees\":{},\"orchard_root_backed_subtrees\":{},\"sapling_prefetched\":{},\"orchard_prefetched\":{}",
                        sapling_root_backed_subtrees,
                        orchard_root_backed_subtrees,
                        historical_prefill_state
                            .as_ref()
                            .map(|state| state.sapling_prefetched)
                            .unwrap_or(0),
                        historical_prefill_state
                            .as_ref()
                            .map(|state| state.orchard_prefetched)
                            .unwrap_or(0)
                    ),
                );
            } else {
                warm_trees = None;
                let reason = if subtree_roots_used {
                    "subtree_roots_prefetched_but_cache_opt_in_disabled"
                } else {
                    "subtree_root_prefill_unavailable_or_bypassed"
                };
                tracing::info!("Disabling warm shardtree cache for this sync: {}", reason);
                append_sync_decision_log(
                    "sync.rs:sync_range_internal",
                    "warm shardtree cache disabled",
                    format!(
                        "\"reason\":\"{}\",\"sapling_root_backed_subtrees\":{},\"orchard_root_backed_subtrees\":{},\"sapling_prefetched\":{},\"orchard_prefetched\":{},\"opt_in\":{}",
                        reason,
                        sapling_root_backed_subtrees,
                        orchard_root_backed_subtrees,
                        historical_prefill_state
                            .as_ref()
                            .map(|state| state.sapling_prefetched)
                            .unwrap_or(0),
                        historical_prefill_state
                            .as_ref()
                            .map(|state| state.orchard_prefetched)
                            .unwrap_or(0),
                        warm_cache_opt_in
                    ),
                );
            }
        }

        self.cleanup_orchard_false_positives().await?;

        // Bootstrap queue extrema at sync start so anchor/target derivation
        // reflects the current known local range immediately, even before the
        // first periodic sync-state flush.
        if self.storage.is_some() {
            if let Some(db) = run_db.as_ref() {
                let scan_queue = ScanQueueStorage::new(db);
                let historic_start = (self.birthday_height as u64).max(1);
                let historic_end = current_height
                    .saturating_add(1)
                    .max(historic_start.saturating_add(1));
                let _ = scan_queue.record_historic_scanned_range(
                    historic_start,
                    historic_end,
                    Some("historic_sync_bootstrap"),
                );
                if let Some(aux) = aux_state.as_mut() {
                    aux.mark_flushed(current_height);
                }
            }
        }

        // #region agent log
        pirate_core::debug_log::with_locked_file(|file| {
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
        });
        // #endregion

        // Outer loop: Keep syncing until we're fully caught up with no new blocks
        'sync_outer: loop {
            if let Some((repair_start, repair_end_exclusive)) =
                self.activate_queued_found_note_range().await?
            {
                let repair_end_height = repair_end_exclusive.saturating_sub(1).max(repair_start);
                let rollback_target = repair_start.saturating_sub(1);
                let rollback_height = self.rollback_to_checkpoint(rollback_target).await?;
                // IMPORTANT:
                // Repairs must replay deterministically from the rollback point, regardless
                // of wallet birthday / resume height heuristics. Skipping blocks here will
                // corrupt shardtree state (missing commitments) and can lead to
                // "unknown-anchor" rejections at broadcast time.
                let replay_start = rollback_height.saturating_add(1).max(1);

                tracing::info!(
                    "Activating queued FoundNote repair range {}..{} with rollback_target={} rollback_height={} replay_start={}",
                    repair_start,
                    repair_end_exclusive,
                    rollback_target,
                    rollback_height,
                    replay_start
                );
                append_debug_log_line(&format!(
                    r#"{{"id":"log_repair_rollback","timestamp":{},"location":"sync.rs:sync_range_internal","message":"rollback before found-note replay","data":{{"repair_start":{},"repair_end_exclusive":{},"rollback_target":{},"rollback_height":{},"replay_start":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                    repair_start,
                    repair_end_exclusive,
                    rollback_target,
                    rollback_height,
                    replay_start
                ));

                current_height = replay_start;
                end = end.max(repair_end_height).max(replay_start);
                last_checkpoint_height = rollback_height;
                last_major_checkpoint_height = rollback_height;
                batches_since_mini_checkpoint = 0;
                batches_since_sync_state_flush = 0;
                last_sync_state_flush = Instant::now();
                Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
            }

            // Main sync loop: sync from current_height to end
            'sync_main: while current_height <= end {
                // Check for cancellation
                if self.is_cancelled().await {
                    tracing::warn!("Sync cancelled at height {}", current_height);
                    Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                    Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
                    return Err(Error::Cancelled);
                }

                let batch_start_time = Instant::now();
                let mut persist_ms: u128 = 0;
                let mut apply_spends_ms: u128 = 0;
                let mut tx_meta_prepare_ms: u128 = 0;
                let mut checkpoint_ms: u128 = 0;
                let mut checkpoint_written_this_batch = false;
                let mut emergency_checkpoint_requested = false;

                let prefetch_plan_start = Instant::now();
                self.fill_prefetch_queue(
                    &mut prefetch_queue,
                    &mut queued_prefetch_bytes,
                    (current_height, end),
                    BatchTuning {
                        target_bytes: current_target_bytes,
                        avg_block_size_estimate,
                        max_batch_blocks: current_max_batch_blocks,
                    },
                    (&mut server_group_end_hint, &mut pending_server_group_hint),
                )
                .await?;
                let prefetch_plan_ms = prefetch_plan_start.elapsed().as_millis();

                let PrefetchTask {
                    start: batch_start,
                    end: batch_end,
                    estimated_bytes,
                    handle,
                } = prefetch_queue.pop_front().ok_or_else(|| {
                    Error::Sync(format!(
                        "Prefetch queue unexpectedly empty at height {}",
                        current_height
                    ))
                })?;
                queued_prefetch_bytes = queued_prefetch_bytes.saturating_sub(estimated_bytes);

                // Stage 1: Fetch blocks (with retry logic)
                self.progress.write().await.set_stage(SyncStage::Headers);
                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:505","message":"fetch_blocks_with_retry start","data":{{"current_height":{},"batch_end":{},"batch_size":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        id,
                        ts,
                        batch_start,
                        batch_end,
                        batch_end - batch_start + 1
                    ));
                }
                // #endregion

                let (blocks, fetch_wait_ms) = {
                    let mut backoff = Duration::from_secs(2);
                    let max_backoff = Duration::from_secs(10);
                    let mut prefetch_handle = Some(handle);

                    loop {
                        let (blocks_res, wait_ms): (Result<Vec<CompactBlockData>>, u128) =
                            if let Some(handle) = prefetch_handle.take() {
                                let fetch_wait_start = Instant::now();
                                let mut handle = handle;
                                let res = tokio::select! {
                                    joined = &mut handle => {
                                        match joined {
                                            Ok(inner) => inner,
                                            Err(e) => Err(Error::Sync(e.to_string())),
                                        }
                                    }
                                    _ = self.cancel.cancelled() => {
                                        handle.abort();
                                        Self::abort_prefetch_queue(
                                            &mut prefetch_queue,
                                            &mut queued_prefetch_bytes,
                                        );
                                        Self::abort_pending_server_batch_hint(
                                            &mut pending_server_group_hint,
                                        );
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
                                    self.wallet_id.clone(),
                                )
                                .await;
                                let wait_ms = fetch_wait_start.elapsed().as_millis();
                                (res, wait_ms)
                            };

                        match blocks_res {
                            Ok(blocks) => break (blocks, wait_ms),
                            Err(Error::Cancelled) => {
                                Self::abort_prefetch_queue(
                                    &mut prefetch_queue,
                                    &mut queued_prefetch_bytes,
                                );
                                Self::abort_pending_server_batch_hint(
                                    &mut pending_server_group_hint,
                                );
                                return Err(Error::Cancelled);
                            }
                            Err(e) => {
                                let ts = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis();
                                let id = format!("{:08x}", ts);
                                append_debug_log_line(&format!(
                                    r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_blocks","message":"fetch batch error","data":{{"start":{},"end":{},"error":"{}","non_retryable":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                                    id,
                                    ts,
                                    batch_start,
                                    batch_end,
                                    format!("{}", e).replace('"', "'"),
                                    Self::is_non_retryable_fetch_error(&e)
                                ));

                                if Self::is_non_retryable_fetch_error(&e) {
                                    if batch_start <= LOW_HEIGHT_BATCH_CAP_HEIGHT {
                                        if let Some(floor) = self.server_compact_floor_hint().await
                                        {
                                            if floor > batch_start && floor <= end {
                                                tracing::warn!(
                                                    "Non-retryable block-range failure at {}-{}; jumping to server compact floor {}",
                                                    batch_start,
                                                    batch_end,
                                                    floor
                                                );
                                                let ts = std::time::SystemTime::now()
                                                    .duration_since(std::time::UNIX_EPOCH)
                                                    .unwrap_or_default()
                                                    .as_millis();
                                                let id = format!("{:08x}", ts);
                                                append_debug_log_line(&format!(
                                                    r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_blocks","message":"apply compact floor fallback","data":{{"start":{},"end":{},"floor":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                                                    id, ts, batch_start, batch_end, floor
                                                ));

                                                Self::abort_prefetch_queue(
                                                    &mut prefetch_queue,
                                                    &mut queued_prefetch_bytes,
                                                );
                                                Self::abort_pending_server_batch_hint(
                                                    &mut pending_server_group_hint,
                                                );
                                                server_group_end_hint = None;
                                                current_height = floor;
                                                {
                                                    let progress = self.progress.write().await;
                                                    progress.set_stage(SyncStage::Headers);
                                                    progress.set_current(
                                                        current_height.saturating_sub(1),
                                                    );
                                                }
                                                continue 'sync_main;
                                            }
                                        }
                                    }

                                    return Err(Error::Sync(format!(
                                        "NON_RETRYABLE: block fetch failed for {}-{}: {}",
                                        batch_start, batch_end, e
                                    )));
                                }

                                let batch_blocks =
                                    batch_end.saturating_sub(batch_start).saturating_add(1);
                                if batch_blocks > self.config.min_batch_size {
                                    consecutive_fetch_failures =
                                        consecutive_fetch_failures.saturating_add(1);
                                    let reduced_blocks = std::cmp::max(
                                        self.config.min_batch_size,
                                        batch_blocks.saturating_add(1) / 2,
                                    );
                                    current_max_batch_blocks =
                                        current_max_batch_blocks.min(reduced_blocks);
                                    current_target_bytes = current_target_bytes.min(
                                        avg_block_size_estimate
                                            .saturating_mul(reduced_blocks)
                                            .max(1),
                                    );
                                    tracing::warn!(
                                        "Reducing sync batch size after retryable fetch failure for {}-{}: max_blocks={}, target_bytes={}, consecutive_failures={}",
                                        batch_start,
                                        batch_end,
                                        current_max_batch_blocks,
                                        current_target_bytes,
                                        consecutive_fetch_failures
                                    );
                                    append_debug_log_line(&format!(
                                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_blocks","message":"adaptive batch reduction","data":{{"start":{},"end":{},"old_blocks":{},"new_max_blocks":{},"target_bytes":{},"consecutive_failures":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                                        id,
                                        ts,
                                        batch_start,
                                        batch_end,
                                        batch_blocks,
                                        current_max_batch_blocks,
                                        current_target_bytes,
                                        consecutive_fetch_failures,
                                        format!("{}", e).replace('"', "'")
                                    ));
                                    self.client.disconnect().await;
                                    let _ = self.client.connect().await;
                                    Self::abort_prefetch_queue(
                                        &mut prefetch_queue,
                                        &mut queued_prefetch_bytes,
                                    );
                                    Self::abort_pending_server_batch_hint(
                                        &mut pending_server_group_hint,
                                    );
                                    server_group_end_hint = None;
                                    continue 'sync_main;
                                }

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
                                    _ = self.cancel.cancelled() => {
                                        Self::abort_prefetch_queue(
                                            &mut prefetch_queue,
                                            &mut queued_prefetch_bytes,
                                        );
                                        Self::abort_pending_server_batch_hint(
                                            &mut pending_server_group_hint,
                                        );
                                        return Err(Error::Cancelled);
                                    },
                                }
                                backoff = std::cmp::min(backoff.saturating_mul(2), max_backoff);
                                // Retry using a direct fetch (no prefetch).
                                continue;
                            }
                        }
                    }
                };

                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:506","message":"fetch_blocks_with_retry result","data":{{"current_height":{},"batch_end":{},"blocks_count":{},"wait_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        id,
                        ts,
                        batch_start,
                        batch_end,
                        blocks.len(),
                        fetch_wait_ms
                    ));
                }
                // #endregion

                if blocks.is_empty() {
                    return Err(Error::Sync(format!(
                        "lightwalletd returned empty compact block batch for {}-{}",
                        batch_start, batch_end
                    )));
                }

                if !self.validate_batch_boundary(batch_start, &blocks)? {
                    tracing::warn!(
                        "Reorg detected at batch boundary before height {}; rolling back to common ancestor",
                        batch_start
                    );
                    let resume_height = self
                        .rollback_to_common_ancestor(batch_start.saturating_sub(1))
                        .await?;
                    current_height = resume_height;
                    last_checkpoint_height = resume_height.saturating_sub(1);
                    last_major_checkpoint_height = resume_height.saturating_sub(1);
                    batches_since_mini_checkpoint = 0;
                    batches_since_sync_state_flush = 0;
                    Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                    Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
                    server_group_end_hint = None;
                    continue 'sync_main;
                }
                consecutive_fetch_failures = 0;

                // Detect heavy/spam blocks and adapt batch size
                // Count actual bytes in outputs and actions
                let batch_sizing_start = Instant::now();
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

                    // Request an extra checkpoint for this batch once frontier updates finish.
                    // Checkpointing before commitment append would persist a stale tree state.
                    if consecutive_heavy_batches >= 2 {
                        emergency_checkpoint_requested = true;
                    }
                } else {
                    // Reset counter and gradually increase batch size back to normal
                    consecutive_heavy_batches = 0;
                    if current_max_batch_blocks < self.config.max_batch_size {
                        let bump =
                            std::cmp::max(self.config.min_batch_size, current_max_batch_blocks / 4);
                        current_max_batch_blocks = std::cmp::min(
                            self.config.max_batch_size,
                            current_max_batch_blocks.saturating_add(bump),
                        );
                    }
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
                let batch_sizing_ms = batch_sizing_start.elapsed().as_millis();

                // Prefetch next batch while we process this one.
                let next_prefetch_start = Instant::now();
                self.fill_prefetch_queue(
                    &mut prefetch_queue,
                    &mut queued_prefetch_bytes,
                    (batch_end.saturating_add(1), end),
                    BatchTuning {
                        target_bytes: current_target_bytes,
                        avg_block_size_estimate,
                        max_batch_blocks: current_max_batch_blocks,
                    },
                    (&mut server_group_end_hint, &mut pending_server_group_hint),
                )
                .await?;
                let next_prefetch_ms = next_prefetch_start.elapsed().as_millis();

                // Stage 2: Trial decryption (batched with parallelism)
                self.progress.write().await.set_stage(SyncStage::Notes);
                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:846","message":"trial_decrypt start","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        current_height,
                        batch_end,
                        blocks.len()
                    ));
                }
                // #endregion
                let decrypt_start = Instant::now();
                let (mut notes, decrypt_telemetry) = self.trial_decrypt_batch(&blocks).await?;
                let decrypt_cpu_ms = decrypt_telemetry.cpu_ms;
                let decrypt_ms = decrypt_start.elapsed().as_millis();
                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:852","message":"trial_decrypt done","data":{{"start":{},"end":{},"notes":{},"ms":{},"cpu_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        current_height,
                        batch_end,
                        notes.len(),
                        decrypt_ms,
                        decrypt_cpu_ms
                    ));
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
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:862","message":"update_frontier start","data":{{"start":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id, ts, current_height, batch_end
                    ));
                }
                // #endregion
                let frontier_start = Instant::now();
                let remaining_to_tip = end.saturating_sub(batch_end);
                // The Zcash reference creates a checkpoint for EVERY block
                // (embedded in the last commitment's Retention::Checkpoint).
                // We must do the same for at least the recent window so that
                // any anchor height used for spending has a real checkpoint.
                //
                // Without per-block checkpoints, a rescan (follow_tip=false)
                // only checkpoints owned-note heights, leaving the anchor
                // height (tip - min_confirmations) without a checkpoint.
                // This causes "unknown-anchor" on the first send after rescan.
                //
                // Use PerBlock for the last SHARDTREE_PRUNING_DEPTH blocks
                // regardless of follow_tip. Old per-block checkpoints are
                // pruned by the ShardTree automatically.
                let checkpoint_mode = if remaining_to_tip <= SHARDTREE_PRUNING_DEPTH as u64 {
                    FrontierCheckpointMode::PerBlock
                } else {
                    FrontierCheckpointMode::OwnedOnly
                };
                let (commitments_applied, position_mappings, frontier_checkpointed_batch_end) =
                    self.update_commitment_trees(
                        &blocks,
                        &notes,
                        checkpoint_mode,
                        warm_trees.as_mut(),
                        historical_prefill_state.as_mut(),
                    )
                    .await?;
                let frontier_ms = frontier_start.elapsed().as_millis();
                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:866","message":"update_frontier done","data":{{"start":{},"end":{},"commitments":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id, ts, current_height, batch_end, commitments_applied, frontier_ms
                    ));
                }
                // #endregion
                let note_post_start = Instant::now();
                if !notes.is_empty() {
                    self.apply_positions(&mut notes, &position_mappings).await;
                    self.apply_sapling_nullifiers(&mut notes, &position_mappings)
                        .await?;
                }

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
                        pirate_core::debug_log::with_locked_file(|file| {
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
                        });
                    }
                }
                let note_post_ms = note_post_start.elapsed().as_millis();

                if frontier_checkpointed_batch_end {
                    checkpoint_written_this_batch = true;
                    last_checkpoint_height = batch_end;
                    let progress = self.progress.write().await;
                    progress.set_checkpoint(batch_end);
                }

                // Persist decrypted notes if storage is configured (after frontier update to get positions)
                if let Some(ref sink) = self.storage {
                    if notes.is_empty() {
                        persist_ms = 0;
                    } else {
                        let persist_start = Instant::now();
                        // #region agent log
                        if verbose_sync_batch_logging_enabled() {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let id = format!("{:08x}", ts);
                            append_debug_log_line(&format!(
                                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:881","message":"persist_notes start","data":{{"start":{},"end":{},"notes":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                id,
                                ts,
                                current_height,
                                batch_end,
                                notes.len()
                            ));
                        }
                        // #endregion
                        // Build txid->block_time map for this batch to persist accurate confirmation timestamps.
                        let tx_meta_prepare_start = Instant::now();
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
                        tx_meta_prepare_ms = tx_meta_prepare_start.elapsed().as_millis();

                        let persist_result = if let Some(db) = run_db.as_ref() {
                            sink.persist_notes_with_db(
                                db,
                                &notes,
                                &tx_times,
                                &tx_fees,
                                &position_mappings,
                            )?
                        } else {
                            sink.persist_notes(&notes, &tx_times, &tx_fees, &position_mappings)?
                        };
                        if !persist_result.inserted.is_empty() {
                            self.update_nullifier_cache(&persist_result.inserted);
                        }
                        if !persist_result.remove_from_cache.is_empty() {
                            for nf in &persist_result.remove_from_cache {
                                self.nullifier_cache.remove(nf);
                            }
                        }
                        self.track_wallet_txids_from_notes(&notes);
                        persist_ms = persist_start.elapsed().as_millis();
                        // #region agent log
                        if verbose_sync_batch_logging_enabled() {
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let id = format!("{:08x}", ts);
                            append_debug_log_line(&format!(
                                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:900","message":"persist_notes done","data":{{"start":{},"end":{},"notes":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                                id,
                                ts,
                                current_height,
                                batch_end,
                                notes.len(),
                                persist_ms
                            ));
                        }
                        // #endregion
                    }
                }

                if !(blocks.is_empty()
                    || (self.nullifier_cache.is_empty() && self.tracked_wallet_txids.is_empty()))
                {
                    // #region agent log
                    if verbose_sync_batch_logging_enabled() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        append_debug_log_line(&format!(
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:906","message":"apply_spends start","data":{{"start":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                            id, ts, current_height, batch_end
                        ));
                    }
                    // #endregion
                    let apply_start = Instant::now();
                    self.apply_spends(&blocks, run_db.as_ref()).await?;
                    apply_spends_ms = apply_start.elapsed().as_millis();
                    // #region agent log
                    if verbose_sync_batch_logging_enabled() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        append_debug_log_line(&format!(
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:909","message":"apply_spends done","data":{{"start":{},"end":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                            id, ts, current_height, batch_end, apply_spends_ms
                        ));
                    }
                    // #endregion
                }

                if let (Some(sink), Some(db)) = (self.storage.as_ref(), run_db.as_ref()) {
                    sink.save_chain_blocks_with_db(db, &blocks)?;
                }

                if self.config.defer_full_tx_fetch && !notes.is_empty() {
                    self.spawn_background_enrich(notes.clone(), require_memos);
                }

                // Record processing-only duration up to this point (legacy batch_total basis).
                let batch_processing_ms = batch_start_time.elapsed().as_millis();

                // Update progress with perf metrics
                let perf_progress_start = Instant::now();
                self.perf.record_batch(
                    blocks.len() as u64,
                    notes.len() as u64,
                    commitments_applied,
                    batch_processing_ms as u64,
                );
                {
                    let progress = self.progress.write().await;
                    progress.set_current(batch_end);
                    progress.update_eta();
                    progress.record_batch(
                        notes.len() as u64,
                        commitments_applied,
                        batch_processing_ms as u64,
                    );
                }
                let perf_progress_ms = perf_progress_start.elapsed().as_millis();
                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    let progress = self.progress.read().await;
                    let wallet_id = self.wallet_id.as_deref().unwrap_or("unknown");
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:664","message":"progress updated","data":{{"current_height":{},"target_height":{},"percent":{:.2},"stage":"{:?}","wallet_id":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"F"}}"#,
                        id,
                        ts,
                        progress.current_height(),
                        progress.target_height(),
                        progress.percentage(),
                        progress.stage(),
                        wallet_id
                    ));
                }
                // #endregion

                if emergency_checkpoint_requested {
                    if !checkpoint_written_this_batch {
                        let emergency_checkpoint_start = Instant::now();
                        self.create_checkpoint(batch_end, warm_trees.as_mut())
                            .await?;
                        checkpoint_ms += emergency_checkpoint_start.elapsed().as_millis();
                        checkpoint_written_this_batch = true;
                    }
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

                batches_since_mini_checkpoint += 1;
                let blocks_since_major_checkpoint = batch_end - last_major_checkpoint_height;
                let blocks_since_last_checkpoint = batch_end.saturating_sub(last_checkpoint_height);
                let wallet_activity = !notes.is_empty() || persist_ms > 0 || apply_spends_ms > 0;

                // Mini-checkpoint every N batches
                if batches_since_mini_checkpoint >= self.config.mini_checkpoint_every
                    && (wallet_activity
                        || blocks_since_last_checkpoint
                            >= self.config.mini_checkpoint_max_block_gap)
                {
                    if !checkpoint_written_this_batch {
                        let mini_checkpoint_start = Instant::now();
                        self.create_checkpoint(batch_end, warm_trees.as_mut())
                            .await?;
                        checkpoint_ms += mini_checkpoint_start.elapsed().as_millis();
                        checkpoint_written_this_batch = true;
                    }
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
                let major_checkpoint_interval = if remaining_to_tip > SHARDTREE_PRUNING_DEPTH as u64
                {
                    HISTORIC_SPARSE_CHECKPOINT_INTERVAL.max(self.config.checkpoint_interval as u64)
                } else {
                    self.config.checkpoint_interval as u64
                };
                if blocks_since_major_checkpoint >= major_checkpoint_interval {
                    if !checkpoint_written_this_batch {
                        let major_checkpoint_start = Instant::now();
                        self.create_checkpoint(batch_end, warm_trees.as_mut())
                            .await?;
                        checkpoint_ms += major_checkpoint_start.elapsed().as_millis();
                        checkpoint_written_this_batch = true;
                    }
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
                let should_flush_sync_state = checkpoint_written_this_batch
                    || batch_end >= end
                    || batches_since_sync_state_flush >= self.config.sync_state_flush_every_batches
                    || last_sync_state_flush.elapsed().as_millis()
                        >= self.config.sync_state_flush_interval_ms as u128;
                let sync_state_ms = if should_flush_sync_state {
                    if let (Some(db), Some(trees)) = (run_db.as_ref(), warm_trees.take()) {
                        warm_trees = Some(trees.flush_and_reload(db.conn())?);
                    }
                    let include_aux_state_update = aux_state.as_ref().is_some_and(|aux| {
                        aux.should_flush(
                            batch_end,
                            checkpoint_written_this_batch,
                            batch_end >= end,
                            !follow_tip,
                        )
                    });
                    let sync_state_start = Instant::now();
                    self.save_sync_state(
                        batch_end,
                        end,
                        last_checkpoint_height,
                        include_aux_state_update,
                        run_db.as_ref(),
                    )
                    .await?;
                    let elapsed_ms = sync_state_start.elapsed().as_millis();
                    batches_since_sync_state_flush = 0;
                    last_sync_state_flush = Instant::now();
                    if include_aux_state_update {
                        if let Some(aux) = aux_state.as_mut() {
                            aux.mark_flushed(batch_end);
                        }
                    }
                    elapsed_ms
                } else {
                    batches_since_sync_state_flush =
                        batches_since_sync_state_flush.saturating_add(1);
                    0
                };

                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    let wallet_id = self.wallet_id.as_deref().unwrap_or("unknown");
                    let avg_block_size = total_block_size / blocks.len().max(1) as u64;
                    let known_processing_ms = fetch_wait_ms
                        + decrypt_ms
                        + frontier_ms
                        + persist_ms
                        + apply_spends_ms
                        + prefetch_plan_ms
                        + batch_sizing_ms
                        + next_prefetch_ms
                        + note_post_ms
                        + tx_meta_prepare_ms;
                    let residual_processing_other_ms =
                        batch_processing_ms.saturating_sub(known_processing_ms);
                    let batch_full_ms = batch_start_time.elapsed().as_millis();
                    let known_full_ms =
                        known_processing_ms + perf_progress_ms + checkpoint_ms + sync_state_ms;
                    let residual_full_other_ms = batch_full_ms.saturating_sub(known_full_ms);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:915","message":"batch_stage_timing","data":{{"wallet_id":"{}","start":{},"end":{},"blocks":{},"notes":{},"total_bytes":{},"avg_block_bytes":{},"fetch_wait_ms":{},"decrypt_ms":{},"decrypt_cpu_ms":{},"frontier_ms":{},"persist_ms":{},"apply_spends_ms":{},"prefetch_plan_ms":{},"batch_sizing_ms":{},"next_prefetch_ms":{},"note_post_ms":{},"tx_meta_prepare_ms":{},"perf_progress_ms":{},"checkpoint_ms":{},"sync_state_ms":{},"residual_processing_other_ms":{},"residual_full_other_ms":{},"batch_total_ms":{},"batch_full_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        wallet_id,
                        current_height,
                        batch_end,
                        blocks.len(),
                        notes.len(),
                        total_block_size,
                        avg_block_size,
                        fetch_wait_ms,
                        decrypt_ms,
                        decrypt_cpu_ms,
                        frontier_ms,
                        persist_ms,
                        apply_spends_ms,
                        prefetch_plan_ms,
                        batch_sizing_ms,
                        next_prefetch_ms,
                        note_post_ms,
                        tx_meta_prepare_ms,
                        perf_progress_ms,
                        checkpoint_ms,
                        sync_state_ms,
                        residual_processing_other_ms,
                        residual_full_other_ms,
                        batch_processing_ms,
                        batch_full_ms
                    ));
                }
                // #endregion

                current_height = batch_end + 1;
                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:709","message":"current_height updated","data":{{"new_current_height":{},"end":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        id, ts, current_height, end
                    ));
                }
                // #endregion

                // When the just-processed batch reaches the known target tip,
                // run witness integrity immediately instead of waiting for the
                // follow-tip monitor loop. This shortens the "sync finalizing"
                // window after percent=100.
                if follow_tip && current_height > end {
                    let tip_height = current_height.saturating_sub(1);

                    if tip_height > last_checkpoint_height {
                        match self
                            .create_checkpoint(tip_height, warm_trees.as_mut())
                            .await
                        {
                            Ok(()) => {
                                if let (Some(db), Some(trees)) =
                                    (run_db.as_ref(), warm_trees.take())
                                {
                                    warm_trees = Some(trees.flush_and_reload(db.conn())?);
                                }
                                last_checkpoint_height = tip_height;
                                let progress = self.progress.write().await;
                                progress.set_checkpoint(tip_height);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to persist immediate tip checkpoint {} before witness integrity check: {}",
                                    tip_height,
                                    e
                                );
                            }
                        }
                    }

                    match self
                        .check_witnesses_and_queue_rescans(tip_height, run_db.as_ref())
                        .await
                    {
                        Ok(Some((repair_from_height, repair_end_exclusive))) => {
                            tracing::warn!(
                                "Immediate tip witness integrity queued FoundNote repair range {}..{} at tip {}; scheduling queue-driven replay",
                                repair_from_height,
                                repair_end_exclusive,
                                tip_height
                            );
                            continue 'sync_outer;
                        }
                        Ok(None) => {
                            let progress = self.progress.write().await;
                            if progress.current_height() >= progress.target_height() {
                                progress.set_stage(SyncStage::Verify);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Immediate tip witness integrity check failed at tip {}: {}",
                                tip_height,
                                e
                            );
                        }
                    }
                }
            }

            if let Some(state) = historical_prefill_state.as_mut() {
                let mut pending_batches = Vec::new();
                merge_emitted_batches(
                    &mut pending_batches,
                    drain_historical_skip_state(&mut state.sapling, append_sapling_leaf),
                );
                merge_emitted_batches(
                    &mut pending_batches,
                    drain_historical_skip_state(&mut state.orchard, append_orchard_leaf),
                );
                if !pending_batches.is_empty() {
                    if let Some(trees) = warm_trees.as_mut() {
                        let _ = trees.persist_batches(&pending_batches, None)?;
                    } else {
                        let _ = self.persist_shardtree_batches(&pending_batches, None)?;
                    }
                }
            }

            // For bounded ranges (e.g. witness repair replay), persist a final
            // frontier checkpoint before returning so anchor-hydrated selection
            // can immediately use the repaired range.
            //
            // Without this forced checkpoint, short replay runs can finish before
            // periodic mini/major checkpoint thresholds are met, leaving send
            // selection with stale snapshot coverage.
            if !follow_tip {
                if let Err(e) = self.create_checkpoint(end, warm_trees.as_mut()).await {
                    tracing::warn!(
                        "Failed to persist bounded-range final checkpoint at {}: {}",
                        end,
                        e
                    );
                } else if let (Some(db), Some(trees)) = (run_db.as_ref(), warm_trees.take()) {
                    warm_trees = Some(trees.flush_and_reload(db.conn())?);
                }
                match self
                    .run_tip_witness_validation(end, "bounded_sync_complete")
                    .await
                {
                    TipWitnessValidationOutcome::RepairQueued {
                        start,
                        end_exclusive,
                    } => {
                        tracing::warn!(
                            "Bounded sync queued FoundNote repair range {}..{} at tip {}; continuing bounded replay loop",
                            start,
                            end_exclusive,
                            end
                        );
                        continue 'sync_outer;
                    }
                    TipWitnessValidationOutcome::Clean | TipWitnessValidationOutcome::Error => {}
                }
                Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
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
                Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
                return Err(Error::Cancelled);
            }

            // Always give witness integrity a chance to converge while monitoring.
            //
            // This follows the "queue-first" model: if spendability is still
            // finalizing (or was downgraded by a prior run), we need a deterministic
            // path to re-validate at the current tip even when the tip height hasn't
            // advanced.
            //
            // The integrity check itself will no-op quickly when the wallet is already
            // validated for the current anchor epoch.
            // Ensure witness checks run against a tip-fresh frontier snapshot.
            // Without this, checks can repeatedly flag false "missing witness" notes
            // when the latest persisted checkpoint is behind the current tip.
            if current > last_checkpoint_height {
                match self.create_checkpoint(current, warm_trees.as_mut()).await {
                    Ok(()) => {
                        if let (Some(db), Some(trees)) = (run_db.as_ref(), warm_trees.take()) {
                            warm_trees = Some(trees.flush_and_reload(db.conn())?);
                        }
                        last_checkpoint_height = current;
                        let progress = self.progress.write().await;
                        progress.set_checkpoint(current);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to persist tip checkpoint {} before witness integrity check: {}",
                            current,
                            e
                        );
                    }
                }
            }

            match self
                .check_witnesses_and_queue_rescans(current, run_db.as_ref())
                .await
            {
                Ok(Some((repair_from_height, repair_end_exclusive))) => {
                    tracing::warn!(
                        "Witness integrity queued FoundNote repair range {}..{} at tip {}; scheduling queue-driven replay",
                        repair_from_height,
                        repair_end_exclusive,
                        current
                    );
                    continue 'sync_outer;
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("Witness integrity check failed at tip {}: {}", current, e);
                }
            }

            match self.client.get_latest_block().await {
                Ok(latest_height) => {
                    let validated_start = self
                        .validate_resume_chain(current.saturating_add(1), latest_height)
                        .await?;
                    if validated_start <= current {
                        tracing::warn!(
                            "Reorg detected while monitoring tip {}; resuming from {}",
                            current,
                            validated_start
                        );
                        current_height = validated_start;
                        end = latest_height.max(validated_start);
                        last_checkpoint_height = validated_start.saturating_sub(1);
                        last_major_checkpoint_height = validated_start.saturating_sub(1);
                        batches_since_mini_checkpoint = 0;
                        batches_since_sync_state_flush = 0;
                        Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                        Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
                        server_group_end_hint = None;
                        continue 'sync_outer;
                    }

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
                        // `current` is progress.current_height() which stores
                        // the last *processed* block (batch_end). The sync loop
                        // must start at the NEXT unprocessed block to avoid
                        // double-appending commitments into the ShardTree.
                        current_height = current.saturating_add(1);
                        // Reset batch tracking for the new range
                        batches_since_mini_checkpoint = 0;
                        // Re-enter the outer loop (which will re-enter the main sync loop)
                        continue; // Continue outer loop to re-enter main sync loop
                    }

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
                        _ = self.cancel.cancelled() => {
                            Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                            Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
                            return Err(Error::Cancelled);
                        },
                    }
                    // Continue the outer loop to check again
                    continue 'sync_outer;
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to check for new blocks after sync: {}, reconnecting and retrying in 5s",
                        e
                    );
                    self.client.disconnect().await;
                    if let Err(conn_err) = self.client.connect().await {
                        tracing::warn!("Reconnect failed: {}", conn_err);
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                        _ = self.cancel.cancelled() => {
                            Self::abort_prefetch_queue(&mut prefetch_queue, &mut queued_prefetch_bytes);
                            Self::abort_pending_server_batch_hint(&mut pending_server_group_hint);
                            return Err(Error::Cancelled);
                        },
                    }
                    continue; // Retry
                }
            }
        }
    }

    fn validate_compact_block_range(
        start: u64,
        end: u64,
        blocks: &[CompactBlockData],
    ) -> Result<()> {
        let expected_blocks = end.saturating_sub(start).saturating_add(1) as usize;
        if blocks.len() != expected_blocks {
            return Err(Error::Sync(format!(
                "compact block range {}-{} returned {} blocks, expected {}",
                start,
                end,
                blocks.len(),
                expected_blocks
            )));
        }

        let mut previous_hash: Option<&[u8]> = None;
        for (index, block) in blocks.iter().enumerate() {
            let expected_height = start.saturating_add(index as u64);
            if block.height != expected_height {
                return Err(Error::Sync(format!(
                    "compact block range {}-{} returned height {} at index {}, expected {}",
                    start, end, block.height, index, expected_height
                )));
            }
            if block.hash.len() != 32 {
                return Err(Error::Sync(format!(
                    "compact block {} has invalid hash length {}",
                    block.height,
                    block.hash.len()
                )));
            }
            if block.prev_hash.len() != 32 {
                return Err(Error::Sync(format!(
                    "compact block {} has invalid prev_hash length {}",
                    block.height,
                    block.prev_hash.len()
                )));
            }
            if let Some(previous_hash) = previous_hash {
                if block.prev_hash.as_slice() != previous_hash {
                    return Err(Error::Sync(format!(
                        "compact block {} prev_hash does not match previous block hash",
                        block.height
                    )));
                }
            }
            previous_hash = Some(&block.hash);
        }

        Ok(())
    }

    async fn cached_blocks_are_canonical(
        client: &LightClient,
        cache: &BlockCache,
        start: u64,
        end: u64,
        blocks: &[CompactBlockData],
    ) -> Result<bool> {
        if let Err(e) = Self::validate_compact_block_range(start, end, blocks) {
            tracing::warn!(
                "Invalid cached compact block range {}-{}; invalidating cache: {}",
                start,
                end,
                e
            );
            let _ = cache.delete_range(start, end);
            return Ok(false);
        }

        let Some(last) = blocks.last() else {
            let _ = cache.delete_range(start, end);
            return Ok(false);
        };

        match client.get_block(height_to_u32(end)?).await {
            Ok(remote) if remote.hash == last.hash => Ok(true),
            Ok(remote) => {
                tracing::warn!(
                    "Cached compact block {} is stale (cache={}, remote={}); invalidating {}-{}",
                    end,
                    hex::encode(&last.hash),
                    hex::encode(&remote.hash),
                    start,
                    end
                );
                let _ = cache.delete_range(start, end);
                Ok(false)
            }
            Err(e) => {
                tracing::warn!(
                    "Could not validate cached compact blocks {}-{} against remote: {}; refetching",
                    start,
                    end,
                    e
                );
                let _ = cache.delete_range(start, end);
                Ok(false)
            }
        }
    }

    async fn fetch_blocks_with_retry_inner(
        client: LightClient,
        start: u64,
        end: u64,
        cancel: CancelToken,
        wallet_id: Option<String>,
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
                    if verbose_sync_batch_logging_enabled() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        append_debug_log_line(&format!(
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache hit","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                            id,
                            ts,
                            start,
                            end,
                            blocks.len()
                        ));
                    }
                    if Self::cached_blocks_are_canonical(&client, &cache, start, end, &blocks)
                        .await?
                    {
                        return Ok(blocks);
                    }
                }
                Ok(blocks) if !blocks.is_empty() => {
                    tracing::debug!(
                        "Block cache partial hit for {}-{} ({} of {})",
                        start,
                        end,
                        blocks.len(),
                        expected_blocks
                    );
                    if verbose_sync_batch_logging_enabled() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        append_debug_log_line(&format!(
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache partial","data":{{"start":{},"end":{},"blocks":{},"expected":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                            id,
                            ts,
                            start,
                            end,
                            blocks.len(),
                            expected_blocks
                        ));
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("Block cache read failed for {}-{}: {}", start, end, e);
                    if verbose_sync_batch_logging_enabled() {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        append_debug_log_line(&format!(
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache read error","data":{{"start":{},"end":{},"error":"{}"}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                            id, ts, start, end, e
                        ));
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
                            if blocks.len() == expected_blocks
                                && Self::cached_blocks_are_canonical(
                                    &client, &cache, start, end, &blocks,
                                )
                                .await?
                            {
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
                            res = client.get_compact_block_range_with_wallet(
                                start as u32..(end + 1) as u32,
                                wallet_id.as_deref()
                            ) => res,
                            _ = cancel.cancelled() => Err(Error::Cancelled),
                        };

                        let fetch = match fetch {
                            Ok(blocks) => {
                                match Self::validate_compact_block_range(start, end, &blocks) {
                                    Ok(()) => Ok(blocks),
                                    Err(e) => Err(e),
                                }
                            }
                            Err(e) => Err(e),
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
                                    } else if verbose_sync_batch_logging_enabled() {
                                        let ts = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis();
                                        let id = format!("{:08x}", ts);
                                        append_debug_log_line(&format!(
                                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:block_cache","message":"block cache store","data":{{"start":{},"end":{},"blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                                            id,
                                            ts,
                                            start,
                                            end,
                                            blocks.len()
                                        ));
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

    async fn trial_decrypt_batch(
        &mut self,
        blocks: &[CompactBlockData],
    ) -> Result<(Vec<DecryptedNote>, TrialDecryptTelemetry)> {
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
        if verbose_sync_batch_logging_enabled() {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            append_debug_log_line(&format!(
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:trial_decrypt_batch","message":"trial_decrypt ivk availability","data":{{"sapling":{},"orchard":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id, ts, has_sapling_ivk, has_orchard_ivk
            ));
        }

        if !has_sapling_ivk && !has_orchard_ivk {
            tracing::warn!("No Sapling or Orchard IVK available for trial decryption");
            return Ok((Vec::new(), TrialDecryptTelemetry::default()));
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
        let decrypt_result = trial_decrypt_batch_impl(TrialDecryptBatchInputs {
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
        let all_notes = decrypt_result.notes;

        if verbose_sync_batch_logging_enabled() || !all_notes.is_empty() {
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
            append_debug_log_line(&format!(
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:trial_decrypt_batch","message":"trial_decrypt batch summary","data":{{"start":{},"end":{},"blocks":{},"sapling_outputs":{},"orchard_actions":{},"sapling_notes":{},"orchard_notes":{},"decrypt_cpu_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                min_height.unwrap_or(0),
                max_height,
                blocks.len(),
                sapling_outputs_total,
                orchard_actions_total,
                sapling_notes,
                orchard_notes,
                decrypt_result.telemetry.cpu_ms
            ));
        }

        Ok((all_notes, decrypt_result.telemetry))
    }

    fn spawn_prefetch(&self, start: u64, end: u64, estimated_bytes: u64) -> PrefetchTask {
        let client = self.client.clone();
        let cancel = self.cancel.clone();
        let wallet_id = self.wallet_id.clone();
        let handle = tokio::spawn(async move {
            SyncEngine::fetch_blocks_with_retry_inner(client, start, end, cancel, wallet_id).await
        });
        PrefetchTask {
            start,
            end,
            estimated_bytes,
            handle,
        }
    }

    fn spawn_server_batch_hint_prefetch(&self, start: u64) -> ServerBatchHintTask {
        let client = self.client.clone();
        let cancel = self.cancel.clone();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => None,
                result = client.get_lite_wallet_block_group(start) => {
                    match result {
                        Ok(value) if value >= start => Some(value),
                        _ => None,
                    }
                }
            }
        });
        ServerBatchHintTask { start, handle }
    }

    fn abort_prefetch_queue(
        prefetch_queue: &mut VecDeque<PrefetchTask>,
        queued_prefetch_bytes: &mut u64,
    ) {
        while let Some(prefetch) = prefetch_queue.pop_front() {
            prefetch.handle.abort();
        }
        *queued_prefetch_bytes = 0;
    }

    fn abort_pending_server_batch_hint(
        pending_server_group_hint: &mut Option<ServerBatchHintTask>,
    ) {
        if let Some(task) = pending_server_group_hint.take() {
            task.handle.abort();
        }
    }

    fn estimate_prefetch_bytes(start: u64, end: u64, avg_block_size_estimate: u64) -> u64 {
        let blocks = end.saturating_sub(start).saturating_add(1);
        blocks.saturating_mul(avg_block_size_estimate.max(1))
    }

    async fn fill_prefetch_queue(
        &self,
        prefetch_queue: &mut VecDeque<PrefetchTask>,
        queued_prefetch_bytes: &mut u64,
        sync_bounds: (u64, u64),
        batch_tuning: BatchTuning,
        hints: (&mut Option<u64>, &mut Option<ServerBatchHintTask>),
    ) -> Result<()> {
        let (start_height, end_height) = sync_bounds;
        let (server_group_end_hint, pending_server_group_hint) = hints;

        if start_height > end_height {
            return Ok(());
        }

        let max_depth = self.config.prefetch_queue_depth.max(1);
        let max_bytes = self.config.prefetch_queue_max_bytes.max(1);
        let mut next_start = prefetch_queue
            .back()
            .map(|task| task.end.saturating_add(1))
            .unwrap_or(start_height);

        while prefetch_queue.len() < max_depth && next_start <= end_height {
            let (batch_end, _desired_blocks) = self
                .compute_batch_end(
                    next_start,
                    end_height,
                    batch_tuning,
                    server_group_end_hint,
                    pending_server_group_hint,
                )
                .await?;

            if batch_end < next_start {
                break;
            }

            let estimated_bytes = Self::estimate_prefetch_bytes(
                next_start,
                batch_end,
                batch_tuning.avg_block_size_estimate,
            );
            let would_exceed_bytes =
                queued_prefetch_bytes.saturating_add(estimated_bytes) > max_bytes;
            if would_exceed_bytes && !prefetch_queue.is_empty() {
                break;
            }

            prefetch_queue.push_back(self.spawn_prefetch(next_start, batch_end, estimated_bytes));
            *queued_prefetch_bytes = queued_prefetch_bytes.saturating_add(estimated_bytes);
            next_start = batch_end.saturating_add(1);

            if self.config.use_server_batch_recommendations && next_start <= end_height {
                let replace_hint = match pending_server_group_hint.as_ref() {
                    Some(task) => task.start != next_start,
                    None => true,
                };
                if replace_hint {
                    Self::abort_pending_server_batch_hint(pending_server_group_hint);
                    *pending_server_group_hint =
                        Some(self.spawn_server_batch_hint_prefetch(next_start));
                }
            }
        }

        Ok(())
    }

    async fn compute_batch_end(
        &self,
        current_height: u64,
        end: u64,
        batch_tuning: BatchTuning,
        server_group_end_hint: &mut Option<u64>,
        pending_server_group_hint: &mut Option<ServerBatchHintTask>,
    ) -> Result<(u64, u64)> {
        let mut target_bytes = batch_tuning
            .target_bytes
            .clamp(self.config.min_batch_bytes, self.config.max_batch_bytes);
        if let Some(max_memory) = self.config.max_batch_memory_bytes {
            target_bytes = target_bytes.min(max_memory);
        }

        let estimated_block_bytes = batch_tuning.avg_block_size_estimate.max(1);
        let mut desired_blocks = target_bytes / estimated_block_bytes;
        if desired_blocks == 0 {
            desired_blocks = 1;
        }
        let max_batch_blocks = batch_tuning
            .max_batch_blocks
            .max(self.config.min_batch_size)
            .min(self.config.max_batch_size);
        desired_blocks = desired_blocks.clamp(self.config.min_batch_size, max_batch_blocks);

        let low_height_cap_end = if current_height <= LOW_HEIGHT_BATCH_CAP_HEIGHT {
            Some(std::cmp::min(
                end,
                current_height.saturating_add(LOW_HEIGHT_BATCH_MAX_BLOCKS.saturating_sub(1)),
            ))
        } else {
            None
        };

        let mut desired_end = std::cmp::min(current_height + desired_blocks - 1, end);
        if let Some(cap_end) = low_height_cap_end {
            desired_end = std::cmp::min(desired_end, cap_end);
        }

        if !self.config.use_server_batch_recommendations {
            return Ok((desired_end, desired_blocks));
        }

        let server_end = match *server_group_end_hint {
            Some(cached_end) if cached_end >= current_height => cached_end,
            _ => {
                if let Some(task) = pending_server_group_hint.take() {
                    if task.start == current_height {
                        if task.handle.is_finished() {
                            match task.handle.await {
                                Ok(Some(value)) => {
                                    *server_group_end_hint = Some(value);
                                    value
                                }
                                _ => {
                                    *server_group_end_hint = None;
                                    *pending_server_group_hint =
                                        Some(self.spawn_server_batch_hint_prefetch(current_height));
                                    return Ok((desired_end, desired_blocks));
                                }
                            }
                        } else {
                            *pending_server_group_hint = Some(task);
                            return Ok((desired_end, desired_blocks));
                        }
                    } else if task.start > current_height {
                        *pending_server_group_hint = Some(task);
                        return Ok((desired_end, desired_blocks));
                    } else {
                        task.handle.abort();
                        *pending_server_group_hint =
                            Some(self.spawn_server_batch_hint_prefetch(current_height));
                        return Ok((desired_end, desired_blocks));
                    }
                } else {
                    *pending_server_group_hint =
                        Some(self.spawn_server_batch_hint_prefetch(current_height));
                    return Ok((desired_end, desired_blocks));
                }
            }
        };

        match server_end {
            server_end if server_end >= current_height => {
                let optimal_end = std::cmp::min(server_end, end);
                let server_batch_size =
                    optimal_end.saturating_sub(current_height).saturating_add(1);
                let max_capped_end = std::cmp::min(
                    optimal_end,
                    current_height.saturating_add(max_batch_blocks.saturating_sub(1)),
                );
                let mut batch_end = std::cmp::min(desired_end, max_capped_end);
                if let Some(cap_end) = low_height_cap_end {
                    batch_end = std::cmp::min(batch_end, cap_end);
                }

                if max_capped_end > current_height && verbose_sync_batch_logging_enabled() {
                    // #region agent log
                    pirate_core::debug_log::with_locked_file(|file| {
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
                    });
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
            _ => {
                *server_group_end_hint = None;
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
                pirate_core::debug_log::with_locked_file(|file| {
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
                });
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
                let db =
                    Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
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
        let mut txids: std::collections::HashSet<[u8; 32]> = std::collections::HashSet::new();
        for note in notes.iter() {
            if note.txid.len() == 32 {
                let mut txid = [0u8; 32];
                txid.copy_from_slice(&note.txid[..32]);
                txids.insert(txid);
            }
        }

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

        pirate_core::debug_log::with_locked_file(|file| {
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
        });

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
                match sink.get_note_by_txid_and_index(
                    &note.tx_hash,
                    note.output_index as i64,
                    note.note_type,
                ) {
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

        pirate_core::debug_log::with_locked_file(|file| {
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
        });

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

        pirate_core::debug_log::with_locked_file(|file| {
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
        });

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
                    pirate_core::debug_log::with_locked_file(|file| {
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
                    });
                    continue;
                }
            };
            pirate_core::debug_log::with_locked_file(|file| {
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
            });

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
                                        NoteType::Sapling,
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
                                        NoteType::Orchard,
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
                                                pirate_core::debug_log::with_locked_file(|file| {
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
                                                });
                                            }
                                        }
                                    }
                                }

                                let nullifier_zero = note.nullifier.iter().all(|b| *b == 0);
                                pirate_core::debug_log::with_locked_file(|file| {
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
                                });
                            }
                            Ok(None) => {
                                pirate_core::debug_log::with_locked_file(|file| {
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
                                });
                            }
                            Err(e) => {
                                tracing::warn!("Error decrypting Orchard memo: {}", e);
                                pirate_core::debug_log::with_locked_file(|file| {
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
                                });
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

        pirate_core::debug_log::with_locked_file(|file| {
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
        });

        Ok(())
    }

    async fn cleanup_orchard_false_positives(&self) -> Result<()> {
        let sink = match self.storage.as_ref() {
            Some(s) => s.clone(),
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
                let db =
                    Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
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

        pirate_core::debug_log::with_locked_file(|file| {
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
        });

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
                    tracing::debug!(
                        "Orchard cleanup: full decrypt returned none for tx {} output {}; keeping note",
                        hex::encode(txid),
                        action_index
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

    /// Process block commitments into ShardTree batches and track positions.
    ///
    /// ShardTree is the single source of truth for commitment tree state.
    /// Positions are tracked via simple counters (initialized from frontier at sync start).
    async fn update_commitment_trees(
        &self,
        blocks: &[CompactBlockData],
        notes: &[DecryptedNote],
        checkpoint_mode: FrontierCheckpointMode,
        warm_trees: Option<&mut SyncWarmTrees<'_>>,
        historical_prefill_state: Option<&mut HistoricalPrefillState>,
    ) -> Result<(u64, PositionMaps, bool)> {
        let mut sapling_pos = self.sapling_tree_position.write().await;
        let mut orchard_pos = self.orchard_tree_position.write().await;
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
        let has_owned_sapling = !sapling_owned.is_empty();
        let has_owned_orchard = !orchard_owned.is_empty();
        let mut shardtree_batches: Vec<ShardtreeBatch> = Vec::with_capacity(blocks.len());
        let batch_end_height = blocks.last().map(|block| block.height);
        let mut historical_prefill_state = historical_prefill_state;

        if checkpoint_mode != FrontierCheckpointMode::OwnedOnly {
            if let Some(state) = historical_prefill_state.as_deref_mut() {
                merge_emitted_batches(
                    &mut shardtree_batches,
                    drain_historical_skip_state(&mut state.sapling, append_sapling_leaf),
                );
                merge_emitted_batches(
                    &mut shardtree_batches,
                    drain_historical_skip_state(&mut state.orchard, append_orchard_leaf),
                );
            }
        }

        for block in blocks {
            let mut shardtree_batch = ShardtreeBatch::new(block.height);
            let sapling_pos_ref = &mut *sapling_pos;
            let orchard_pos_ref = &mut *orchard_pos;
            let count_ref = &mut count;
            let position_mappings_ref = &mut position_mappings;
            let (mut sapling_skip_state, mut orchard_skip_state) =
                if checkpoint_mode == FrontierCheckpointMode::OwnedOnly {
                    match historical_prefill_state.as_deref_mut() {
                        Some(state) => (Some(&mut state.sapling), Some(&mut state.orchard)),
                        None => (None, None),
                    }
                } else {
                    (None, None)
                };

            let mut process_tx = |tx: &crate::client::CompactTx| -> Result<()> {
                let txid = tx.hash.as_slice();
                for (output_idx, output) in tx.outputs.iter().enumerate() {
                    if output.cmu.len() == 32 {
                        let mut cm = [0u8; 32];
                        cm.copy_from_slice(&output.cmu);
                        let pos = *sapling_pos_ref;
                        *sapling_pos_ref = sapling_pos_ref.saturating_add(1);
                        let is_owned = has_owned_sapling && sapling_owned.contains(&cm);
                        if is_owned {
                            if let Some(key) = TxOutputKey::new(txid, output_idx) {
                                position_mappings_ref.sapling_by_tx.insert(key, pos);
                            }
                        }
                        let cmu_opt: Option<SaplingExtractedNoteCommitment> =
                            SaplingExtractedNoteCommitment::from_bytes(&cm).into();
                        if let Some(cmu_value) = cmu_opt {
                            let node = SaplingNode::from_cmu(&cmu_value);
                            let retention = if is_owned {
                                Retention::Marked
                            } else {
                                Retention::Ephemeral
                            };
                            process_historical_leaf(
                                sapling_skip_state.as_deref_mut(),
                                pos,
                                block.height,
                                node,
                                retention,
                                HistoricalLeafSink {
                                    current_batch: &mut shardtree_batch,
                                    shardtree_batches: &mut shardtree_batches,
                                },
                                append_sapling_leaf,
                            );
                            *count_ref += 1;
                        }
                    }
                }

                for action in &tx.actions {
                    if action.cmx.len() == 32 {
                        let mut cm = [0u8; 32];
                        cm.copy_from_slice(&action.cmx);
                        let pos = *orchard_pos_ref;
                        *orchard_pos_ref = orchard_pos_ref.saturating_add(1);
                        let is_owned = has_owned_orchard && orchard_owned.contains(&cm);
                        if is_owned {
                            position_mappings_ref.orchard_by_commitment.insert(cm, pos);
                        }
                        let cmx_opt: Option<OrchardExtractedNoteCommitment> =
                            OrchardExtractedNoteCommitment::from_bytes(&cm).into();
                        if let Some(cmx) = cmx_opt {
                            let cmx_hash = MerkleHashOrchard::from_cmx(&cmx);
                            let retention = if is_owned {
                                Retention::Marked
                            } else {
                                Retention::Ephemeral
                            };
                            process_historical_leaf(
                                orchard_skip_state.as_deref_mut(),
                                pos,
                                block.height,
                                cmx_hash,
                                retention,
                                HistoricalLeafSink {
                                    current_batch: &mut shardtree_batch,
                                    shardtree_batches: &mut shardtree_batches,
                                },
                                append_orchard_leaf,
                            );
                            *count_ref += 1;
                        }
                    }
                }

                Ok(())
            };

            let mut monotonic = true;
            let mut last_idx = 0u64;
            for (fallback_idx, tx) in block.transactions.iter().enumerate() {
                let idx = tx.index.unwrap_or(fallback_idx as u64);
                if fallback_idx > 0 && idx < last_idx {
                    monotonic = false;
                    break;
                }
                last_idx = idx;
            }

            if monotonic {
                for tx in &block.transactions {
                    process_tx(tx)?;
                }
            } else {
                let mut txs: Vec<_> = block.transactions.iter().enumerate().collect();
                txs.sort_by_key(|(fallback_idx, tx)| tx.index.unwrap_or(*fallback_idx as u64));
                for (_, tx) in txs {
                    process_tx(tx)?;
                }
            }

            let checkpoint_height = u32::try_from(block.height).map_err(|_| {
                Error::Sync(format!(
                    "Checkpoint height {} exceeds u32::MAX",
                    block.height
                ))
            })?;
            let checkpoint_id = BlockHeight::from(checkpoint_height);
            let has_wallet_mark = shardtree_batch
                .sapling
                .iter()
                .any(|(_, retention)| retention.is_marked())
                || shardtree_batch
                    .orchard
                    .iter()
                    .any(|(_, retention)| retention.is_marked());
            let should_checkpoint = match checkpoint_mode {
                FrontierCheckpointMode::PerBlock => true,
                FrontierCheckpointMode::OwnedOnly => has_wallet_mark,
            };
            shardtree_batch.checkpoint_id = should_checkpoint.then_some(checkpoint_id);
            if should_checkpoint {
                if let Some((_, retention)) = shardtree_batch.sapling.last_mut() {
                    *retention = Retention::Checkpoint {
                        id: checkpoint_id,
                        is_marked: retention.is_marked(),
                    };
                } else {
                    shardtree_batch.sapling_empty_checkpoint = true;
                }
                if let Some((_, retention)) = shardtree_batch.orchard.last_mut() {
                    *retention = Retention::Checkpoint {
                        id: checkpoint_id,
                        is_marked: retention.is_marked(),
                    };
                } else {
                    shardtree_batch.orchard_empty_checkpoint = true;
                }
            }

            shardtree_batches.push(shardtree_batch);
        }

        let persist_result = if let Some(trees) = warm_trees {
            trees.persist_batches(&shardtree_batches, batch_end_height)?
        } else {
            self.persist_shardtree_batches(&shardtree_batches, batch_end_height)?
        };
        Ok((
            count,
            position_mappings,
            persist_result.batch_end_checkpointed,
        ))
    }

    /// Persist commitment batches to the ShardTree (SQLite-backed).
    ///
    /// Uses upstream-style retained leaves and encodes per-block checkpoints at insert time.
    fn persist_shardtree_batches(
        &self,
        batches: &[ShardtreeBatch],
        batch_end_height: Option<u64>,
    ) -> Result<ShardtreePersistResult> {
        if batches.is_empty() {
            return Ok(ShardtreePersistResult::default());
        }
        let Some(sink) = self.storage.as_ref() else {
            return Ok(ShardtreePersistResult::default());
        };

        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let tx = db
            .conn()
            .unchecked_transaction()
            .map_err(|e| Error::Sync(format!("Failed to start shardtree transaction: {}", e)))?;

        let sapling_store =
            SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                &tx,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open Sapling shard store: {}", e)))?;
        let orchard_store =
            SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                &tx,
                ORCHARD_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open Orchard shard store: {}", e)))?;

        let mut sapling_tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT> =
            ShardTree::new(sapling_store, SHARDTREE_PRUNING_DEPTH);
        let mut orchard_tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT> =
            ShardTree::new(orchard_store, SHARDTREE_PRUNING_DEPTH);

        // Guard: find the highest block height already checkpointed so we can
        // skip blocks that were already committed to the tree. Re-appending
        // commitments for an already-processed block corrupts the tree because
        // ShardTree::append() is NOT idempotent — each call adds a leaf at the
        // next position regardless of whether the commitment was already present.
        let max_existing_sapling_checkpoint: Option<u32> = tx
            .query_row(
                "SELECT MAX(checkpoint_id) FROM sapling_tree_checkpoints",
                [],
                |row| row.get(0),
            )
            .unwrap_or(None);
        let max_existing_orchard_checkpoint: Option<u32> = tx
            .query_row(
                "SELECT MAX(checkpoint_id) FROM orchard_tree_checkpoints",
                [],
                |row| row.get(0),
            )
            .unwrap_or(None);
        let max_committed_height = match (
            max_existing_sapling_checkpoint,
            max_existing_orchard_checkpoint,
        ) {
            (Some(s), Some(o)) => Some(s.max(o)),
            (Some(s), None) => Some(s),
            (None, Some(o)) => Some(o),
            (None, None) => None,
        };

        let result = apply_shardtree_batches_to_trees(
            &mut sapling_tree,
            &mut orchard_tree,
            batches,
            batch_end_height,
            max_committed_height,
        )?;

        tx.commit()
            .map_err(|e| Error::Sync(format!("Failed to commit shardtree transaction: {}", e)))?;

        Ok(result)
    }

    async fn apply_positions(&self, notes: &mut [DecryptedNote], position_mappings: &PositionMaps) {
        // Canonical path: persist stable note identity material (position, note bytes,
        // nullifier, key mapping). Witness paths are ephemeral and are resolved from
        // the active frontier at spend time, not persisted per-note.
        for note in notes.iter_mut() {
            match note.note_type {
                crate::pipeline::NoteType::Sapling => {
                    let position = TxOutputKey::new(&note.tx_hash, note.output_index)
                        .and_then(|key| position_mappings.sapling_by_tx.get(&key).copied());
                    if let Some(pos) = position {
                        note.position = Some(pos);
                    }
                }
                crate::pipeline::NoteType::Orchard => {
                    if let Some(position) = position_mappings
                        .orchard_by_commitment
                        .get(&note.commitment)
                        .copied()
                    {
                        note.position = Some(position);
                    }
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

            let needs_nullifier = note.nullifier.iter().all(|b| *b == 0);
            let needs_note_bytes = note.note_bytes.is_empty();
            if !needs_nullifier && !needs_note_bytes {
                continue;
            }

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
            let rseed = if leadbyte == 0x02 {
                zcash_primitives::sapling::Rseed::AfterZip212(rseed_bytes)
            } else {
                let rcm = Option::from(jubjub::Fr::from_bytes(&rseed_bytes))
                    .ok_or_else(|| Error::Sync("Invalid Sapling rseed bytes".to_string()))?;
                zcash_primitives::sapling::Rseed::BeforeZip212(rcm)
            };

            let position = TxOutputKey::new(&note.tx_hash, note.output_index)
                .and_then(|key| position_mappings.sapling_by_tx.get(&key).copied());
            if needs_nullifier && position.is_none() {
                tracing::warn!(
                    "Missing Sapling position for tx {} output {}",
                    hex::encode(&note.tx_hash),
                    note.output_index
                );
                continue;
            }

            let decoded_address = decode_sapling_address_bytes_from_note_bytes(&note.note_bytes)
                .and_then(|address_bytes| SaplingPaymentAddress::from_bytes(&address_bytes));

            let mut candidate_keys: Vec<(i64, &ExtendedFullViewingKey)> = Vec::new();
            let mut seen_key_ids: HashSet<i64> = HashSet::new();
            if let Some(key_id) = note.key_id {
                if let Some(dfvk) = dfvk_by_id.get(&key_id) {
                    candidate_keys.push((key_id, dfvk));
                    seen_key_ids.insert(key_id);
                }
            }
            if let Some(dfvk) = dfvk_by_id.get(&default_key_id) {
                if !seen_key_ids.contains(&default_key_id) {
                    candidate_keys.push((default_key_id, dfvk));
                    seen_key_ids.insert(default_key_id);
                }
            }
            for (key_id, dfvk) in &dfvk_by_id {
                if !seen_key_ids.contains(key_id) {
                    candidate_keys.push((*key_id, dfvk));
                }
            }

            let diversifier = if note.diversifier.len() == 11 {
                let mut d = [0u8; 11];
                d.copy_from_slice(&note.diversifier[..11]);
                Some(zcash_primitives::sapling::Diversifier(d))
            } else {
                None
            };

            let mut selected: Option<(i64, SaplingPaymentAddress, Option<[u8; 32]>)> = None;
            for (candidate_key_id, dfvk) in &candidate_keys {
                let payment_address = if let Some(addr) = decoded_address {
                    addr
                } else {
                    let diversifier = match diversifier {
                        Some(d) => d,
                        None => continue,
                    };
                    let sapling_ivk = if note.address_scope == AddressScope::Internal {
                        let internal_ivk_bytes = dfvk.to_internal_ivk_bytes();
                        match Option::from(jubjub::Fr::from_bytes(&internal_ivk_bytes)) {
                            Some(ivk_fr) => SaplingIvk(ivk_fr),
                            None => continue,
                        }
                    } else {
                        dfvk.sapling_ivk()
                    };
                    match sapling_ivk.to_payment_address(diversifier) {
                        Some(addr) => addr,
                        None => continue,
                    }
                };

                let mut external_nf: Option<[u8; 32]> = None;
                let mut internal_nf: Option<[u8; 32]> = None;
                if let Some(pos) = position {
                    let note_value =
                        zcash_primitives::sapling::value::NoteValue::from_raw(note.value);
                    let sapling_note = zcash_primitives::sapling::Note::from_parts(
                        payment_address,
                        note_value,
                        rseed,
                    );
                    external_nf = Some(
                        sapling_note
                            .nf(
                                &dfvk.nullifier_deriving_key_for_scope(SaplingScope::External),
                                pos,
                            )
                            .0,
                    );
                    internal_nf = Some(
                        sapling_note
                            .nf(
                                &dfvk.nullifier_deriving_key_for_scope(SaplingScope::Internal),
                                pos,
                            )
                            .0,
                    );
                }

                let preferred_nf = if note.address_scope == AddressScope::Internal {
                    internal_nf.or(external_nf)
                } else {
                    external_nf.or(internal_nf)
                };

                if !needs_nullifier {
                    if let Some(nf) = external_nf {
                        if note.nullifier.len() == 32 && note.nullifier.as_slice() == nf {
                            selected = Some((*candidate_key_id, payment_address, Some(nf)));
                            break;
                        }
                    }
                    if let Some(nf) = internal_nf {
                        if note.nullifier.len() == 32 && note.nullifier.as_slice() == nf {
                            selected = Some((*candidate_key_id, payment_address, Some(nf)));
                            break;
                        }
                    }
                } else if selected.is_none() {
                    selected = Some((*candidate_key_id, payment_address, preferred_nf));
                }
            }

            if selected.is_none() {
                if let Some(addr) = decoded_address {
                    selected = Some((note.key_id.unwrap_or(default_key_id), addr, None));
                } else {
                    tracing::warn!(
                        "Failed to derive Sapling address for tx {} output {}",
                        hex::encode(&note.tx_hash),
                        note.output_index
                    );
                    continue;
                }
            }

            let (selected_key_id, selected_address, selected_nf) = selected.unwrap();
            if note.key_id != Some(selected_key_id) {
                note.key_id = Some(selected_key_id);
            }

            if needs_note_bytes {
                note.note_bytes =
                    encode_sapling_note_bytes(selected_address, leadbyte, rseed_bytes);
            }

            if needs_nullifier {
                if let Some(nf) = selected_nf {
                    note.nullifier = nf;
                }
            }
        }

        Ok(())
    }

    async fn apply_spends(
        &mut self,
        blocks: &[CompactBlockData],
        db: Option<&Database>,
    ) -> Result<()> {
        let sink = match self.storage.as_ref() {
            Some(s) => s.clone(),
            None => return Ok(()),
        };
        if self.nullifier_cache.is_empty() && self.tracked_wallet_txids.is_empty() {
            return Ok(());
        }
        let mut spend_updates: Vec<(i64, [u8; 32])> = Vec::new();
        let mut spend_nullifiers: Vec<(
            pirate_storage_sqlite::models::NoteType,
            [u8; 32],
            [u8; 32],
        )> = Vec::new();
        let mut matched_nullifiers: std::collections::HashSet<(
            pirate_storage_sqlite::models::NoteType,
            [u8; 32],
        )> = std::collections::HashSet::new();
        let mut sapling_spends = 0u64;
        let mut orchard_spends = 0u64;
        let mut matched_spends = 0u64;
        let mut min_height: Option<u64> = None;
        let mut max_height: Option<u64> = None;
        let mut matched_spend_txids: HashSet<[u8; 32]> = HashSet::new();
        let mut spend_tx_meta: HashMap<[u8; 32], (i64, i64, i64)> = HashMap::new();
        let mut recovered_nullifiers: Vec<[u8; 32]> = Vec::new();
        let mut matched_cache_nullifiers: Vec<[u8; 32]> = Vec::new();

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
                let mut saw_any_spend = false;
                let track_unmatched_for_tx = self.tracked_wallet_txids.contains(&txid);

                for spend in &tx.spends {
                    if spend.nf.len() == 32 {
                        sapling_spends += 1;
                        saw_any_spend = true;
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&spend.nf[..32]);
                        if !nf.iter().all(|b| *b == 0) {
                            let note_type = pirate_storage_sqlite::models::NoteType::Sapling;
                            if let Some(id) = self.nullifier_cache.get(&nf).copied() {
                                spend_updates.push((id, txid));
                                matched_cache_nullifiers.push(nf);
                                matched_nullifiers.insert((note_type, nf));
                                has_spend = true;
                                matched_spend_txids.insert(txid);
                                matched_spends += 1;
                            } else if track_unmatched_for_tx {
                                spend_nullifiers.push((note_type, nf, txid));
                            }
                        }
                    }
                }

                for action in &tx.actions {
                    if action.nullifier.len() == 32 {
                        orchard_spends += 1;
                        saw_any_spend = true;
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&action.nullifier[..32]);
                        if !nf.iter().all(|b| *b == 0) {
                            let note_type = pirate_storage_sqlite::models::NoteType::Orchard;
                            if let Some(id) = self.nullifier_cache.get(&nf).copied() {
                                spend_updates.push((id, txid));
                                matched_cache_nullifiers.push(nf);
                                matched_nullifiers.insert((note_type, nf));
                                has_spend = true;
                                matched_spend_txids.insert(txid);
                                matched_spends += 1;
                            } else if track_unmatched_for_tx {
                                spend_nullifiers.push((note_type, nf, txid));
                            }
                        }
                    }
                }

                if saw_any_spend {
                    spend_tx_meta
                        .insert(txid, (block_height, block_time, tx.fee.unwrap_or(0) as i64));
                }

                if has_spend {
                    self.tracked_wallet_txids.insert(txid);
                }
            }
        }

        let has_wallet_relevant_candidates =
            !spend_updates.is_empty() || !spend_nullifiers.is_empty();
        if !has_wallet_relevant_candidates {
            if verbose_sync_batch_logging_enabled() {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = format!("{:08x}", ts);
                append_debug_log_line(&format!(
                    r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:apply_spends","message":"apply_spends skipped","data":{{"start":{},"end":{},"sapling_spends":{},"orchard_spends":{},"reason":"no_wallet_relevant_candidates","cache_size":{},"tracked_wallet_txids":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                    id,
                    ts,
                    min_height.unwrap_or(0),
                    max_height.unwrap_or(0),
                    sapling_spends,
                    orchard_spends,
                    self.nullifier_cache.len(),
                    self.tracked_wallet_txids.len()
                ));
            }
            return Ok(());
        }

        let mut updated_count = 0u64;
        let mut fallback_updates = 0u64;
        let mut fallback_entries: Vec<([u8; 32], [u8; 32])> = Vec::new();
        if !spend_nullifiers.is_empty() {
            let has_sapling_rederive_keys = self.keys.iter().any(|k| k.sapling_dfvk.is_some());
            let has_orchard_rederive_keys = self.keys.iter().any(|k| k.orchard_fvk.is_some());
            let mut unlinked_entries: Vec<(
                pirate_storage_sqlite::models::NoteType,
                [u8; 32],
                [u8; 32],
            )> = Vec::new();
            for (note_type, nf, txid) in &spend_nullifiers {
                if !matched_nullifiers.contains(&(*note_type, *nf)) {
                    unlinked_entries.push((*note_type, *nf, *txid));
                }
            }
            if !unlinked_entries.is_empty() {
                let should_attempt_rederive =
                    unlinked_entries
                        .iter()
                        .any(|(note_type, _, _)| match note_type {
                            pirate_storage_sqlite::models::NoteType::Sapling => {
                                has_sapling_rederive_keys
                            }
                            pirate_storage_sqlite::models::NoteType::Orchard => {
                                has_orchard_rederive_keys
                            }
                        });
                if should_attempt_rederive {
                    match self.rederive_unmatched_spends(&unlinked_entries, db)? {
                        recovered if !recovered.is_empty() => {
                            let recover_updates: Vec<(i64, [u8; 32])> = recovered
                                .iter()
                                .map(|(id, _, _, txid)| (*id, *txid))
                                .collect();
                            if !recover_updates.is_empty() {
                                spend_updates.extend(recover_updates);
                                for (_, _, nf, _) in &recovered {
                                    recovered_nullifiers.push(*nf);
                                }
                            }

                            let recovered_keys: HashSet<(
                                pirate_storage_sqlite::models::NoteType,
                                [u8; 32],
                            )> = recovered
                                .iter()
                                .map(|(_, note_type, nf, _)| (*note_type, *nf))
                                .collect();
                            for (_, _, _, txid) in &recovered {
                                matched_spend_txids.insert(*txid);
                            }
                            unlinked_entries.retain(|(note_type, nf, _)| {
                                !recovered_keys.contains(&(*note_type, *nf))
                            });
                        }
                        _ => {}
                    }
                }
            }

            for (_, nf, txid) in &unlinked_entries {
                fallback_entries.push((*nf, *txid));
            }
            if !unlinked_entries.is_empty() {
                if let Err(e) = sink.upsert_unlinked_spend_nullifiers_with_txid(&unlinked_entries) {
                    tracing::warn!("Failed to store unlinked spend nullifiers: {}", e);
                }
            }
        }

        let mut wallet_relevant_spend_txids: HashSet<[u8; 32]> = matched_spend_txids;
        for (_, txid) in &fallback_entries {
            wallet_relevant_spend_txids.insert(*txid);
        }

        let mut tx_updates: Vec<(String, i64, i64, i64)> =
            Vec::with_capacity(wallet_relevant_spend_txids.len() * 2);
        for txid in wallet_relevant_spend_txids.iter().copied() {
            if let Some((height, timestamp, fee)) = spend_tx_meta.get(&txid) {
                let txid_internal_hex = hex::encode(txid);
                tx_updates.push((txid_internal_hex.clone(), *height, *timestamp, *fee));

                let mut txid_display = txid;
                txid_display.reverse();
                let txid_display_hex = hex::encode(txid_display);
                if txid_display_hex != txid_internal_hex {
                    tx_updates.push((txid_display_hex, *height, *timestamp, *fee));
                }
            }
        }
        if !tx_updates.is_empty() {
            tx_updates.sort_by(|a, b| a.0.cmp(&b.0));
            tx_updates.dedup_by(|a, b| a.0 == b.0);
        }

        let spend_apply_start = Instant::now();
        match match db {
            Some(db) => sink.apply_spend_updates_with_txmeta_with_db(
                db,
                &spend_updates,
                &fallback_entries,
                &tx_updates,
            ),
            None => {
                sink.apply_spend_updates_with_txmeta(&spend_updates, &fallback_entries, &tx_updates)
            }
        } {
            Ok((updated, fallback)) => {
                updated_count = updated;
                fallback_updates = fallback;
                if updated_count > 0 || fallback_updates > 0 {
                    for nf in matched_cache_nullifiers {
                        self.nullifier_cache.remove(&nf);
                    }
                    for nf in recovered_nullifiers {
                        self.nullifier_cache.remove(&nf);
                    }
                    for (nf, _) in &fallback_entries {
                        self.nullifier_cache.remove(nf);
                    }
                }
                tracing::debug!(
                    "Applied spend updates in {}ms (id_updates={}, fallback_updates={}, tx_meta={})",
                    spend_apply_start.elapsed().as_millis(),
                    updated_count,
                    fallback_updates,
                    tx_updates.len()
                );
            }
            Err(e) => {
                tracing::warn!("Failed batched spend apply for batch: {}", e);
            }
        }

        if verbose_sync_batch_logging_enabled()
            || matched_spends > 0
            || updated_count > 0
            || fallback_updates > 0
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let id = format!("{:08x}", ts);
            append_debug_log_line(&format!(
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:apply_spends","message":"apply_spends summary","data":{{"start":{},"end":{},"sapling_spends":{},"orchard_spends":{},"matched_spends":{},"updates":{},"fallback_updates":{},"cache_size":{},"tracked_wallet_txids":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                min_height.unwrap_or(0),
                max_height.unwrap_or(0),
                sapling_spends,
                orchard_spends,
                matched_spends,
                updated_count,
                fallback_updates,
                self.nullifier_cache.len(),
                self.tracked_wallet_txids.len()
            ));
        }

        Ok(())
    }

    fn rederive_unmatched_sapling_spends(
        &self,
        entries: &[TypedSpendEntry],
        db: Option<&Database>,
    ) -> Result<Vec<RecoveredSpend>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let mut spend_map: HashMap<NullifierBytes, TxidBytes> = HashMap::new();
        for (note_type, nf, txid) in entries {
            if *note_type == pirate_storage_sqlite::models::NoteType::Sapling {
                spend_map.entry(*nf).or_insert(*txid);
            }
        }
        if spend_map.is_empty() {
            return Ok(Vec::new());
        }

        let mut dfvk_by_key: HashMap<i64, ExtendedFullViewingKey> = HashMap::new();
        for key in &self.keys {
            if let Some(dfvk) = key.sapling_dfvk.as_ref() {
                dfvk_by_key.insert(key.key_id, dfvk.clone());
            }
        }
        if dfvk_by_key.is_empty() {
            return Ok(Vec::new());
        }

        let owned_db;
        let db = if let Some(db) = db {
            db
        } else {
            owned_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &owned_db
        };
        let repo = Repository::new(db);
        let notes = repo.get_spend_reconciliation_notes(sink.account_id)?;

        let mut recovered: Vec<RecoveredSpend> = Vec::new();
        for mut note in notes {
            if note.note_type != pirate_storage_sqlite::models::NoteType::Sapling {
                continue;
            }
            let id = match note.id {
                Some(id) => id,
                None => continue,
            };
            let position = match note.position {
                Some(pos) if pos >= 0 => pos as u64,
                _ => continue,
            };
            let note_bytes = match note.note.as_ref() {
                Some(bytes) if !bytes.is_empty() => bytes,
                _ => continue,
            };
            let (leadbyte, rseed_bytes) = match decode_sapling_rseed_from_note_bytes(note_bytes) {
                Some(parts) => parts,
                None => continue,
            };
            let address_bytes = match decode_sapling_address_bytes_from_note_bytes(note_bytes) {
                Some(bytes) => bytes,
                None => continue,
            };
            let payment_address = match SaplingPaymentAddress::from_bytes(&address_bytes) {
                Some(address) => address,
                None => continue,
            };
            let rseed = if leadbyte == 0x02 {
                Rseed::AfterZip212(rseed_bytes)
            } else {
                let rcm = match Option::from(jubjub::Fr::from_bytes(&rseed_bytes)) {
                    Some(rcm) => rcm,
                    None => continue,
                };
                Rseed::BeforeZip212(rcm)
            };
            let value = match u64::try_from(note.value) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let note_value = zcash_primitives::sapling::value::NoteValue::from_raw(value);
            let sapling_note =
                zcash_primitives::sapling::Note::from_parts(payment_address, note_value, rseed);

            // Try the stored key first, then all other keys as fallback.
            // This recovers from stale/misassigned key_id values without requiring local hacks.
            let mut candidate_keys: Vec<(i64, &ExtendedFullViewingKey)> = Vec::new();
            let mut seen_key_ids: HashSet<i64> = HashSet::new();
            if let Some(key_id) = note.key_id {
                if let Some(dfvk) = dfvk_by_key.get(&key_id) {
                    candidate_keys.push((key_id, dfvk));
                    seen_key_ids.insert(key_id);
                }
            }
            for (key_id, dfvk) in &dfvk_by_key {
                if !seen_key_ids.contains(key_id) {
                    candidate_keys.push((*key_id, dfvk));
                }
            }

            let mut matched: Option<([u8; 32], [u8; 32], i64)> = None;
            for (candidate_key_id, dfvk) in candidate_keys {
                let external_nf = sapling_note
                    .nf(
                        &dfvk.nullifier_deriving_key_for_scope(SaplingScope::External),
                        position,
                    )
                    .0;
                if let Some(spent_txid) = spend_map.get(&external_nf) {
                    matched = Some((external_nf, *spent_txid, candidate_key_id));
                    break;
                }

                let internal_nf = sapling_note
                    .nf(
                        &dfvk.nullifier_deriving_key_for_scope(SaplingScope::Internal),
                        position,
                    )
                    .0;
                if let Some(spent_txid) = spend_map.get(&internal_nf) {
                    matched = Some((internal_nf, *spent_txid, candidate_key_id));
                    break;
                }
            }

            if let Some((nf, spent_txid, matched_key_id)) = matched {
                let mut note_changed = false;
                if note.nullifier.len() != 32 || note.nullifier.as_slice() != nf {
                    note.nullifier = nf.to_vec();
                    note_changed = true;
                }
                if note.key_id != Some(matched_key_id) {
                    note.key_id = Some(matched_key_id);
                    note_changed = true;
                }
                if note_changed {
                    let _ = repo.update_note_by_id(&note);
                }
                recovered.push((id, nf, spent_txid));
                spend_map.remove(&nf);
                if spend_map.is_empty() {
                    break;
                }
            }
        }

        Ok(recovered)
    }

    fn rederive_unmatched_orchard_spends(
        &self,
        entries: &[TypedSpendEntry],
        db: Option<&Database>,
    ) -> Result<Vec<RecoveredSpend>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let mut spend_map: HashMap<NullifierBytes, TxidBytes> = HashMap::new();
        for (note_type, nf, txid) in entries {
            if *note_type == pirate_storage_sqlite::models::NoteType::Orchard {
                spend_map.entry(*nf).or_insert(*txid);
            }
        }
        if spend_map.is_empty() {
            return Ok(Vec::new());
        }

        let mut fvk_by_key: HashMap<i64, OrchardExtendedFullViewingKey> = HashMap::new();
        for key in &self.keys {
            if let Some(fvk) = key.orchard_fvk.as_ref() {
                fvk_by_key.insert(key.key_id, fvk.clone());
            }
        }
        if fvk_by_key.is_empty() {
            return Ok(Vec::new());
        }

        let owned_db;
        let db = if let Some(db) = db {
            db
        } else {
            owned_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &owned_db
        };
        let repo = Repository::new(db);
        let notes = repo.get_spend_reconciliation_notes(sink.account_id)?;

        let mut recovered: Vec<RecoveredSpend> = Vec::new();
        for mut note in notes {
            if note.note_type != pirate_storage_sqlite::models::NoteType::Orchard {
                continue;
            }
            let id = match note.id {
                Some(id) => id,
                None => continue,
            };
            let note_bytes = match note.note.as_ref() {
                Some(bytes) if !bytes.is_empty() => bytes,
                _ => continue,
            };
            let address_bytes = match decode_orchard_address_bytes_from_note_bytes(note_bytes) {
                Some(bytes) => bytes,
                None => continue,
            };
            let (rho, rseed) = match decode_orchard_rho_rseed_from_note_bytes(note_bytes) {
                Some(parts) => parts,
                None => continue,
            };
            let value = match u64::try_from(note.value) {
                Ok(value) => value,
                Err(_) => continue,
            };

            let mut candidate_keys: Vec<(i64, &OrchardExtendedFullViewingKey)> = Vec::new();
            let mut seen_key_ids: HashSet<i64> = HashSet::new();
            if let Some(key_id) = note.key_id {
                if let Some(fvk) = fvk_by_key.get(&key_id) {
                    candidate_keys.push((key_id, fvk));
                    seen_key_ids.insert(key_id);
                }
            }
            for (key_id, fvk) in &fvk_by_key {
                if !seen_key_ids.contains(key_id) {
                    candidate_keys.push((*key_id, fvk));
                }
            }

            let mut matched: Option<([u8; 32], [u8; 32], i64)> = None;
            for (candidate_key_id, fvk) in candidate_keys {
                let nf = match orchard_nullifier_from_parts(
                    &fvk.inner,
                    address_bytes,
                    value,
                    rho,
                    rseed,
                ) {
                    Ok(nf) => nf,
                    Err(_) => continue,
                };
                if let Some(spent_txid) = spend_map.get(&nf) {
                    matched = Some((nf, *spent_txid, candidate_key_id));
                    break;
                }
            }

            if let Some((nf, spent_txid, matched_key_id)) = matched {
                let mut note_changed = false;
                if note.nullifier.len() != 32 || note.nullifier.as_slice() != nf {
                    note.nullifier = nf.to_vec();
                    note_changed = true;
                }
                if note.key_id != Some(matched_key_id) {
                    note.key_id = Some(matched_key_id);
                    note_changed = true;
                }
                if note_changed {
                    let _ = repo.update_note_by_id(&note);
                }
                recovered.push((id, nf, spent_txid));
                spend_map.remove(&nf);
                if spend_map.is_empty() {
                    break;
                }
            }
        }

        Ok(recovered)
    }

    fn rederive_unmatched_spends(
        &self,
        entries: &[TypedSpendEntry],
        db: Option<&Database>,
    ) -> Result<Vec<TypedRecoveredSpend>> {
        let mut recovered: Vec<TypedRecoveredSpend> = Vec::new();

        for (id, nf, txid) in self.rederive_unmatched_sapling_spends(entries, db)? {
            recovered.push((
                id,
                pirate_storage_sqlite::models::NoteType::Sapling,
                nf,
                txid,
            ));
        }
        for (id, nf, txid) in self.rederive_unmatched_orchard_spends(entries, db)? {
            recovered.push((
                id,
                pirate_storage_sqlite::models::NoteType::Orchard,
                nf,
                txid,
            ));
        }
        Ok(recovered)
    }

    /// Get current Sapling anchor from the ShardTree, if available.
    pub fn get_sapling_anchor_from_shardtree(&self) -> Option<[u8; 32]> {
        let sink = self.storage.as_ref()?;
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone()).ok()?;
        let repo = Repository::new(&db);
        let spendability = SpendabilityStateStorage::new(&db);
        let anchors = spendability
            .get_target_and_anchor_heights_by_pool_for_account(
                SPENDABILITY_MIN_CONFIRMATIONS,
                sink.account_id,
            )
            .ok()??;
        repo.resolve_sapling_root_from_db_state(anchors.sapling_anchor_height)
            .ok()?
    }

    /// Get current Orchard anchor from the ShardTree, if available.
    pub fn get_orchard_anchor_from_shardtree(&self) -> Option<[u8; 32]> {
        let sink = self.storage.as_ref()?;
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone()).ok()?;
        let repo = Repository::new(&db);
        let spendability = SpendabilityStateStorage::new(&db);
        let anchors = spendability
            .get_target_and_anchor_heights_by_pool_for_account(
                SPENDABILITY_MIN_CONFIRMATIONS,
                sink.account_id,
            )
            .ok()??;
        repo.resolve_orchard_anchor_from_db_state(anchors.orchard_anchor_height)
            .ok()?
            .map(|a| a.to_bytes())
    }

    /// Persist a ShardTree checkpoint for both pools at the requested height.
    ///
    /// This is idempotent: if both pools already have the checkpoint id, it
    /// returns successfully without mutating tree state.
    async fn create_checkpoint(
        &self,
        height: u64,
        warm_trees: Option<&mut SyncWarmTrees<'_>>,
    ) -> Result<()> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(());
        };

        let checkpoint_height = u32::try_from(height)
            .map_err(|_| Error::Sync(format!("Checkpoint height {} exceeds u32::MAX", height)))?;
        let checkpoint_id = BlockHeight::from(checkpoint_height);

        if let Some(trees) = warm_trees {
            let _ = trees.checkpoint_tip(checkpoint_id)?;
            return Ok(());
        }

        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let tx = db
            .conn()
            .unchecked_transaction()
            .map_err(|e| Error::Sync(format!("Failed to start checkpoint transaction: {}", e)))?;

        let checkpoint_exists = |table_prefix: &str| -> Result<bool> {
            let exists: i64 = tx
                .query_row(
                    &format!(
                        "SELECT EXISTS(SELECT 1 FROM {}_tree_checkpoints WHERE checkpoint_id = ?1)",
                        table_prefix
                    ),
                    [checkpoint_height],
                    |row| row.get(0),
                )
                .map_err(|e| {
                    Error::Sync(format!(
                        "Failed to query existing checkpoint {} for {}: {}",
                        checkpoint_height, table_prefix, e
                    ))
                })?;
            Ok(exists != 0)
        };

        let sapling_exists = checkpoint_exists(SAPLING_TABLE_PREFIX)?;
        let orchard_exists = checkpoint_exists(ORCHARD_TABLE_PREFIX)?;

        if !sapling_exists {
            let sapling_store =
                SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                    &tx,
                    SAPLING_TABLE_PREFIX,
                )
                .map_err(|e| Error::Sync(format!("Failed to open Sapling shard store: {}", e)))?;
            let mut sapling_tree: ShardTree<
                _,
                { NOTE_COMMITMENT_TREE_DEPTH },
                SAPLING_SHARD_HEIGHT,
            > = ShardTree::new(sapling_store, SHARDTREE_PRUNING_DEPTH);
            sapling_tree.checkpoint(checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to checkpoint Sapling shardtree at {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }

        if !orchard_exists {
            let orchard_store =
                SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                    &tx,
                    ORCHARD_TABLE_PREFIX,
                )
                .map_err(|e| Error::Sync(format!("Failed to open Orchard shard store: {}", e)))?;
            let mut orchard_tree: ShardTree<
                _,
                { NOTE_COMMITMENT_TREE_DEPTH },
                ORCHARD_SHARD_HEIGHT,
            > = ShardTree::new(orchard_store, SHARDTREE_PRUNING_DEPTH);
            orchard_tree.checkpoint(checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to checkpoint Orchard shardtree at {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }

        tx.commit()
            .map_err(|e| Error::Sync(format!("Failed to commit checkpoint transaction: {}", e)))?;
        Ok(())
    }

    /// Save sync state periodically
    async fn save_sync_state(
        &self,
        local_height: u64,
        target_height: u64,
        last_checkpoint: u64,
        include_aux_state_update: bool,
        db_session: Option<&Database>,
    ) -> Result<()> {
        if let Some(ref sink) = self.storage {
            match db_session {
                Some(db) => {
                    sink.save_sync_state_with_db(db, local_height, target_height, last_checkpoint)?
                }
                None => sink.save_sync_state(local_height, target_height, last_checkpoint)?,
            }
            if !include_aux_state_update {
                return Ok(());
            }
            let owned_db;
            let db = if let Some(db) = db_session {
                db
            } else {
                owned_db =
                    Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
                &owned_db
            };
            let scan_queue = ScanQueueStorage::new(db);
            let historic_start = (self.birthday_height as u64).max(1);
            let historic_end = local_height.saturating_add(1);
            scan_queue.record_historic_scanned_range(
                historic_start,
                historic_end.max(historic_start.saturating_add(1)),
                Some("historic_sync_progress"),
            )?;
            let _ = scan_queue.mark_found_note_done_through(local_height.saturating_add(1));
            let spendability = SpendabilityStateStorage::new(db);
            if let Some((computed_target, computed_anchor)) = spendability
                .get_target_and_anchor_heights_for_account(
                    SPENDABILITY_MIN_CONFIRMATIONS,
                    sink.account_id,
                )?
            {
                let next_found_note_row = scan_queue.next_found_note_range()?;
                let has_found_note_work = next_found_note_row.is_some();
                let current_state = spendability.load_state().unwrap_or_default();
                let at_or_past_target = local_height.saturating_add(1) >= computed_target;
                let validated_for_anchor = current_state.spendable
                    && current_state.validated_anchor_height >= computed_anchor;

                if has_found_note_work {
                    let repair_from = next_found_note_row
                        .as_ref()
                        .map(|row| row.range_start.max(1))
                        .unwrap_or_else(|| computed_anchor.max(1));
                    spendability.mark_repair_pending_without_enqueue(
                        repair_from,
                        SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED,
                    )?;
                } else if !at_or_past_target || !validated_for_anchor {
                    // Only downgrade to ERR_SYNC_FINALIZING if the wallet was NOT
                    // previously validated, or if the anchor has drifted far enough
                    // that the old validation is no longer trustworthy.
                    //
                    // When the chain advances by just a few blocks the previous
                    // validated_anchor_height falls behind the new computed_anchor,
                    // but the commitment tree at the old validated anchor is still
                    // valid — the tip witness check will re-validate shortly.
                    // Eagerly downgrading here creates a race where save_sync_state
                    // keeps undoing the validation that check_witnesses just performed,
                    // trapping the wallet in ERR_SYNC_FINALIZING forever.
                    let confirmations_u64 = u64::from(SPENDABILITY_MIN_CONFIRMATIONS);
                    let anchor_drift =
                        computed_anchor.saturating_sub(current_state.validated_anchor_height);
                    let recently_validated = current_state.spendable
                        && current_state.validated_anchor_height > 0
                        && anchor_drift <= confirmations_u64;

                    if !recently_validated {
                        spendability.mark_sync_finalizing(computed_target, computed_anchor)?;
                    }
                }
            } else {
                spendability.mark_sync_finalizing(0, 0)?;
            }
        }
        Ok(())
    }

    /// Rollback to a specific checkpoint height.
    ///
    /// Uses only ShardTree truncation (via `truncate_above_height`). Position counters
    /// are recovered from the ShardTree's checkpoint state after truncation.
    async fn rollback_to_checkpoint(&mut self, checkpoint_height: u64) -> Result<u64> {
        let Some(ref sink) = self.storage else {
            *self.sapling_tree_position.write().await = 0;
            *self.orchard_tree_position.write().await = 0;
            return Ok(checkpoint_height);
        };

        let mut db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        truncate_above_height(&mut db, checkpoint_height)?;

        self.nullifier_cache.clear();
        self.nullifier_cache_loaded = false;
        self.tracked_wallet_txids.clear();
        self.recover_position_counters_from_shardtree().await?;

        tracing::info!(
            "Rolled back to checkpoint height {} (sapling_pos={}, orchard_pos={})",
            checkpoint_height,
            *self.sapling_tree_position.read().await,
            *self.orchard_tree_position.read().await,
        );
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
        // Resume must be contiguous from the rollback point; clamping to a later "birthday"
        // can skip commitments and corrupt anchor roots.
        let resume_height = rollback_height.saturating_add(1).max(1);

        // Resume sync from next height after rollback
        self.sync_range(resume_height, None).await
    }

    /// Detect and handle reorg
    pub async fn detect_and_handle_reorg(&mut self, height: u64) -> Result<bool> {
        let local_block = self
            .storage
            .as_ref()
            .and_then(|sink| sink.load_chain_block(height).ok().flatten());

        let remote_block = match self.client.get_block(height_to_u32(height)?).await {
            Ok(block) => block,
            Err(e) => {
                tracing::warn!("Reorg check failed at height {}: {}", height, e);
                return Ok(false);
            }
        };

        if let Some(local) = local_block {
            if local.hash != remote_block.hash {
                tracing::warn!("Reorg detected at height {}", height);
                self.rollback_to_common_ancestor(height).await?;
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

    /// Fetch the latest block height from the configured lightwalletd endpoint.
    pub async fn latest_block_height(&self) -> Result<u64> {
        self.client.get_latest_block().await
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
// BridgeTree snapshot magic/version removed.

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

fn decode_sapling_rseed_from_note_bytes(note_bytes: &[u8]) -> Option<(u8, [u8; 32])> {
    if note_bytes.is_empty() {
        return None;
    }

    let expected = 1 + 43 + 1 + 32;
    if note_bytes.len() >= expected && note_bytes[0] == SAPLING_NOTE_BYTES_VERSION {
        let leadbyte = note_bytes[44];
        let mut rseed = [0u8; 32];
        rseed.copy_from_slice(&note_bytes[45..77]);
        return Some((leadbyte, rseed));
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

fn decode_orchard_rho_rseed_from_note_bytes(note_bytes: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    let expected = 1 + 43 + 32 + 32;
    if note_bytes.len() < expected || note_bytes[0] != ORCHARD_NOTE_BYTES_VERSION {
        return None;
    }
    let mut rho = [0u8; 32];
    rho.copy_from_slice(&note_bytes[44..76]);
    let mut rseed = [0u8; 32];
    rseed.copy_from_slice(&note_bytes[76..108]);
    Some((rho, rseed))
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

// BridgeTree frontier snapshot encode/decode removed -- ShardTree state is
// persisted directly in SQLite tables. No snapshot blobs needed.

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
    address_network_type: NetworkType,
}

struct PersistNotesResult {
    inserted: Vec<([u8; 32], i64)>,
    remove_from_cache: Vec<[u8; 32]>,
}

struct SyncAuxState {
    last_aux_flush_height: u64,
    last_aux_flush_at: Instant,
}

impl SyncAuxState {
    fn new(current_height: u64) -> Self {
        Self {
            last_aux_flush_height: current_height.saturating_sub(1),
            last_aux_flush_at: Instant::now(),
        }
    }

    fn should_flush(
        &self,
        local_height: u64,
        force: bool,
        end_reached: bool,
        bounded_replay: bool,
    ) -> bool {
        force
            || end_reached
            || bounded_replay
            || local_height.saturating_sub(self.last_aux_flush_height)
                >= HISTORIC_AUX_FLUSH_BLOCK_INTERVAL
            || self.last_aux_flush_at.elapsed().as_millis()
                >= HISTORIC_AUX_FLUSH_INTERVAL_MS as u128
    }

    fn mark_flushed(&mut self, local_height: u64) {
        self.last_aux_flush_height = local_height;
        self.last_aux_flush_at = Instant::now();
    }
}

impl Clone for StorageSink {
    fn clone(&self) -> Self {
        let key_bytes = *self.key.as_bytes();
        Self {
            db_path: self.db_path.clone(),
            key: EncryptionKey::from_bytes(key_bytes),
            master_key: self.master_key.clone(),
            account_id: self.account_id,
            address_network_type: self.address_network_type,
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
    ) -> Result<PersistNotesResult> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        self.persist_notes_with_db(&db, notes, tx_times, tx_fees, position_mappings)
    }

    fn persist_notes_with_db(
        &self,
        db: &Database,
        notes: &[DecryptedNote],
        tx_times: &HashMap<String, i64>,
        tx_fees: &HashMap<String, i64>,
        position_mappings: &PositionMaps,
    ) -> Result<PersistNotesResult> {
        let repo = Repository::new(db);
        let sync_state = SyncStateStorage::new(db);
        let mut inserted: Vec<([u8; 32], i64)> = Vec::new();
        let mut remove_from_cache: Vec<[u8; 32]> = Vec::new();
        let mut shard_metadata_updates: Vec<(StorageNoteType, Option<i64>, i64)> =
            Vec::with_capacity(notes.len());
        // Fast path: most batches have no owned notes.
        if notes.is_empty() {
            return Ok(PersistNotesResult {
                inserted,
                remove_from_cache,
            });
        }
        // Batch-local caches; populate lazily so we don't decrypt full tables each batch.
        let mut existing_by_output: HashMap<(Vec<u8>, i64, u8), NoteRecord> = HashMap::new();
        let mut address_cache: HashMap<String, i64> = HashMap::new();
        let candidate_output_keys: HashSet<(Vec<u8>, i64, u8)> = notes
            .iter()
            .filter(|note| !note.txid.is_empty())
            .map(|note| {
                (
                    note.txid.clone(),
                    note.output_index as i64,
                    match note.note_type {
                        crate::pipeline::NoteType::Sapling => 0u8,
                        crate::pipeline::NoteType::Orchard => 1u8,
                    },
                )
            })
            .collect();
        if !candidate_output_keys.is_empty() {
            for existing in repo.get_unspent_notes(self.account_id)? {
                let key = (
                    existing.txid.clone(),
                    existing.output_index,
                    match existing.note_type {
                        pirate_storage_sqlite::models::NoteType::Sapling => 0u8,
                        pirate_storage_sqlite::models::NoteType::Orchard => 1u8,
                    },
                );
                if candidate_output_keys.contains(&key) {
                    existing_by_output.insert(key, existing);
                }
            }
        }

        let derive_address_id = |note: &DecryptedNote,
                                 timestamp: i64,
                                 address_cache: &mut HashMap<String, i64>|
         -> Result<Option<i64>> {
            if note.note_bytes.is_empty() {
                return Ok(None);
            }
            let address_string = match note.note_type {
                crate::pipeline::NoteType::Sapling => {
                    decode_sapling_address_bytes_from_note_bytes(&note.note_bytes)
                        .and_then(|bytes| SaplingPaymentAddress::from_bytes(&bytes))
                        .map(|addr| {
                            PiratePaymentAddress { inner: addr }
                                .encode_for_network(self.address_network_type)
                        })
                }
                crate::pipeline::NoteType::Orchard => {
                    decode_orchard_address_bytes_from_note_bytes(&note.note_bytes)
                        .and_then(|bytes| {
                            Option::from(OrchardAddress::from_raw_address_bytes(&bytes))
                        })
                        .and_then(|addr| {
                            PirateOrchardPaymentAddress { inner: addr }
                                .encode_for_network(self.address_network_type)
                                .ok()
                        })
                }
            };

            let Some(address_string) = address_string else {
                return Ok(None);
            };

            if let Some(existing_id) = address_cache.get(&address_string).copied() {
                return Ok(Some(existing_id));
            }

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
            let address_id = repo
                .get_address_by_string(self.account_id, &address_string)?
                .and_then(|addr| addr.id);
            if let Some(id) = address_id {
                address_cache.insert(address_string, id);
            }
            Ok(address_id)
        };

        let candidate_unlinked_spend_keys: Vec<(
            pirate_storage_sqlite::models::NoteType,
            [u8; 32],
        )> = notes
            .iter()
            .filter_map(|n| {
                if n.nullifier.len() != 32 || n.nullifier.iter().all(|b| *b == 0) {
                    return None;
                }
                let note_type = match n.note_type {
                    crate::pipeline::NoteType::Orchard => {
                        pirate_storage_sqlite::models::NoteType::Orchard
                    }
                    crate::pipeline::NoteType::Sapling => {
                        pirate_storage_sqlite::models::NoteType::Sapling
                    }
                };
                let mut nf = [0u8; 32];
                nf.copy_from_slice(&n.nullifier[..32]);
                Some((note_type, nf))
            })
            .collect();
        let unlinked_spend_map = repo.consume_unlinked_spends_for_nullifiers(
            self.account_id,
            &candidate_unlinked_spend_keys,
        )?;
        let mut upserted_txids: HashSet<String> = HashSet::new();

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
            if verbose_note_logging_enabled() {
                pirate_core::debug_log::with_locked_file(|file| {
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
                });
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
            if upserted_txids.insert(txid_hex.clone()) {
                let _ = repo.upsert_transaction(&txid_hex, n.height as i64, timestamp, fee);
            }

            let address_id = derive_address_id(n, timestamp, &mut address_cache)?;
            let note_type_tag = match note_type {
                pirate_storage_sqlite::models::NoteType::Sapling => 0u8,
                pirate_storage_sqlite::models::NoteType::Orchard => 1u8,
            };
            let output_key = (n.txid.clone(), n.output_index as i64, note_type_tag);
            let existing_note = existing_by_output.get(&output_key).cloned();

            if let Some(existing) = existing_note {
                let mut updated = existing.clone();
                let mut changed = false;
                let existing_id = existing.id;
                let incoming_nullifier = n.nullifier.to_vec();
                let incoming_commitment = n.commitment.to_vec();
                let incoming_value = n.value as i64;
                let old_nullifier = existing.nullifier.clone();

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

                if n.key_id.is_some() && existing.key_id != n.key_id {
                    let previous = existing.key_id;
                    updated.key_id = n.key_id;
                    changed = true;
                    tracing::info!(
                        "Corrected note key_id for tx {} output {} from {:?} to {:?}",
                        txid_hex,
                        n.output_index,
                        previous,
                        n.key_id
                    );
                }

                if !n.diversifier.is_empty() {
                    let diversifier = n.diversifier.clone();
                    if existing.diversifier.as_ref() != Some(&diversifier) {
                        updated.diversifier = Some(diversifier);
                        changed = true;
                    }
                }

                if old_nullifier != updated.nullifier
                    && old_nullifier.len() == 32
                    && !old_nullifier.iter().all(|b| *b == 0)
                {
                    let mut old_nf = [0u8; 32];
                    old_nf.copy_from_slice(&old_nullifier[..32]);
                    remove_from_cache.push(old_nf);
                }
                if updated.nullifier.len() == 32 && !updated.nullifier.iter().all(|b| *b == 0) {
                    let mut nf = [0u8; 32];
                    nf.copy_from_slice(&updated.nullifier[..32]);
                    if let Some(spent_txid) = unlinked_spend_map.get(&(note_type, nf)).copied() {
                        if !updated.spent
                            || updated
                                .spent_txid
                                .as_deref()
                                .map(|v| v != spent_txid.as_slice())
                                .unwrap_or(true)
                        {
                            updated.spent = true;
                            updated.spent_txid = Some(spent_txid.to_vec());
                            changed = true;
                        }
                        if changed || !updated.spent {
                            remove_from_cache.push(nf);
                        }
                    } else if !updated.spent {
                        if let Some(id) = existing_id {
                            inserted.push((nf, id));
                        }
                    }
                }
                if changed {
                    repo.update_note_by_id_without_shard_metadata(&updated)?;
                }
                shard_metadata_updates.push((updated.note_type, updated.position, updated.height));
                existing_by_output.insert(output_key, updated);
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
                note: if !n.note_bytes.is_empty() {
                    Some(n.note_bytes.clone())
                } else {
                    None
                },
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
            let mut record = record;
            if record.nullifier.len() == 32 && !record.nullifier.iter().all(|b| *b == 0) {
                let mut nf = [0u8; 32];
                nf.copy_from_slice(&record.nullifier[..32]);
                if let Some(spent_txid) = unlinked_spend_map.get(&(note_type, nf)).copied() {
                    record.spent = true;
                    record.spent_txid = Some(spent_txid.to_vec());
                }
            }
            match repo.insert_note_without_shard_metadata(&record) {
                Ok(id) => {
                    let mut stored = record.clone();
                    stored.id = Some(id);
                    if record.nullifier.len() == 32 && !record.nullifier.iter().all(|b| *b == 0) {
                        let mut nf = [0u8; 32];
                        nf.copy_from_slice(&record.nullifier[..32]);
                        if stored.spent {
                            remove_from_cache.push(nf);
                        } else {
                            inserted.push((nf, id));
                        }
                    }
                    shard_metadata_updates.push((stored.note_type, stored.position, stored.height));
                    existing_by_output.insert(output_key, stored);
                }
                Err(e) => {
                    // #region agent log
                    if verbose_note_logging_enabled() {
                        pirate_core::debug_log::with_locked_file(|file| {
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
                        });
                    }
                    // #endregion
                }
            }
        }
        repo.upsert_note_shard_metadata_batch(shard_metadata_updates.into_iter())?;
        // Optionally update sync state height
        if let Some(max_h) = notes.iter().map(|n| n.height).max() {
            let _ = sync_state.save_sync_state(max_h, max_h, max_h);
        }
        Ok(PersistNotesResult {
            inserted,
            remove_from_cache,
        })
    }

    fn get_note_by_txid_and_index(
        &self,
        txid: &[u8],
        output_index: i64,
        note_type: NoteType,
    ) -> Result<Option<NoteRecord>> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        let note_type = match note_type {
            NoteType::Sapling => pirate_storage_sqlite::models::NoteType::Sapling,
            NoteType::Orchard => pirate_storage_sqlite::models::NoteType::Orchard,
        };
        Ok(repo.get_note_by_txid_and_index_with_type(
            self.account_id,
            txid,
            output_index,
            Some(note_type),
        )?)
    }

    fn list_orchard_note_refs(&self) -> Result<Vec<OrchardNoteRef>> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.get_orchard_note_refs(self.account_id)?)
    }

    fn update_note_memo(
        &self,
        txid: &[u8],
        output_index: i64,
        note_type: NoteType,
        memo: Option<&[u8]>,
    ) -> Result<()> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        let note_type = match note_type {
            NoteType::Sapling => pirate_storage_sqlite::models::NoteType::Sapling,
            NoteType::Orchard => pirate_storage_sqlite::models::NoteType::Orchard,
        };
        Ok(repo.update_note_memo_with_type(
            self.account_id,
            txid,
            output_index,
            Some(note_type),
            memo,
        )?)
    }

    fn apply_spend_updates_with_txmeta(
        &self,
        spend_updates: &[(i64, [u8; 32])],
        fallback_entries: &[([u8; 32], [u8; 32])],
        tx_meta: &[(String, i64, i64, i64)],
    ) -> Result<(u64, u64)> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        self.apply_spend_updates_with_txmeta_with_db(&db, spend_updates, fallback_entries, tx_meta)
    }

    fn apply_spend_updates_with_txmeta_with_db(
        &self,
        db: &Database,
        spend_updates: &[(i64, [u8; 32])],
        fallback_entries: &[([u8; 32], [u8; 32])],
        tx_meta: &[(String, i64, i64, i64)],
    ) -> Result<(u64, u64)> {
        let repo = Repository::new(db);
        Ok(repo.apply_spend_updates_with_txmeta(
            self.account_id,
            spend_updates,
            fallback_entries,
            tx_meta,
        )?)
    }

    fn upsert_unlinked_spend_nullifiers_with_txid(
        &self,
        entries: &[(pirate_storage_sqlite::models::NoteType, [u8; 32], [u8; 32])],
    ) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.upsert_unlinked_spend_nullifiers_with_txid(self.account_id, entries)?)
    }

    fn upsert_tx_memo(&self, txid_hex: &str, memo: &[u8]) -> Result<()> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.upsert_tx_memo(txid_hex, memo)?)
    }

    fn get_tx_memo(&self, txid_hex: &str) -> Result<Option<Vec<u8>>> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let repo = Repository::new(&db);
        Ok(repo.get_tx_memo(txid_hex)?)
    }

    fn load_sync_state(&self) -> Result<pirate_storage_sqlite::sync_state::SyncStateRow> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let sync_state = SyncStateStorage::new(&db);
        Ok(sync_state.load_sync_state()?)
    }

    fn save_sync_state(
        &self,
        local_height: u64,
        target_height: u64,
        last_checkpoint_height: u64,
    ) -> Result<()> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        self.save_sync_state_with_db(&db, local_height, target_height, last_checkpoint_height)
    }

    fn save_sync_state_with_db(
        &self,
        db: &Database,
        local_height: u64,
        target_height: u64,
        last_checkpoint_height: u64,
    ) -> Result<()> {
        let sync_state = SyncStateStorage::new(db);
        Ok(sync_state.save_sync_state(local_height, target_height, last_checkpoint_height)?)
    }

    fn save_chain_blocks_with_db(&self, db: &Database, blocks: &[CompactBlockData]) -> Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }
        let rows: Vec<ChainBlockRow> = blocks
            .iter()
            .map(|block| ChainBlockRow {
                height: block.height,
                hash: block.hash.clone(),
                prev_hash: block.prev_hash.clone(),
                time: block.time,
            })
            .collect();
        let sync_state = SyncStateStorage::new(db);
        Ok(sync_state.save_chain_blocks(&rows)?)
    }

    fn load_chain_block(&self, height: u64) -> Result<Option<ChainBlockRow>> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let sync_state = SyncStateStorage::new(&db);
        Ok(sync_state.load_chain_block(height)?)
    }

    fn load_latest_chain_block(&self) -> Result<Option<ChainBlockRow>> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let sync_state = SyncStateStorage::new(&db);
        Ok(sync_state.load_latest_chain_block()?)
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

#[derive(Debug, Default, Clone)]
struct DecryptBackendTelemetry {
    cpu_ms: u128,
}

#[derive(Debug, Default, Clone)]
struct TrialDecryptTelemetry {
    cpu_ms: u128,
}

impl TrialDecryptTelemetry {
    fn merge_stage(&mut self, stage: &DecryptBackendTelemetry, _note_type: NoteType) {
        self.cpu_ms += stage.cpu_ms;
    }
}

struct TrialDecryptBatchResult {
    notes: Vec<DecryptedNote>,
    telemetry: TrialDecryptTelemetry,
}

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

fn try_compact_note_decryption_backend<D, Output>(
    pool: &rayon::ThreadPool,
    ivks: &[D::IncomingViewingKey],
    outputs: &[(D, Output)],
    max_parallel: usize,
) -> (Vec<CompactDecryptResult<D>>, DecryptBackendTelemetry)
where
    D: zcash_note_encryption::BatchDomain + Sync,
    Output: ShieldedOutput<D, COMPACT_NOTE_SIZE> + Sync,
    D::IncomingViewingKey: Sync,
    D::Note: Send,
    D::Recipient: Send,
{
    let started = Instant::now();
    let results = try_compact_note_decryption_parallel(pool, ivks, outputs, max_parallel);
    let telemetry = DecryptBackendTelemetry {
        cpu_ms: started.elapsed().as_millis(),
    };
    (results, telemetry)
}

fn trial_decrypt_batch_impl(
    inputs: TrialDecryptBatchInputs<'_>,
) -> Result<TrialDecryptBatchResult> {
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
    let mut telemetry = TrialDecryptTelemetry::default();

    if !sapling_ivks.is_empty() && !sapling_outputs.is_empty() {
        let (sapling_results, sapling_telemetry) = try_compact_note_decryption_backend(
            decrypt_pool,
            sapling_ivks,
            &sapling_outputs,
            max_parallel,
        );
        telemetry.merge_stage(&sapling_telemetry, NoteType::Sapling);

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
        let (orchard_results, orchard_telemetry) = try_compact_note_decryption_backend(
            decrypt_pool,
            orchard_ivks,
            &orchard_outputs,
            max_parallel,
        );
        telemetry.merge_stage(&orchard_telemetry, NoteType::Orchard);

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

    Ok(TrialDecryptBatchResult { notes, telemetry })
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
    let batch = trial_decrypt_batch_impl(TrialDecryptBatchInputs {
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
    })?;
    Ok(batch.notes)
}

// DecryptedNote is imported from pipeline module - no need to redefine here

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.checkpoint_interval, 10_000);
        assert_eq!(config.batch_size, 4_000);
        assert_eq!(config.max_batch_size, 4_000);
        assert_eq!(config.target_batch_bytes, 128_000_000);
        assert!(config.lazy_memo_decode);
    }

    #[tokio::test]
    async fn test_sync_engine_creation() {
        let engine = SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
        assert_eq!(engine.birthday_height(), 3_800_000);
    }

    #[tokio::test]
    async fn test_compute_batch_end_treats_server_hint_as_cap() {
        let engine = SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
        let mut server_group_end_hint = Some(100_198);
        let mut pending_server_group_hint = None;
        let tuning = BatchTuning {
            target_bytes: 128_000_000,
            avg_block_size_estimate: 16_000,
            max_batch_blocks: 4_000,
        };

        let (batch_end, desired_blocks) = engine
            .compute_batch_end(
                100_000,
                110_000,
                tuning,
                &mut server_group_end_hint,
                &mut pending_server_group_hint,
            )
            .await
            .unwrap();

        assert_eq!(desired_blocks, 4_000);
        assert_eq!(batch_end, 100_198);
    }

    #[tokio::test]
    async fn test_compute_batch_end_respects_adaptive_network_cap() {
        let engine = SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
        let mut server_group_end_hint = Some(110_000);
        let mut pending_server_group_hint = None;
        let tuning = BatchTuning {
            target_bytes: 128_000_000,
            avg_block_size_estimate: 16_000,
            max_batch_blocks: 500,
        };

        let (batch_end, desired_blocks) = engine
            .compute_batch_end(
                100_000,
                110_000,
                tuning,
                &mut server_group_end_hint,
                &mut pending_server_group_hint,
            )
            .await
            .unwrap();

        assert_eq!(desired_blocks, 500);
        assert_eq!(batch_end, 100_499);
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

        let result = SyncEngine::fetch_blocks_with_retry_inner(client, 10, 20, cancel, None).await;
        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[tokio::test]
    async fn test_fetch_blocks_with_retry_empty_range() {
        let client = LightClient::new("http://127.0.0.1:1".to_string());
        let cancel = CancelToken::new();

        let blocks = SyncEngine::fetch_blocks_with_retry_inner(client, 20, 10, cancel, None)
            .await
            .unwrap();
        assert!(blocks.is_empty());
    }
}
