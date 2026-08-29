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

use crate::block_cache::{acquire_inflight, BlockCache, InflightLease, InflightToken};
use crate::client::{CompactBlockData, LightdInfo, TransportMode, TreeState};
use crate::intake::{
    local_batch_prefix_len, AdaptiveDurableSegmentController, AdaptiveScanBatcher,
    AdaptiveShieldedWorkBatcher, DurableSegmentObservation, LocalBatchWeight, PrefetchReservation,
    PrefetchWatermarks, ScanBatchObservation, ScanBatchSource, DEFAULT_DURABLE_SEGMENT_BLOCKS,
    DEFAULT_NETWORK_SCAN_BATCH_TARGET, DURABLE_SEGMENT_CHANNEL_CAPACITY,
    LOCAL_SCAN_BATCH_CHANNEL_CAPACITY,
};
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
use incrementalmerkletree::{Marking, Position, Retention};
use orchard::keys::{
    Diversifier as OrchardDiversifier, IncomingViewingKey as IronwoodIncomingViewingKey,
    PreparedIncomingViewingKey as OrchardPreparedIncomingViewingKey,
};
use orchard::note::{
    ExtractedNoteCommitment as OrchardExtractedNoteCommitment, Note as OrchardNote,
    NoteVersion as OrchardNoteVersion, Nullifier as OrchardNullifier,
    RandomSeed as OrchardRandomSeed, Rho as OrchardRho,
};
use orchard::note_encryption::{CompactAction, IronwoodDomain};
use orchard::tree::MerkleHashOrchard;
use orchard::value::NoteValue as OrchardNoteValue;
use orchard::Address as OrchardAddress;
use pirate_core::keys::{
    DiversifierScope, ExtendedFullViewingKey, ExtendedSpendingKey, IronwoodExtendedFullViewingKey,
    IronwoodExtendedSpendingKey, IronwoodPaymentAddress as PirateIronwoodPaymentAddress,
    PaymentAddress as PiratePaymentAddress,
};
use pirate_core::transaction::{read_pirate_transaction, PirateNetwork};
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
use sapling::keys::{OutgoingViewingKey as SaplingOutgoingViewingKey, PreparedIncomingViewingKey};
use sapling::note_encryption::{try_sapling_output_recovery, SaplingDomain};
use sapling::{
    note::ExtractedNoteCommitment as SaplingExtractedNoteCommitment, Node as SaplingNode,
    PaymentAddress as SaplingPaymentAddress, Rseed, SaplingIvk, NOTE_COMMITMENT_TREE_DEPTH,
};
use shardtree::store::caching::{CachingShardStore, SparseCachingShardStore};
use shardtree::ShardTree;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use subtle::CtOption;
use tokio::sync::{mpsc, oneshot, RwLock};
use tonic::Code;
use zcash_note_encryption::try_output_recovery_with_ovk;
use zcash_note_encryption::{
    batch as note_batch, EphemeralKeyBytes, ShieldedOutput, COMPACT_NOTE_SIZE,
};
use zcash_primitives::merkle_tree::{
    read_commitment_tree, read_frontier_v0, read_frontier_v1, HashSer,
};
use zcash_primitives::transaction::components::sapling::zip212_enforcement;
use zcash_protocol::consensus::BlockHeight;
use zip32::Scope as SaplingScope;

mod shardtree_support;

use self::shardtree_support::{
    append_orchard_leaf, append_sapling_leaf, apply_shardtree_batches_to_trees,
    drain_historical_skip_state, fetch_remote_historical_subtree_roots,
    log_shardtree_persistence_telemetry, merge_emitted_batches, persist_verified_pool_roots,
    prepare_historical_subtree_roots, process_historical_leaf, sparse_preload_addresses,
    warm_shardtree_cache_with_subtrees_enabled, CommittedCheckpointHeights, HistoricalLeafSink,
    HistoricalPrefillState, HistoricalSubtreeRootRequest, PersistenceShardTrees,
    RemoteHistoricalSubtreeRoots, ShardtreeBatch, ShardtreePersistResult, SyncWarmTrees,
    VerifiedSubtreeRoots,
};

type StorageNoteType = pirate_storage_sqlite::models::NoteType;
type NullifierBytes = [u8; 32];
type TxidBytes = [u8; 32];
type TypedSpendEntry = (StorageNoteType, NullifierBytes, TxidBytes);
type RecoveredSpend = (i64, NullifierBytes, TxidBytes);
type TypedRecoveredSpend = (i64, StorageNoteType, NullifierBytes, TxidBytes);

fn verbose_note_logging_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match env::var("PIRATE_VERBOSE_NOTE_LOGS") {
        Ok(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
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
        Err(_) => false,
    })
}

fn sync_performance_logging_enabled() -> bool {
    pirate_core::debug_log::is_enabled() || verbose_sync_batch_logging_enabled()
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
const SYNC_KEY_INVENTORY_LOG_SCHEMA_VERSION: u8 = 2;
const MIN_PERSISTENCE_SHARDTREE_CACHE_BYTES: u64 = 8_000_000;
const DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES: u64 = 64_000_000;
const MAX_PERSISTENCE_SHARDTREE_CACHE_BYTES: u64 = 128_000_000;

fn persistence_shardtree_cache_limit(max_batch_memory_bytes: Option<u64>) -> u64 {
    max_batch_memory_bytes
        .map(|bytes| bytes / 4)
        .unwrap_or(DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES)
        .clamp(
            MIN_PERSISTENCE_SHARDTREE_CACHE_BYTES,
            MAX_PERSISTENCE_SHARDTREE_CACHE_BYTES,
        )
}

/// Shard height used for subtree-addressed spendability/repair scheduling.
fn build_key_group_from_account_key(
    key: &AccountKey,
    seed_derivation: Option<(u32, bool)>,
) -> Result<Option<WalletKeyGroup>> {
    let key_id = key.id.unwrap_or(0);

    let sapling_dfvk = if let Some(ref extsk_bytes) = key.sapling_extsk {
        let extsk = ExtendedSpendingKey::from_bytes(extsk_bytes)
            .map_err(|e| Error::Sync(format!("Invalid Sapling spending key bytes: {}", e)))?;
        Some(extsk.to_extended_fvk())
    } else if let Some(ref bytes) = key.sapling_dfvk {
        ExtendedFullViewingKey::from_bytes(bytes)
    } else {
        None
    };

    let orchard_fvk = if let Some(ref extsk_bytes) = key.orchard_extsk {
        let extsk = IronwoodExtendedSpendingKey::from_bytes(extsk_bytes)
            .map_err(|e| Error::Sync(format!("Invalid Ironwood spending key bytes: {}", e)))?;
        Some(extsk.to_extended_fvk())
    } else if let Some(ref bytes) = key.orchard_fvk {
        Some(
            IronwoodExtendedFullViewingKey::from_bytes(bytes)
                .map_err(|e| Error::Sync(format!("Invalid Ironwood viewing key bytes: {}", e)))?,
        )
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
        key_type: key.key_type,
        seed_derivation_index: seed_derivation.map(|(index, _)| index),
        discovery_candidate: seed_derivation.is_some_and(|(_, candidate)| candidate),
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
    /// Minimum batch size for dense or full blocks
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
    /// Minimum batch size in bytes during dense block ranges
    pub min_batch_bytes: u64,
    /// Maximum batch size in bytes (cap for large batches)
    pub max_batch_bytes: u64,
    /// Threshold for detecting unusually large blocks (bytes per block)
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
    /// Trial-decrypt exactly one fetched batch while the preceding batch is persisted.
    pub one_batch_ahead_decryption: bool,
    /// Experimental A/B switch that splits lookahead decryption into additional
    /// ordered tasks. Disabled by default because the release benchmark showed
    /// no critical-path benefit over Rayon's existing work stealing.
    pub stage_aware_cpu_scheduling: bool,
}

/// Constants for retry logic
#[cfg(test)]
const MAX_RETRY_ATTEMPTS: u32 = 3;
#[cfg(test)]
const RETRY_BACKOFF_MS: u64 = 100;
// BridgeTree snapshot retention removed -- ShardTree is persistent in SQLite.
const MIN_PARALLEL_OUTPUTS: usize = 96;
const MIN_PARALLEL_DECRYPT_CHUNK: usize = 64;
const SPENDABILITY_REASON_ERR_WITNESS_REPAIR_QUEUED: &str = "ERR_WITNESS_REPAIR_QUEUED";
const SPENDABILITY_MIN_CONFIRMATIONS: u32 = 1;
const LOW_HEIGHT_BATCH_CAP_HEIGHT: u64 = 10_000;
#[cfg(test)]
const LOW_HEIGHT_BATCH_MAX_BLOCKS: u64 = 1_024;
const HISTORIC_AUX_FLUSH_BLOCK_INTERVAL: u64 = 25_000;
const HISTORIC_AUX_FLUSH_INTERVAL_MS: u64 = 30_000;
const HISTORIC_SPARSE_CHECKPOINT_INTERVAL: u64 = 50_000;
const MAX_REORG_SEARCH_DEPTH: u64 = 2_000;
const CANONICAL_BLOCK_WINDOW: usize = MAX_REORG_SEARCH_DEPTH as usize + 1;

fn resume_chain_network_timeout(transport: TransportMode) -> Duration {
    match transport {
        TransportMode::Direct => Duration::from_secs(30),
        TransportMode::Tor | TransportMode::Socks5 => Duration::from_secs(90),
        TransportMode::I2p => Duration::from_secs(180),
    }
}

fn reorg_backoff_probe(divergent_height: u64, stop_height: u64, distance: u64) -> u64 {
    divergent_height.saturating_sub(distance).max(stop_height)
}
const I2P_SERVER_INFO_TIMEOUT: Duration = Duration::from_secs(45);
const I2P_SERVER_INFO_ATTEMPTS: usize = 2;
#[cfg(test)]
const SERVER_BATCH_GROUP_TARGET_BYTES: u64 = 4_000_000;
#[cfg(test)]
const TARGET_FETCH_BATCH_MS: u128 = 5_000;
#[cfg(test)]
const MAX_BATCH_CAP_GROWTH_FACTOR: u64 = 2;
#[cfg(test)]
const MAX_CACHED_BATCH_BLOCKS: u64 = 16_000;
const LOOKAHEAD_DECRYPT_TASK_MULTIPLIER: usize = 4;

// Keep the batch tip and every ancestor that find_common_ancestor can inspect.
fn canonical_block_window(blocks: &[CompactBlockData]) -> &[CompactBlockData] {
    let start = blocks.len().saturating_sub(CANONICAL_BLOCK_WINDOW);
    &blocks[start..]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TipWitnessValidationOutcome {
    Clean,
    RepairQueued { start: u64, end_exclusive: u64 },
    Error,
}

struct WitnessCheckDbOutcome {
    repair_range: Option<(u64, u64)>,
    checked: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ServerInfoValidationPolicy {
    attempt_timeout: Duration,
    max_attempts: usize,
}

fn server_info_validation_policy(transport: TransportMode) -> ServerInfoValidationPolicy {
    match transport {
        TransportMode::Direct => ServerInfoValidationPolicy {
            attempt_timeout: Duration::from_secs(5),
            max_attempts: 1,
        },
        TransportMode::I2p => ServerInfoValidationPolicy {
            attempt_timeout: I2P_SERVER_INFO_TIMEOUT,
            max_attempts: I2P_SERVER_INFO_ATTEMPTS,
        },
        TransportMode::Tor | TransportMode::Socks5 => ServerInfoValidationPolicy {
            attempt_timeout: Duration::from_secs(15),
            max_attempts: 1,
        },
    }
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
            min_batch_size,
        ) = if is_mobile {
            (
                4,
                Some(64_000_000),
                4_000_000,
                1_000_000,
                8_000_000,
                1,
                8_000_000,
                2_000,
                2_000,
                3,
                1_500,
                25,
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
                100,
            )
        };

        Self {
            checkpoint_interval: 10_000,
            batch_size,     // Used when server recommendations disabled/unavailable.
            min_batch_size, // Minimum batch size for dense block ranges
            max_batch_size, // Maximum batch size (caps server batches to prevent OOM)
            use_server_batch_recommendations: true, // Use server's ~4MB chunk recommendations (typically ~199 blocks)
            mini_checkpoint_every: 5,               // Mini-checkpoint every 5 batches
            mini_checkpoint_max_block_gap: 20_000,  // Always checkpoint at least every 20k blocks
            max_parallel_decrypt,
            lazy_memo_decode: true,
            defer_full_tx_fetch: true,
            target_batch_bytes,
            min_batch_bytes,
            max_batch_bytes,
            heavy_block_threshold_bytes: 500_000, // 500KB per block triggers conservative byte sizing
            max_batch_memory_bytes,
            sync_state_flush_every_batches,
            sync_state_flush_interval_ms,
            prefetch_queue_depth,
            prefetch_queue_max_bytes,
            one_batch_ahead_decryption: true,
            stage_aware_cpu_scheduling: false,
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
    /// Fixed or chain-derived Ironwood activation height (`0` while unresolved).
    ironwood_activation_height: Arc<AtomicU64>,
    wallet_id: Option<String>,
    storage: Option<StorageSink>,
    keys: Vec<WalletKeyGroup>,
    trial_decrypt_keys: TrialDecryptKeys,
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
    /// Immutable optimization hints captured before a rescan removes derived
    /// note rows. Hinted subtrees are scanned leaf-by-leaf instead of grafted.
    historical_sapling_mark_subtrees: HashSet<u64>,
    historical_ironwood_mark_subtrees: HashSet<u64>,
}

enum PrefetchPayload {
    Fetch {
        receiver: mpsc::Receiver<Result<FetchedBlockBatch>>,
        handle: tokio::task::JoinHandle<()>,
    },
    Decrypt {
        handle: tokio::task::JoinHandle<Result<DecryptLookaheadOutput>>,
        producer_abort: tokio::task::AbortHandle,
    },
}

struct PrefetchTask {
    start: u64,
    end: u64,
    payload: Option<PrefetchPayload>,
}

struct DecryptLookaheadOutput {
    fetched: FetchedBlockBatch,
    notes: Vec<DecryptedNote>,
    telemetry: TrialDecryptTelemetry,
    prepared_commitments: PreparedCommitmentBatch,
    receiver: mpsc::Receiver<Result<FetchedBlockBatch>>,
    producer_handle: tokio::task::JoinHandle<()>,
}

struct ReceivedPrefetchBatch {
    fetched: FetchedBlockBatch,
    prepared_notes: Option<(Vec<DecryptedNote>, TrialDecryptTelemetry)>,
    prepared_commitments: Option<PreparedCommitmentBatch>,
}

struct PreparedSaplingCommitment {
    output_index: usize,
    commitment: [u8; 32],
    node: Option<SaplingNode>,
}

struct PreparedIronwoodCommitment {
    commitment: [u8; 32],
    node: Option<MerkleHashOrchard>,
}

struct PreparedCommitmentTransaction {
    hash: Vec<u8>,
    sapling: Vec<PreparedSaplingCommitment>,
    ironwood: Vec<PreparedIronwoodCommitment>,
}

struct PreparedBlockCommitments {
    height: u64,
    hash: Vec<u8>,
    transactions: Vec<PreparedCommitmentTransaction>,
}

struct PreparedCommitmentBatch {
    blocks: Vec<PreparedBlockCommitments>,
    elapsed: Duration,
    sapling_count: usize,
    ironwood_count: usize,
}

impl PreparedCommitmentBatch {
    fn validate_source(&self, blocks: &[CompactBlockData]) -> Result<()> {
        if self.blocks.len() != blocks.len()
            || self.blocks.iter().zip(blocks).any(|(prepared, source)| {
                prepared.height != source.height || prepared.hash != source.hash
            })
        {
            return Err(Error::Sync(
                "Prepared commitment batch does not match its validated compact-block source"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

type PersistenceDatabaseOperation = Box<dyn FnOnce(&Database) + Send + 'static>;

enum PersistenceOperation {
    Execute(PersistenceDatabaseOperation),
    InvalidateAndExecute(PersistenceDatabaseOperation),
    PersistShardtrees {
        batches: Vec<ShardtreeBatch>,
        batch_end_height: Option<u64>,
        verified_roots: VerifiedSubtreeRoots,
        response: oneshot::Sender<Result<ShardtreePersistResult>>,
    },
    Checkpoint {
        checkpoint_id: BlockHeight,
        response: oneshot::Sender<Result<()>>,
    },
    RetainCheckpoint {
        checkpoint_id: BlockHeight,
        response: oneshot::Sender<Result<()>>,
    },
}

struct PersistenceWorker {
    sender: Option<std_mpsc::Sender<PersistenceOperation>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl PersistenceWorker {
    #[cfg(test)]
    fn start(sink: StorageSink, shardtree_cache_limit_bytes: u64) -> Result<Self> {
        let construction_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_cpus::get().clamp(1, 4))
            .thread_name(|index| format!("shardtree-build-{}", index))
            .build()
            .map(Arc::new)
            .map_err(|error| {
                Error::Sync(format!(
                    "Failed to start ShardTree construction pool: {}",
                    error
                ))
            })?;
        Self::start_with_pool(sink, shardtree_cache_limit_bytes, construction_pool)
    }

    fn start_with_pool(
        sink: StorageSink,
        shardtree_cache_limit_bytes: u64,
        construction_pool: Arc<rayon::ThreadPool>,
    ) -> Result<Self> {
        let (sender, receiver) = std_mpsc::channel::<PersistenceOperation>();
        let (ready_sender, ready_receiver) = std_mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("pirate-sync-persistence".to_string())
            .spawn(move || {
                let db = match Database::open_existing(
                    &sink.db_path,
                    &sink.key,
                    sink.master_key.clone(),
                ) {
                    Ok(db) => {
                        let _ = ready_sender.send(Ok(()));
                        db
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(error.to_string()));
                        return;
                    }
                };
                let mut shardtrees: Option<PersistenceShardTrees<'_>> = None;
                while let Ok(operation) = receiver.recv() {
                    match operation {
                        PersistenceOperation::Execute(operation) => operation(&db),
                        PersistenceOperation::InvalidateAndExecute(operation) => {
                            invalidate_persistence_shardtrees(
                                &mut shardtrees,
                                "external_tree_mutation",
                            );
                            operation(&db);
                        }
                        PersistenceOperation::PersistShardtrees {
                            batches,
                            batch_end_height,
                            verified_roots,
                            response,
                        } => {
                            if shardtrees.is_none() {
                                match PersistenceShardTrees::load(
                                    db.conn(),
                                    shardtree_cache_limit_bytes,
                                ) {
                                    Ok(loaded) => shardtrees = Some(loaded),
                                    Err(error) => {
                                        let _ = response.send(Err(error));
                                        continue;
                                    }
                                }
                            }
                            let result = shardtrees
                                .as_mut()
                                .expect("persistence shardtrees loaded")
                                .persist_owned_batches_with_roots(
                                    &db,
                                    batches,
                                    batch_end_height,
                                    &verified_roots,
                                    construction_pool.as_ref(),
                                );
                            match result {
                                Ok((persisted, telemetry, evict)) => {
                                    log_shardtree_persistence_telemetry(
                                        "persist_batches",
                                        &telemetry,
                                    );
                                    if evict {
                                        invalidate_persistence_shardtrees(
                                            &mut shardtrees,
                                            "memory_limit",
                                        );
                                    }
                                    if response.send(Ok(persisted)).is_err() {
                                        invalidate_persistence_shardtrees(
                                            &mut shardtrees,
                                            "cancelled_batch_response",
                                        );
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        "Invalidating persistence ShardTree cache after failed batch: {}",
                                        error
                                    );
                                    invalidate_persistence_shardtrees(
                                        &mut shardtrees,
                                        "failed_batch",
                                    );
                                    let _ = response.send(Err(error));
                                }
                            }
                        }
                        PersistenceOperation::Checkpoint {
                            checkpoint_id,
                            response,
                        } => {
                            if shardtrees.is_none() {
                                match PersistenceShardTrees::load(
                                    db.conn(),
                                    shardtree_cache_limit_bytes,
                                ) {
                                    Ok(loaded) => shardtrees = Some(loaded),
                                    Err(error) => {
                                        let _ = response.send(Err(error));
                                        continue;
                                    }
                                }
                            }
                            let result = shardtrees
                                .as_mut()
                                .expect("persistence shardtrees loaded")
                                .checkpoint_tip(&db, checkpoint_id);
                            match result {
                                Ok((telemetry, evict)) => {
                                    log_shardtree_persistence_telemetry(
                                        "checkpoint_tip",
                                        &telemetry,
                                    );
                                    if evict {
                                        invalidate_persistence_shardtrees(
                                            &mut shardtrees,
                                            "memory_limit",
                                        );
                                    }
                                    if response.send(Ok(())).is_err() {
                                        invalidate_persistence_shardtrees(
                                            &mut shardtrees,
                                            "cancelled_checkpoint_response",
                                        );
                                    }
                                }
                                Err(error) => {
                                    invalidate_persistence_shardtrees(
                                        &mut shardtrees,
                                        "failed_checkpoint",
                                    );
                                    let _ = response.send(Err(error));
                                }
                            }
                        }
                        PersistenceOperation::RetainCheckpoint {
                            checkpoint_id,
                            response,
                        } => {
                            if shardtrees.is_none() {
                                match PersistenceShardTrees::load(
                                    db.conn(),
                                    shardtree_cache_limit_bytes,
                                ) {
                                    Ok(loaded) => shardtrees = Some(loaded),
                                    Err(error) => {
                                        let _ = response.send(Err(error));
                                        continue;
                                    }
                                }
                            }
                            let result = shardtrees
                                .as_mut()
                                .expect("persistence shardtrees loaded")
                                .retain_checkpoint(&db, checkpoint_id);
                            match result {
                                Ok((telemetry, evict)) => {
                                    log_shardtree_persistence_telemetry(
                                        "retain_checkpoint",
                                        &telemetry,
                                    );
                                    if evict {
                                        invalidate_persistence_shardtrees(
                                            &mut shardtrees,
                                            "memory_limit",
                                        );
                                    }
                                    if response.send(Ok(())).is_err() {
                                        invalidate_persistence_shardtrees(
                                            &mut shardtrees,
                                            "cancelled_retained_checkpoint_response",
                                        );
                                    }
                                }
                                Err(error) => {
                                    invalidate_persistence_shardtrees(
                                        &mut shardtrees,
                                        "failed_retained_checkpoint",
                                    );
                                    let _ = response.send(Err(error));
                                }
                            }
                        }
                    }
                }
            })
            .map_err(|error| {
                Error::Sync(format!("Failed to start persistence worker: {}", error))
            })?;
        match ready_receiver.recv() {
            Ok(Ok(())) => Ok(Self {
                sender: Some(sender),
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(Error::Sync(format!(
                    "Failed to open persistence database: {}",
                    error
                )))
            }
            Err(error) => {
                let _ = thread.join();
                Err(Error::Sync(format!(
                    "Persistence worker stopped during startup: {}",
                    error
                )))
            }
        }
    }

    async fn execute<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T> + Send + 'static,
    {
        let (response_sender, response_receiver) = oneshot::channel();
        let job = Box::new(move |db: &Database| {
            let _ = response_sender.send(operation(db));
        });
        self.sender
            .as_ref()
            .ok_or_else(|| Error::Sync("Persistence worker is closed".to_string()))?
            .send(PersistenceOperation::Execute(job))
            .map_err(|_| Error::Sync("Persistence worker stopped unexpectedly".to_string()))?;
        response_receiver
            .await
            .map_err(|_| Error::Sync("Persistence worker dropped a response".to_string()))?
    }

    async fn execute_invalidating_shardtrees<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Database) -> Result<T> + Send + 'static,
    {
        let (response_sender, response_receiver) = oneshot::channel();
        let job = Box::new(move |db: &Database| {
            let _ = response_sender.send(operation(db));
        });
        self.sender
            .as_ref()
            .ok_or_else(|| Error::Sync("Persistence worker is closed".to_string()))?
            .send(PersistenceOperation::InvalidateAndExecute(job))
            .map_err(|_| Error::Sync("Persistence worker stopped unexpectedly".to_string()))?;
        response_receiver
            .await
            .map_err(|_| Error::Sync("Persistence worker dropped a response".to_string()))?
    }

    async fn persist_shardtree_batches(
        &self,
        batches: Vec<ShardtreeBatch>,
        batch_end_height: Option<u64>,
    ) -> Result<ShardtreePersistResult> {
        self.persist_shardtree_batches_with_roots(
            batches,
            batch_end_height,
            VerifiedSubtreeRoots::default(),
        )
        .await
    }

    async fn persist_shardtree_batches_with_roots(
        &self,
        batches: Vec<ShardtreeBatch>,
        batch_end_height: Option<u64>,
        verified_roots: VerifiedSubtreeRoots,
    ) -> Result<ShardtreePersistResult> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or_else(|| Error::Sync("Persistence worker is closed".to_string()))?
            .send(PersistenceOperation::PersistShardtrees {
                batches,
                batch_end_height,
                verified_roots,
                response,
            })
            .map_err(|_| Error::Sync("Persistence worker stopped unexpectedly".to_string()))?;
        receiver
            .await
            .map_err(|_| Error::Sync("Persistence worker dropped a response".to_string()))?
    }

    async fn checkpoint_shardtrees(&self, checkpoint_id: BlockHeight) -> Result<()> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or_else(|| Error::Sync("Persistence worker is closed".to_string()))?
            .send(PersistenceOperation::Checkpoint {
                checkpoint_id,
                response,
            })
            .map_err(|_| Error::Sync("Persistence worker stopped unexpectedly".to_string()))?;
        receiver
            .await
            .map_err(|_| Error::Sync("Persistence worker dropped a response".to_string()))?
    }

    async fn retain_shardtree_checkpoint(&self, checkpoint_id: BlockHeight) -> Result<()> {
        let (response, receiver) = oneshot::channel();
        self.sender
            .as_ref()
            .ok_or_else(|| Error::Sync("Persistence worker is closed".to_string()))?
            .send(PersistenceOperation::RetainCheckpoint {
                checkpoint_id,
                response,
            })
            .map_err(|_| Error::Sync("Persistence worker stopped unexpectedly".to_string()))?;
        receiver
            .await
            .map_err(|_| Error::Sync("Persistence worker dropped a response".to_string()))?
    }
}

fn invalidate_persistence_shardtrees(
    shardtrees: &mut Option<PersistenceShardTrees<'_>>,
    reason: &'static str,
) {
    if let Some(trees) = shardtrees.take() {
        let (sapling_shards, ironwood_shards) = trees.cached_shard_counts();
        tracing::debug!(
            reason,
            sapling_shards,
            ironwood_shards,
            "invalidated persistence ShardTree cache"
        );
        if verbose_sync_batch_logging_enabled() {
            append_sync_decision_log(
                "sync.rs:persistence_worker",
                "invalidated persistence shardtree cache",
                format!(
                    "\"reason\":\"{}\",\"sapling_evicted_shards\":{},\"ironwood_evicted_shards\":{}",
                    reason, sapling_shards, ironwood_shards
                ),
            );
        }
    }
}

impl Drop for PersistenceWorker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct FetchedBlockBatch {
    blocks: Vec<CompactBlockData>,
    encoded_bytes: u64,
    shielded_work_items: u64,
    requested_blocks: u64,
    requested_bytes: u64,
    requested_work_items: u64,
    source: BlockFetchSource,
    elapsed: Duration,
    network_elapsed: Duration,
    cache_write_elapsed: Duration,
    spool_reservations: Vec<PrefetchReservation>,
}

struct PrefetchFlowControl {
    target_blocks: AtomicU64,
    target_bytes: AtomicU64,
    /// Adaptive work target for network-fed batches.
    target_work_items: AtomicU64,
    /// Larger bounded target for blocks already durable in the local cache.
    cached_target_work_items: u64,
    sapling_work_factor: u64,
    ironwood_work_factor: u64,
    durable_segment_blocks: Arc<AtomicU64>,
    watermarks: Arc<PrefetchWatermarks>,
}

struct DurableBlockSegment {
    blocks: Vec<CompactBlockData>,
    encoded_block_bytes: Vec<u64>,
    encoded_bytes: u64,
    network_elapsed: Duration,
    cache_write_elapsed: Duration,
    reservation: PrefetchReservation,
}

struct AbortTaskOnDrop(tokio::task::AbortHandle);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[derive(Default)]
struct NetworkBatchAccumulator {
    blocks: Vec<CompactBlockData>,
    encoded_bytes: u64,
    shielded_work_items: u64,
    network_elapsed: Duration,
    cache_write_elapsed: Duration,
    spool_reservations: Vec<PrefetchReservation>,
}

impl NetworkBatchAccumulator {
    fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    fn reached(&self, target: (u64, u64, u64)) -> bool {
        self.blocks.len() as u64 >= target.0
            || self.encoded_bytes >= target.1
            || self.shielded_work_items >= target.2
    }

    fn push(
        &mut self,
        mut blocks: Vec<CompactBlockData>,
        encoded_bytes: u64,
        shielded_work_items: u64,
        network_elapsed: Duration,
        cache_write_elapsed: Duration,
        reservation: PrefetchReservation,
    ) {
        self.blocks.append(&mut blocks);
        self.encoded_bytes = self.encoded_bytes.saturating_add(encoded_bytes);
        self.shielded_work_items = self.shielded_work_items.saturating_add(shielded_work_items);
        self.network_elapsed += network_elapsed;
        self.cache_write_elapsed += cache_write_elapsed;
        self.spool_reservations.push(reservation);
    }

    fn take_batch(
        &mut self,
        requested_blocks: u64,
        requested_bytes: u64,
        requested_work_items: u64,
    ) -> FetchedBlockBatch {
        let network_elapsed = std::mem::take(&mut self.network_elapsed);
        let cache_write_elapsed = std::mem::take(&mut self.cache_write_elapsed);
        FetchedBlockBatch {
            blocks: std::mem::take(&mut self.blocks),
            encoded_bytes: std::mem::take(&mut self.encoded_bytes),
            shielded_work_items: std::mem::take(&mut self.shielded_work_items),
            requested_blocks,
            requested_bytes,
            requested_work_items,
            source: BlockFetchSource::Network,
            elapsed: network_elapsed + cache_write_elapsed,
            network_elapsed,
            cache_write_elapsed,
            spool_reservations: std::mem::take(&mut self.spool_reservations),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
struct BlockFetchTimings {
    network_elapsed: Duration,
    cache_write_elapsed: Duration,
    encoded_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ValidatedCacheRange {
    start: u64,
    end: u64,
}

impl ValidatedCacheRange {
    fn contains(self, start: u64, end: u64) -> bool {
        self.start <= start && end <= self.end
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BlockFetchSource {
    Cache,
    Network,
}

impl BlockFetchSource {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Network => "network",
        }
    }
}

#[cfg(test)]
struct ServerBatchHintTask {
    start: u64,
    handle: tokio::task::JoinHandle<Option<u64>>,
}

struct HistoricalPrefillTask {
    handle: Option<tokio::task::JoinHandle<Result<RemoteHistoricalSubtreeRoots>>>,
}

impl HistoricalPrefillTask {
    fn spawn(
        client: LightClient,
        request: HistoricalSubtreeRootRequest,
        timeout: Duration,
        cancel: CancelToken,
    ) -> Self {
        let handle = tokio::spawn(async move {
            tokio::select! {
                roots = fetch_remote_historical_subtree_roots(&client, request, timeout) => {
                    Ok(roots)
                },
                _ = cancel.cancelled() => Err(Error::Cancelled),
            }
        });
        Self {
            handle: Some(handle),
        }
    }

    async fn take_ready(&mut self) -> Option<Result<RemoteHistoricalSubtreeRoots>> {
        if !self
            .handle
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
        {
            return None;
        }
        let handle = self.handle.take()?;
        Some(
            handle
                .await
                .map_err(|error| {
                    Error::Sync(format!(
                        "Historical subtree-root prefill task failed: {}",
                        error
                    ))
                })
                .and_then(|result| result),
        )
    }
}

impl Drop for HistoricalPrefillTask {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

async fn merge_ready_historical_prefill(
    task: &mut Option<HistoricalPrefillTask>,
    state: &mut Option<HistoricalPrefillState>,
) -> bool {
    let ready = match task.as_mut() {
        Some(task) => task.take_ready().await,
        None => None,
    };
    let Some(result) = ready else {
        return false;
    };
    *task = None;

    match result {
        Ok(remote) => {
            if let Some(state) = state.as_mut() {
                state.merge_remote_roots(remote);
                append_sync_decision_log(
                    "sync.rs:sync_range_internal",
                    "remote subtree roots merged at batch boundary",
                    format!(
                        "\"sapling_prefetched\":{},\"ironwood_prefetched\":{},\"sapling_available\":{},\"ironwood_available\":{}",
                        state.sapling_prefetched,
                        state.orchard_prefetched,
                        state.sapling.roots_by_index.len(),
                        state.orchard.roots_by_index.len()
                    ),
                );
            }
        }
        Err(Error::Cancelled) => {}
        Err(error) => {
            tracing::warn!(
                "Optional historical subtree-root prefill unavailable; continuing with compact blocks: {}",
                error
            );
            append_sync_decision_log(
                "sync.rs:sync_range_internal",
                "remote subtree-root prefill unavailable",
                format!("\"error\":\"{}\"", error.to_string().replace('"', "'")),
            );
        }
    }
    true
}

#[cfg(test)]
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
    LocalSnapshot,
    RemoteTreeState,
    ReplayFrom(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResumeChainPolicy {
    RepairMetadataGap,
    PreserveHistoricalBootstrap,
}

fn matching_resume_height(
    requested_start_height: u64,
    local_tip_height: u64,
    metadata_gap: bool,
    policy: ResumeChainPolicy,
) -> u64 {
    if metadata_gap && policy == ResumeChainPolicy::RepairMetadataGap {
        local_tip_height.saturating_add(1).max(1)
    } else {
        requested_start_height
    }
}

fn wallet_relevant_blocks(
    blocks: &[CompactBlockData],
    birthday_height: u32,
) -> &[CompactBlockData] {
    let birthday_height = u64::from(birthday_height);
    let first_relevant = blocks.partition_point(|block| block.height < birthday_height);
    &blocks[first_relevant..]
}

fn block_shielded_work_items(
    blocks: &[CompactBlockData],
    sapling_work_factor: u64,
    ironwood_work_factor: u64,
) -> Vec<u64> {
    blocks
        .iter()
        .map(|block| block.shielded_work_items(sapling_work_factor, ironwood_work_factor))
        .collect()
}

fn prepare_commitment_batch(blocks: &[CompactBlockData]) -> PreparedCommitmentBatch {
    let started = Instant::now();
    let prepared_blocks = blocks
        .par_iter()
        .map(|block| {
            let mut monotonic = true;
            let mut last_index = 0u64;
            for (fallback_index, tx) in block.transactions.iter().enumerate() {
                let index = tx.index.unwrap_or(fallback_index as u64);
                if fallback_index > 0 && index < last_index {
                    monotonic = false;
                    break;
                }
                last_index = index;
            }

            let mut ordered_transactions =
                block.transactions.iter().enumerate().collect::<Vec<_>>();
            if !monotonic {
                ordered_transactions
                    .sort_by_key(|(fallback_index, tx)| tx.index.unwrap_or(*fallback_index as u64));
            }

            let transactions = ordered_transactions
                .into_iter()
                .map(|(_, tx)| {
                    let sapling = tx
                        .outputs
                        .iter()
                        .enumerate()
                        .filter_map(|(output_index, output)| {
                            let commitment: [u8; 32] = output.cmu.as_slice().try_into().ok()?;
                            let node = Option::<SaplingExtractedNoteCommitment>::from(
                                SaplingExtractedNoteCommitment::from_bytes(&commitment),
                            )
                            .map(|cmu| SaplingNode::from_cmu(&cmu));
                            Some(PreparedSaplingCommitment {
                                output_index,
                                commitment,
                                node,
                            })
                        })
                        .collect();
                    let ironwood = tx
                        .actions
                        .iter()
                        .filter_map(|action| {
                            let commitment: [u8; 32] = action.cmx.as_slice().try_into().ok()?;
                            let node = Option::<OrchardExtractedNoteCommitment>::from(
                                OrchardExtractedNoteCommitment::from_bytes(&commitment),
                            )
                            .map(|cmx| MerkleHashOrchard::from_cmx(&cmx));
                            Some(PreparedIronwoodCommitment { commitment, node })
                        })
                        .collect();
                    PreparedCommitmentTransaction {
                        hash: tx.hash.clone(),
                        sapling,
                        ironwood,
                    }
                })
                .collect();

            PreparedBlockCommitments {
                height: block.height,
                hash: block.hash.clone(),
                transactions,
            }
        })
        .collect::<Vec<_>>();
    let (sapling_count, ironwood_count) = prepared_blocks
        .iter()
        .flat_map(|block| &block.transactions)
        .fold((0usize, 0usize), |(sapling, ironwood), tx| {
            (
                sapling.saturating_add(tx.sapling.len()),
                ironwood.saturating_add(tx.ironwood.len()),
            )
        });

    PreparedCommitmentBatch {
        blocks: prepared_blocks,
        elapsed: started.elapsed(),
        sapling_count,
        ironwood_count,
    }
}

#[cfg(test)]
fn batch_cap_for_target_latency(
    current_cap: u64,
    fetched_blocks: u64,
    elapsed: Duration,
    min_batch_size: u64,
    max_batch_size: u64,
) -> u64 {
    let max_batch_size = max_batch_size.max(min_batch_size).max(1);
    let min_batch_size = min_batch_size.max(1).min(max_batch_size);
    let current_cap = current_cap.clamp(min_batch_size, max_batch_size);
    if fetched_blocks == 0 {
        return current_cap;
    }

    let projected = (u128::from(fetched_blocks) * TARGET_FETCH_BATCH_MS)
        .checked_div(elapsed.as_millis().max(1))
        .unwrap_or(u128::from(max_batch_size))
        .min(u128::from(max_batch_size)) as u64;
    let growth_cap = current_cap
        .saturating_mul(MAX_BATCH_CAP_GROWTH_FACTOR)
        .min(max_batch_size);
    let adjusted = if projected > current_cap {
        projected.min(growth_cap)
    } else {
        projected
    };

    adjusted.clamp(min_batch_size, max_batch_size)
}

#[cfg(test)]
fn network_batch_cap_after_fetch(
    current_cap: u64,
    source: BlockFetchSource,
    fetched_blocks: u64,
    elapsed: Duration,
    min_batch_size: u64,
    max_batch_size: u64,
) -> u64 {
    if source == BlockFetchSource::Cache {
        current_cap
    } else {
        batch_cap_for_target_latency(
            current_cap,
            fetched_blocks,
            elapsed,
            min_batch_size,
            max_batch_size,
        )
    }
}

#[cfg(test)]
fn initial_network_batch_cap(config: &SyncConfig) -> u64 {
    let min_batch_size = config.min_batch_size.max(1);
    let max_batch_size = config.max_batch_size.max(min_batch_size);
    let memory_limit = config.max_batch_memory_bytes.unwrap_or(u64::MAX);
    let byte_limit = config
        .target_batch_bytes
        .min(config.max_batch_bytes)
        .min(memory_limit);
    let heavy_block_size = config.heavy_block_threshold_bytes.max(1);

    (byte_limit / heavy_block_size).clamp(min_batch_size, max_batch_size)
}

#[cfg(test)]
fn cached_batch_block_cap() -> u64 {
    // This is only a logical planning horizon. The bounded cache decoder and
    // byte semaphore split it using actual encoded row sizes, so a coarse
    // device profile must not impose a second, stale block-count cliff.
    MAX_CACHED_BATCH_BLOCKS
}

fn prefetched_batch_encoded_byte_cap(config: &SyncConfig) -> u64 {
    config
        .max_batch_bytes
        .max(1)
        .min(config.max_batch_memory_bytes.unwrap_or(u64::MAX).max(1))
        .min(config.prefetch_queue_max_bytes.max(1))
}

fn resumable_tree_replay_checkpoint(
    sync_height: u64,
    requested_tree_height: u64,
    sapling_activation: u64,
    common_checkpoint: Option<u64>,
) -> Option<u64> {
    let replay_baseline = sapling_activation.saturating_sub(1);
    if sync_height < replay_baseline || sync_height >= requested_tree_height {
        return None;
    }

    common_checkpoint
        .filter(|checkpoint| *checkpoint >= replay_baseline && *checkpoint <= sync_height)
}

fn tree_replay_prefetch_end(replay_target: Option<u64>, next_start: u64, sync_end: u64) -> u64 {
    replay_target
        .filter(|target| next_start <= *target)
        .map_or(sync_end, |target| sync_end.min(target))
}

fn tree_replay_checkpoint_due(
    replay_target: Option<u64>,
    batch_end: u64,
    sync_state_flush_due: bool,
    checkpoint_written: bool,
) -> bool {
    replay_target.is_some()
        && !checkpoint_written
        && (sync_state_flush_due || replay_target.is_some_and(|target| batch_end >= target))
}

fn select_sync_target(
    start_height: u64,
    requested_end: Option<u64>,
    server_height: u64,
    follow_tip: bool,
) -> u64 {
    requested_end.unwrap_or_else(|| {
        if follow_tip {
            server_height
        } else {
            server_height.max(start_height)
        }
    })
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

    fn resolved_ironwood_activation_height(&self) -> Option<u32> {
        u32::try_from(self.ironwood_activation_height.load(Ordering::Acquire))
            .ok()
            .filter(|height| *height > 0)
    }

    fn store_ironwood_activation_height(&self, height: Option<u32>) -> Result<()> {
        if height == self.resolved_ironwood_activation_height() {
            return Ok(());
        }

        if let Some(sink) = self.storage.as_ref() {
            let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            SyncStateStorage::new(&db).set_ironwood_activation_height(height)?;
        }
        self.ironwood_activation_height
            .store(u64::from(height.unwrap_or(0)), Ordering::Release);
        Ok(())
    }

    async fn validate_server_consensus_branch(&self, info: &LightdInfo) -> Result<()> {
        let known_activation_height = self.resolved_ironwood_activation_height();
        let activation_height = crate::activation::resolve_ironwood_activation_height(
            &self.client,
            self.network_type,
            info.block_height,
            &info.consensus_branch_id,
            known_activation_height,
        )
        .await?;
        if activation_height != known_activation_height {
            self.store_ironwood_activation_height(activation_height)?;
            tracing::info!(
                "Resolved Ironwood activation height {:?} for {:?}",
                activation_height,
                self.network_type
            );
        }

        let check = crate::consensus::check_consensus_branch_with_activation_height(
            self.network_type,
            info.block_height,
            &info.consensus_branch_id,
            activation_height,
        )?;
        check.require_match()?;
        tracing::debug!(
            "Validated consensus branch {} at server height {}",
            check.sdk_branch_id,
            check.height
        );
        Ok(())
    }

    async fn validated_server_info(&self) -> Result<LightdInfo> {
        let transport = self.client.transport_mode();
        let policy = server_info_validation_policy(transport);
        let mut attempt = 0usize;

        let info = loop {
            attempt += 1;
            append_sync_decision_log(
                "sync.rs:validated_server_info",
                "server info validation attempt",
                format!(
                    "\"transport\":\"{:?}\",\"attempt\":{},\"max_attempts\":{},\"timeout_ms\":{}",
                    transport,
                    attempt,
                    policy.max_attempts,
                    policy.attempt_timeout.as_millis()
                ),
            );

            let result =
                tokio::time::timeout(policy.attempt_timeout, self.client.get_lightd_info()).await;

            match result {
                Ok(Ok(info)) => break info,
                Ok(Err(error)) if attempt >= policy.max_attempts => return Err(error),
                Err(_) if attempt >= policy.max_attempts => {
                    return Err(Error::Network(format!(
                        "Timed out after {} attempts of {:?} while validating server consensus over {:?}",
                        policy.max_attempts, policy.attempt_timeout, transport
                    )));
                }
                Ok(Err(error)) => {
                    tracing::warn!(
                        "Server-info validation failed over {:?} (attempt {}/{}): {}; reconnecting",
                        transport,
                        attempt,
                        policy.max_attempts,
                        error
                    );
                }
                Err(_) => {
                    tracing::warn!(
                        "Server-info validation timed out over {:?} after {:?} (attempt {}/{}); reconnecting",
                        transport,
                        policy.attempt_timeout,
                        attempt,
                        policy.max_attempts
                    );
                }
            }

            self.client.disconnect().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            tokio::time::timeout(policy.attempt_timeout, self.client.connect())
                .await
                .map_err(|_| {
                    Error::Network(format!(
                        "Timed out after {:?} while reconnecting {:?} for server validation",
                        policy.attempt_timeout, transport
                    ))
                })??;
        };
        self.validate_server_consensus_branch(&info).await?;
        Ok(info)
    }

    async fn require_server_consensus_branch(&self) -> Result<()> {
        self.validated_server_info().await.map(|_| ())
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
            ironwood_activation_height: Arc::new(AtomicU64::new(0)),
            wallet_id: None,
            storage: None,
            keys: Vec::new(),
            trial_decrypt_keys: TrialDecryptKeys::default(),
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
            historical_sapling_mark_subtrees: HashSet::new(),
            historical_ironwood_mark_subtrees: HashSet::new(),
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
            ironwood_activation_height: Arc::new(AtomicU64::new(0)),
            wallet_id: None,
            storage: None,
            keys: Vec::new(),
            trial_decrypt_keys: TrialDecryptKeys::default(),
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
            historical_sapling_mark_subtrees: HashSet::new(),
            historical_ironwood_mark_subtrees: HashSet::new(),
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
            ironwood_activation_height: Arc::new(AtomicU64::new(0)),
            wallet_id: None,
            storage: None,
            keys: Vec::new(),
            trial_decrypt_keys: TrialDecryptKeys::default(),
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
            historical_sapling_mark_subtrees: HashSet::new(),
            historical_ironwood_mark_subtrees: HashSet::new(),
        }
    }

    /// Preserve conservative note-position hints across explicit rescan
    /// truncation. These hints never establish ownership or chain validity;
    /// they only disable subtree-root grafting where a mark was seen before.
    pub fn with_historical_mark_positions(
        mut self,
        sapling_positions: impl IntoIterator<Item = u64>,
        ironwood_positions: impl IntoIterator<Item = u64>,
    ) -> Self {
        self.historical_sapling_mark_subtrees.extend(
            sapling_positions
                .into_iter()
                .map(|position| position >> SAPLING_SHARD_HEIGHT),
        );
        self.historical_ironwood_mark_subtrees.extend(
            ironwood_positions
                .into_iter()
                .map(|position| position >> ORCHARD_SHARD_HEIGHT),
        );
        self
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
        self,
        wallet_id: String,
        key: EncryptionKey,
        master_key: MasterKey,
        network_type: NetworkType,
        address_network_type: NetworkType,
    ) -> Result<Self> {
        let db_path = wallet_db_path(&wallet_id)?;
        self.with_wallet_at_path(
            wallet_id,
            db_path,
            key,
            master_key,
            network_type,
            address_network_type,
        )
    }

    /// Attach wallet context using a database path resolved by the host.
    ///
    /// Embedders that support runtime storage namespaces must use this method
    /// so the sync engine opens the same encrypted database as the host API.
    pub fn with_wallet_at_path(
        mut self,
        wallet_id: String,
        db_path: PathBuf,
        key: EncryptionKey,
        master_key: MasterKey,
        network_type: NetworkType,
        address_network_type: NetworkType,
    ) -> Result<Self> {
        self.wallet_id = Some(wallet_id.clone());
        self.network_type = network_type;

        let db = Database::open(&db_path, &key, master_key.clone())?;
        let repo = Repository::new(&db);
        let sync_state = SyncStateStorage::new(&db).load_sync_state()?;
        let activation_height = PirateParamsNetwork::from_type(network_type)
            .ironwood_activation_height
            .or(sync_state.ironwood_activation_height);
        self.ironwood_activation_height
            .store(u64::from(activation_height.unwrap_or(0)), Ordering::Release);

        // Load wallet secret to know account id (if present)
        let secret = repo
            .get_wallet_secret(&wallet_id)?
            .ok_or_else(|| Error::Sync(format!("Wallet secret not found for {}", wallet_id)))?;

        if !secret.extsk.is_empty() {
            let (_, replay_required) =
                repo.reconcile_primary_seed_account_key(&secret, i64::from(self.birthday_height))?;
            if replay_required {
                let replay_height = truncate_above_height(&db, 0)?;
                SyncStateStorage::new(&db).reset_sync_state(replay_height)?;
                ScanQueueStorage::new(&db).clear_all()?;
                repo.clear_seed_key_scan_replay_required()?;
                tracing::warn!(
                    "Reconciled wallet {} seed key material and reset derived scan state",
                    wallet_id
                );
            }
        }

        let mut account_keys = repo.get_account_keys(secret.account_id)?;
        if account_keys.is_empty() {
            let sapling_dfvk_bytes = if !secret.extsk.is_empty() {
                let extsk = ExtendedSpendingKey::from_bytes(&secret.extsk)
                    .map_err(|e| Error::Sync(format!("Invalid spending key bytes: {}", e)))?;
                Some(extsk.to_extended_fvk().to_bytes())
            } else {
                secret.dfvk.clone()
            };

            let orchard_fvk_bytes = if let Some(ref extsk_bytes) = secret.orchard_extsk {
                let extsk = IronwoodExtendedSpendingKey::from_bytes(extsk_bytes).map_err(|e| {
                    Error::Sync(format!("Invalid Ironwood spending key bytes: {}", e))
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

        let seed_derived_keys = repo
            .get_seed_derived_account_keys(secret.account_id)?
            .into_iter()
            .map(|metadata| {
                (
                    metadata.key_id,
                    (metadata.derivation_index, metadata.is_discovery_candidate),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut key_groups = Vec::new();
        for key in &account_keys {
            let seed_derivation = key
                .id
                .and_then(|key_id| seed_derived_keys.get(&key_id).copied());
            if let Some(group) = build_key_group_from_account_key(key, seed_derivation)? {
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
        let trial_decrypt_keys = TrialDecryptKeys::from_key_groups(&key_groups);
        let key_inventory =
            SyncKeyInventory::from_sources(&account_keys, &key_groups, &trial_decrypt_keys);
        key_inventory.append_debug_event(&wallet_id);

        self.storage = Some(sink);
        self.trial_decrypt_keys = trial_decrypt_keys;
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
        let info = self.validated_server_info().await?;
        {
            let progress = self.progress.write().await;
            progress.set_current(start_height.saturating_sub(1));
            progress.set_target(info.block_height);
            progress.set_stage(SyncStage::Preparing);
        }
        Ok((start_height, info.block_height))
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
        policy: ResumeChainPolicy,
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

        let remote_tip_block = tokio::time::timeout(
            resume_chain_network_timeout(self.client.transport_mode()),
            self.client.get_block(height_to_u32(local_tip.height)?),
        )
        .await
        .map_err(|_| {
            Error::Network(format!(
                "Timed out validating the local resume block at height {} over {:?}",
                local_tip.height,
                self.client.transport_mode()
            ))
        })??;
        if remote_tip_block.hash == local_tip.hash {
            let resume_height = matching_resume_height(
                requested_start_height,
                local_tip.height,
                metadata_gap,
                policy,
            );
            if resume_height != requested_start_height {
                self.rollback_to_checkpoint(local_tip.height, None).await?;
                self.invalidate_block_cache_above(local_tip.height);
            } else if metadata_gap {
                tracing::info!(
                    "Preserving intentional historical bootstrap at {} after validating retained chain tip {}",
                    requested_start_height,
                    local_tip.height
                );
                append_debug_log_line(&format!(
                    r#"{{"id":"log_rescan_bootstrap_gap","timestamp":{},"location":"sync.rs:validate_resume_chain","message":"preserved intentional historical bootstrap gap","data":{{"requested_start":{},"retained_tip":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"R"}}"#,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis(),
                    requested_start_height,
                    local_tip.height
                ));
            }
            return Ok(resume_height);
        }

        tracing::warn!(
            "Reorg detected on resume at height {} (local={}, remote={})",
            local_tip.height,
            hex::encode(&local_tip.hash),
            hex::encode(&remote_tip_block.hash)
        );
        self.rollback_to_common_ancestor(local_tip.height, None)
            .await
    }

    async fn rollback_to_common_ancestor(
        &mut self,
        divergent_height: u64,
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<u64> {
        let rollback_height = self
            .find_common_ancestor(divergent_height)
            .await?
            .unwrap_or_else(|| (self.birthday_height as u64).saturating_sub(1));

        self.rollback_to_checkpoint(rollback_height, persistence_worker)
            .await?;
        self.invalidate_block_cache_above(rollback_height);

        Ok(rollback_height.saturating_add(1).max(1))
    }

    async fn find_common_ancestor(&self, divergent_height: u64) -> Result<Option<u64>> {
        let timeout = resume_chain_network_timeout(self.client.transport_mode());
        tokio::time::timeout(timeout, self.find_common_ancestor_bounded(divergent_height))
            .await
            .map_err(|_| {
                Error::Network(format!(
                    "Timed out after {:?} while locating a common chain ancestor over {:?}",
                    timeout,
                    self.client.transport_mode()
                ))
            })?
    }

    async fn find_common_ancestor_bounded(&self, divergent_height: u64) -> Result<Option<u64>> {
        let Some(sink) = self.storage.clone() else {
            return Ok(None);
        };
        let birthday_floor = (self.birthday_height as u64).saturating_sub(1);
        let stop_height = divergent_height
            .saturating_sub(MAX_REORG_SEARCH_DEPTH)
            .max(birthday_floor);

        let local_blocks = sink.load_chain_blocks(stop_height, divergent_height)?;
        let local_by_height = local_blocks
            .iter()
            .map(|block| (block.height, block))
            .collect::<HashMap<_, _>>();
        let expected_rows = divergent_height
            .saturating_sub(stop_height)
            .saturating_add(1);

        // Current wallets persist a contiguous reorg window. Probe that window
        // exponentially, then binary-search the final bracket. This reduces a
        // worst-case 2,001-RPC linear walk to at most a few dozen small RPCs.
        if local_blocks.len() as u64 == expected_rows {
            let mut upper_mismatch = divergent_height;
            let mut distance = 1u64;
            let lower_match = loop {
                let probe = reorg_backoff_probe(divergent_height, stop_height, distance);
                let local = local_by_height.get(&probe).ok_or_else(|| {
                    Error::Sync(format!(
                        "Canonical reorg window is missing probe height {}",
                        probe
                    ))
                })?;
                let remote = self.client.get_block(height_to_u32(probe)?).await?;
                if local.hash == remote.hash {
                    break Some(probe);
                }
                upper_mismatch = probe;
                if probe == stop_height {
                    break None;
                }
                distance = distance.saturating_mul(2);
            };

            if let Some(mut lower_match) = lower_match {
                let mut upper_search = upper_mismatch.saturating_sub(1);
                while lower_match < upper_search {
                    let probe = lower_match.saturating_add(upper_search).saturating_add(1) / 2;
                    let local = local_by_height.get(&probe).ok_or_else(|| {
                        Error::Sync(format!(
                            "Canonical reorg window is missing probe height {}",
                            probe
                        ))
                    })?;
                    let remote = self.client.get_block(height_to_u32(probe)?).await?;
                    if local.hash == remote.hash {
                        lower_match = probe;
                    } else {
                        upper_search = probe.saturating_sub(1);
                    }
                }
                tracing::info!(
                    "Found common chain ancestor at height {} using bounded probes",
                    lower_match
                );
                return Ok(Some(lower_match));
            }
        } else {
            // Legacy metadata can contain gaps. Preserve the conservative
            // descending behavior, but keep it under the outer transport-aware
            // deadline so a damaged window cannot hold Preparing indefinitely.
            for local in local_blocks.iter().rev() {
                let remote = self.client.get_block(height_to_u32(local.height)?).await?;
                if local.hash == remote.hash {
                    tracing::info!(
                        "Found common chain ancestor at height {} in sparse metadata",
                        local.height
                    );
                    return Ok(Some(local.height));
                }
            }
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
        db_session: Option<&Database>,
        previous_processed_hash: Option<&[u8]>,
    ) -> Result<bool> {
        if batch_start <= 1 || blocks.is_empty() {
            return Ok(true);
        }
        let first = &blocks[0];
        if first.prev_hash.len() != 32 {
            return Err(Error::Sync(format!(
                "Block {} has invalid prev_hash length {}",
                first.height,
                first.prev_hash.len()
            )));
        }
        if let Some(expected_hash) = previous_processed_hash {
            return Ok(first.prev_hash == expected_hash);
        }
        let Some(sink) = self.storage.as_ref() else {
            return Ok(true);
        };
        let previous_height = batch_start.saturating_sub(1);
        let previous = match db_session {
            Some(db) => sink.load_chain_block_with_db(db, previous_height)?,
            None => sink.load_chain_block(previous_height)?,
        };
        let Some(previous) = previous else {
            return Ok(true);
        };
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
        let follow_tip = end_height.is_none();
        self.sync_range_with_mode(
            start_height,
            end_height,
            follow_tip,
            ResumeChainPolicy::RepairMetadataGap,
        )
        .await
    }

    /// Sync through one validated snapshot of the server tip, then return.
    pub async fn sync_range_to_latest(&mut self, start_height: u64) -> Result<()> {
        self.sync_range_with_mode(
            start_height,
            None,
            false,
            ResumeChainPolicy::RepairMetadataGap,
        )
        .await
    }

    /// Sync an explicit historical rescan through one validated tip snapshot.
    /// A gap between retained chain metadata and `start_height` is intentional:
    /// ShardTrees will be bootstrapped from the server at the requested height.
    pub async fn sync_rescan_to_latest(&mut self, start_height: u64) -> Result<()> {
        self.sync_range_with_mode(
            start_height,
            None,
            false,
            ResumeChainPolicy::PreserveHistoricalBootstrap,
        )
        .await
    }

    async fn sync_range_with_mode(
        &mut self,
        start_height: u64,
        end_height: Option<u64>,
        follow_tip: bool,
        resume_chain_policy: ResumeChainPolicy,
    ) -> Result<()> {
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

        // One server-info snapshot supplies both the target and the consensus
        // branch, avoiding two independently timed control-plane RPCs.
        let info = self.validated_server_info().await?;
        let end = select_sync_target(start_height, end_height, info.block_height, follow_tip);
        tracing::debug!(
            "Using sync target {} from validated server height {}",
            end,
            info.block_height
        );

        // Publish progress from the same validated server snapshot used to
        // select this run's target height.
        {
            let progress = self.progress.write().await;
            progress.set_target(end);
            progress.set_current(start_height.saturating_sub(1));
            progress.set_stage(SyncStage::Preparing);
            progress.start();
        }
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
                // loop is a no-op (start > end, so there is nothing to fetch). The follow-tip
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
            .validate_resume_chain(effective_start_height, end, resume_chain_policy)
            .await?;

        self.ensure_nullifier_cache()?;

        // Initialize progress
        {
            let progress = self.progress.write().await;
            progress.set_target(end);
            progress.set_current(effective_start_height.saturating_sub(1));
            progress.set_stage(SyncStage::TreeState);
            progress.start();
            tracing::debug!(
                "Progress initialized: current={}, target={}, stage={:?}",
                effective_start_height,
                end,
                SyncStage::TreeState
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
            let durable_state = self
                .storage
                .as_ref()
                .and_then(|sink| sink.load_sync_state().ok());
            let progress = self.progress.write().await;
            if let Some(state) = durable_state {
                progress.set_current(state.local_height);
                progress.set_checkpoint(state.last_checkpoint_height);
            }
            progress.set_stage(SyncStage::Verify);
            tracing::error!("Sync failed: {:?}", result);
        }

        result
    }

    /// Check whether both ShardTrees have the exact checkpoint required by the
    /// persisted sync cursor.
    fn shardtrees_have_checkpoint(&self, height: u64) -> Result<bool> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(false);
        };
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let height_u32 = u32::try_from(height).unwrap_or(u32::MAX);
        let has: bool = db
            .conn()
            .query_row(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM sapling_tree_checkpoints s
                    INNER JOIN orchard_tree_checkpoints o
                        ON o.checkpoint_id = s.checkpoint_id
                    WHERE s.checkpoint_id = ?1
                )
                "#,
                [height_u32],
                |row| row.get(0),
            )
            .unwrap_or(false);
        Ok(has)
    }

    fn partial_tree_replay_checkpoint(
        &self,
        requested_tree_height: u64,
        sapling_activation: u64,
    ) -> Result<Option<u64>> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(None);
        };
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let sync_state = SyncStateStorage::new(&db).load_sync_state()?;
        let replay_baseline = sapling_activation.saturating_sub(1);
        if sync_state.local_height < replay_baseline
            || sync_state.local_height >= requested_tree_height
        {
            return Ok(None);
        }

        let checkpoint: Option<u32> = db
            .conn()
            .query_row(
                r#"
                SELECT MAX(s.checkpoint_id)
                FROM sapling_tree_checkpoints s
                INNER JOIN orchard_tree_checkpoints o
                    ON o.checkpoint_id = s.checkpoint_id
                WHERE s.checkpoint_id <= ?1
                "#,
                [u32::try_from(sync_state.local_height).unwrap_or(u32::MAX)],
                |row| row.get(0),
            )
            .map_err(|e| Error::Sync(format!("Failed to query replay checkpoint: {}", e)))?;

        Ok(resumable_tree_replay_checkpoint(
            sync_state.local_height,
            requested_tree_height,
            sapling_activation,
            checkpoint.map(u64::from),
        ))
    }

    async fn rewind_shardtrees_for_replay(&self, checkpoint_height: u64) -> Result<()> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(());
        };
        let checkpoint_height = u32::try_from(checkpoint_height).map_err(|_| {
            Error::Sync(format!(
                "Replay checkpoint {} exceeds u32::MAX",
                checkpoint_height
            ))
        })?;
        let checkpoint_id = BlockHeight::from(checkpoint_height);
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let tx = db.unchecked_immediate_transaction().map_err(|e| {
            Error::Sync(format!("Failed to start replay-resume transaction: {}", e))
        })?;

        {
            let store = SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                &tx,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open Sapling shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
            tree.truncate_to_checkpoint(&checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to resume Sapling replay at {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }
        {
            let store =
                SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                    &tx,
                    ORCHARD_TABLE_PREFIX,
                )
                .map_err(|e| Error::Sync(format!("Failed to open Orchard shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
            tree.truncate_to_checkpoint(&checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to resume Orchard replay at {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }
        tx.execute(
            r#"
            UPDATE sync_state
            SET local_height = ?1,
                last_checkpoint_height = ?1,
                updated_at = ?2
            WHERE id = 1
            "#,
            rusqlite::params![checkpoint_height, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Sync(format!("Failed to align replay sync state: {}", e)))?;
        tx.commit().map_err(|e| {
            Error::Sync(format!("Failed to commit replay-resume transaction: {}", e))
        })?;
        self.recover_position_counters_from_shardtree().await?;
        Ok(())
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
        let sapling_activation =
            u64::from(PirateParamsNetwork::from_type(self.network_type).sapling_activation_height)
                .max(1);
        if start_height <= sapling_activation {
            return self.prepare_shardtrees_for_replay(sapling_activation).await;
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

        if self.shardtrees_have_checkpoint(tree_height)? {
            self.retain_checkpoint(tree_height, None, None, None)
                .await?;
            self.recover_position_counters_from_shardtree().await?;
            tracing::debug!(
                "ShardTree already has checkpoints at height {}; reusing existing state",
                tree_height
            );
            return Ok(FrontierInitSource::LocalSnapshot);
        }

        // A Zcash-style wallet birthday consists of both a height and the exact
        // note-commitment frontier at the end of the preceding block. Always try
        // that bounded remote seed before resuming an activation-era replay. This
        // lets wallets created by older builds escape an accidentally-started
        // replay as soon as a conforming light server is available.
        let partial_replay =
            self.partial_tree_replay_checkpoint(tree_height, sapling_activation)?;
        let remote_seed = match self.fetch_tree_state_with_retry(tree_height).await {
            Ok(tree_state) => {
                self.seed_shardtrees_from_tree_state(tree_height, tree_state)
                    .await
            }
            Err(e) => Err(e),
        };

        match remote_seed {
            Ok(()) => {
                self.retain_checkpoint(tree_height, None, None, None)
                    .await?;
                Ok(FrontierInitSource::RemoteTreeState)
            }
            Err(remote_error) => {
                if let Some(checkpoint_height) = partial_replay {
                    self.rewind_shardtrees_for_replay(checkpoint_height).await?;
                    tracing::warn!(
                        "Historical tree state at {} is unavailable ({}); resuming the previously-started commitment-tree replay at {}",
                        tree_height,
                        remote_error,
                        checkpoint_height
                    );
                    return Ok(FrontierInitSource::ReplayFrom(
                        checkpoint_height.saturating_add(1),
                    ));
                }

                Err(Error::Sync(format!(
                    "Cannot initialize wallet birthday {} because the selected light server did not provide a valid tree state at height {}: {}. Retry or select another light server; refusing to replay automatically from Sapling activation {}",
                    start_height, tree_height, remote_error, sapling_activation
                )))
            }
        }
    }

    async fn prepare_shardtrees_for_replay(
        &self,
        replay_height: u64,
    ) -> Result<FrontierInitSource> {
        self.prepare_empty_shardtrees_for_replay(replay_height.saturating_sub(1))
            .await?;
        Ok(FrontierInitSource::ReplayFrom(replay_height))
    }

    async fn prepare_empty_shardtrees_for_replay(&self, checkpoint_height: u64) -> Result<()> {
        let Some(sink) = self.storage.as_ref() else {
            *self.sapling_tree_position.write().await = 0;
            *self.orchard_tree_position.write().await = 0;
            return Ok(());
        };
        let checkpoint_height = u32::try_from(checkpoint_height).map_err(|_| {
            Error::Sync(format!(
                "Replay baseline checkpoint {} exceeds u32::MAX",
                checkpoint_height
            ))
        })?;
        let checkpoint_id = BlockHeight::from(checkpoint_height);
        let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
        let tx = db
            .conn()
            .unchecked_transaction()
            .map_err(|e| Error::Sync(format!("Failed to start shardtree replay reset: {}", e)))?;
        for prefix in [SAPLING_TABLE_PREFIX, ORCHARD_TABLE_PREFIX] {
            for suffix in [
                "tree_checkpoint_marks_removed",
                "tree_checkpoints",
                "tree_shards",
                "tree_cap",
                "tree_retained_checkpoints",
            ] {
                tx.execute(&format!("DELETE FROM {}_{}", prefix, suffix), [])
                    .map_err(|e| {
                        Error::Sync(format!(
                            "Failed to reset {} shardtree for deterministic replay: {}",
                            prefix, e
                        ))
                    })?;
            }
        }
        {
            let store = SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                &tx,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open Sapling shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
            tree.checkpoint(checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to create Sapling replay baseline at {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }
        {
            let store =
                SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                    &tx,
                    ORCHARD_TABLE_PREFIX,
                )
                .map_err(|e| Error::Sync(format!("Failed to open Orchard shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
            tree.checkpoint(checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to create Orchard replay baseline at {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }
        tx.execute(
            r#"
            UPDATE sync_state
            SET local_height = ?1,
                last_checkpoint_height = ?1,
                updated_at = ?2
            WHERE id = 1
            "#,
            rusqlite::params![checkpoint_height, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Sync(format!("Failed to align replay baseline state: {}", e)))?;
        tx.commit()
            .map_err(|e| Error::Sync(format!("Failed to commit shardtree replay reset: {}", e)))?;
        *self.sapling_tree_position.write().await = 0;
        *self.orchard_tree_position.write().await = 0;
        Ok(())
    }

    /// Seed both ShardTrees from a lightwalletd tree state at the given height.
    async fn seed_shardtrees_from_tree_state(
        &self,
        tree_height: u64,
        tree_state: TreeState,
    ) -> Result<()> {
        if tree_state.height != tree_height {
            return Err(Error::Sync(format!(
                "Server returned tree state for height {}, expected {}",
                tree_state.height, tree_height
            )));
        }

        let sapling_required = tree_height
            >= u64::from(
                PirateParamsNetwork::from_type(self.network_type).sapling_activation_height,
            );
        let sapling_frontier = if !tree_state.sapling_frontier.is_empty() {
            match Self::parse_frontier_hex::<SaplingNode>(
                "sapling_frontier",
                &tree_state.sapling_frontier,
            ) {
                Ok(f) => Some(f),
                Err(e) => {
                    return Err(Error::Sync(format!(
                        "Invalid Sapling frontier at height {}: {}",
                        tree_height, e
                    )))
                }
            }
        } else if !tree_state.sapling_tree.is_empty() {
            match Self::parse_frontier_hex::<SaplingNode>("sapling_tree", &tree_state.sapling_tree)
            {
                Ok(f) => Some(f),
                Err(e) => {
                    return Err(Error::Sync(format!(
                        "Invalid Sapling tree state at height {}: {}",
                        tree_height, e
                    )))
                }
            }
        } else if sapling_required {
            return Err(Error::Sync(format!(
                "Server returned no Sapling tree data at active height {}",
                tree_height
            )));
        } else {
            tracing::info!(
                "No Sapling tree data from server at height {} -- empty tree",
                tree_height
            );
            None
        };

        let orchard_required = self.orchard_tree_required(tree_height);
        let orchard_hex_len = tree_state.ironwood_tree.len();
        let orchard_frontier = if !orchard_required {
            if !tree_state.ironwood_tree.is_empty() {
                tracing::debug!(
                    "Ignoring pre-activation Ironwood tree data at height {}",
                    tree_height
                );
            }
            None
        } else if tree_state.ironwood_tree.is_empty() {
            return Err(Error::Sync(format!(
                "Server returned no Orchard tree data at active height {}",
                tree_height
            )));
        } else {
            match Self::parse_frontier_hex::<MerkleHashOrchard>(
                "ironwood_tree",
                &tree_state.ironwood_tree,
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
                    return Err(Error::Sync(format!(
                        "Invalid Orchard tree state at height {}: {}",
                        tree_height, e
                    )));
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
        let tx = db.unchecked_immediate_transaction().map_err(|e| {
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
            tx.execute("DELETE FROM sapling_tree_retained_checkpoints", [])
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
                        marking: Marking::None,
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
            tx.execute("DELETE FROM orchard_tree_retained_checkpoints", [])
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
                            marking: Marking::None,
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
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<Option<(u64, u64)>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        let already_checked = {
            let last = self.last_witness_check_height.read().await;
            *last >= current_height
        };

        let outcome = if let Some(worker) = persistence_worker {
            worker
                .execute(move |db| {
                    Self::check_witnesses_with_db(&sink, current_height, already_checked, db)
                })
                .await?
        } else {
            let owned_db;
            let db = if let Some(db) = db_session {
                db
            } else {
                owned_db =
                    Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
                &owned_db
            };
            Self::check_witnesses_with_db(&sink, current_height, already_checked, db)?
        };
        if outcome.checked {
            let mut last = self.last_witness_check_height.write().await;
            *last = (*last).max(current_height);
        }
        Ok(outcome.repair_range)
    }

    fn check_witnesses_with_db(
        sink: &StorageSink,
        current_height: u64,
        already_checked: bool,
        db: &Database,
    ) -> Result<WitnessCheckDbOutcome> {
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
            return Ok(WitnessCheckDbOutcome {
                repair_range: None,
                checked: false,
            });
        };

        let state = spendability.load_state().unwrap_or_default();
        if state.rescan_required {
            return Ok(WitnessCheckDbOutcome {
                repair_range: None,
                checked: false,
            });
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
                return Ok(WitnessCheckDbOutcome {
                    repair_range: None,
                    checked: false,
                });
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
            return Ok(WitnessCheckDbOutcome {
                repair_range: None,
                checked: true,
            });
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
        Ok(WitnessCheckDbOutcome {
            repair_range: Some((queued_start, queued_end)),
            checked: true,
        })
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
        persistence_worker: Option<&PersistenceWorker>,
    ) -> TipWitnessValidationOutcome {
        let mut queued_start = 0u64;
        let mut queued_end_exclusive = 0u64;
        let outcome: &'static str;
        let mut error_detail = String::new();
        let result;
        match self
            .check_witnesses_and_queue_rescans(tip_height, None, persistence_worker)
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

    async fn activate_queued_found_note_range(
        &self,
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<Option<(u64, u64)>> {
        let sink = match self.storage.as_ref() {
            Some(s) => s.clone(),
            None => return Ok(None),
        };
        let range = if let Some(worker) = persistence_worker {
            worker
                .execute(Self::activate_queued_found_note_range_with_db)
                .await?
        } else {
            let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            Self::activate_queued_found_note_range_with_db(&db)?
        };
        if let Some((range_start, _)) = range {
            // Force one post-repair integrity pass at the same tip after replay
            // finishes, so spendability can return to validated without waiting for
            // a new block.
            let mut last = self.last_witness_check_height.write().await;
            let force_height = range_start.saturating_sub(1);
            if *last > force_height {
                *last = force_height;
            }
        }
        Ok(range)
    }

    fn activate_queued_found_note_range_with_db(db: &Database) -> Result<Option<(u64, u64)>> {
        let scan_queue = ScanQueueStorage::new(db);
        let spendability = SpendabilityStateStorage::new(db);
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
            // occasional circuit/bootstrap jitter. Historical tree-state requests
            // are infrequent but can be expensive on the backing full node, so do
            // not create a queue of short-lived duplicate requests.
            TransportMode::Tor => TreeStateRetryProfile {
                max_attempts: 1,
                base_timeout: Duration::from_secs(150),
                timeout_step: Duration::ZERO,
                max_timeout: Duration::from_secs(150),
                initial_backoff: Duration::from_millis(500),
                max_backoff: Duration::from_secs(2),
                bridge_timeout_cap: Duration::from_secs(150),
                hash_timeout_cap: Duration::from_secs(30),
                enable_hash_fallback: false,
                extended_timeout: Duration::from_secs(170),
                extended_hash_timeout: Duration::from_secs(60),
            },
            TransportMode::I2p => TreeStateRetryProfile {
                max_attempts: 1,
                base_timeout: Duration::from_secs(150),
                timeout_step: Duration::ZERO,
                max_timeout: Duration::from_secs(150),
                initial_backoff: Duration::from_millis(500),
                max_backoff: Duration::from_secs(2),
                bridge_timeout_cap: Duration::from_secs(150),
                hash_timeout_cap: Duration::from_secs(30),
                enable_hash_fallback: false,
                extended_timeout: Duration::from_secs(170),
                extended_hash_timeout: Duration::from_secs(60),
            },
            TransportMode::Socks5 => TreeStateRetryProfile {
                max_attempts: 1,
                base_timeout: Duration::from_secs(150),
                timeout_step: Duration::ZERO,
                max_timeout: Duration::from_secs(150),
                initial_backoff: Duration::from_millis(500),
                max_backoff: Duration::from_secs(2),
                bridge_timeout_cap: Duration::from_secs(150),
                hash_timeout_cap: Duration::from_secs(30),
                enable_hash_fallback: false,
                extended_timeout: Duration::from_secs(170),
                extended_hash_timeout: Duration::from_secs(60),
            },
            // A mainnet historical state on the configured server currently takes
            // around one minute. One appropriately-bounded request is cheaper and
            // more reliable than several requests that time out while the server
            // continues processing them.
            TransportMode::Direct => TreeStateRetryProfile {
                max_attempts: 1,
                base_timeout: Duration::from_secs(120),
                timeout_step: Duration::ZERO,
                max_timeout: Duration::from_secs(120),
                initial_backoff: Duration::from_millis(250),
                max_backoff: Duration::from_secs(1),
                bridge_timeout_cap: Duration::from_secs(120),
                hash_timeout_cap: Duration::from_secs(30),
                enable_hash_fallback: false,
                extended_timeout: Duration::from_secs(170),
                extended_hash_timeout: Duration::from_secs(60),
            },
        }
    }

    fn orchard_tree_required(&self, tree_height: u64) -> bool {
        let Ok(height_u32) = u32::try_from(tree_height) else {
            // If we somehow exceed u32 range, prefer requiring Orchard tree data.
            return true;
        };
        PirateParamsNetwork::from_type(self.network_type).is_ironwood_active_with_resolved_height(
            height_u32,
            self.resolved_ironwood_activation_height(),
        )
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
                    if orchard_required && state.ironwood_tree.is_empty() {
                        Some("missing_ironwood_tree".to_string())
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
                            if orchard_required && state.ironwood_tree.is_empty() {
                                legacy_hash_err = Some("missing_ironwood_tree".to_string());
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
                        if orchard_required && state.ironwood_tree.is_empty() {
                            Some("missing_ironwood_tree".to_string())
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
                                if orchard_required && state.ironwood_tree.is_empty() {
                                    extended_legacy_hash_err =
                                        Some("missing_ironwood_tree".to_string());
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
        if self
            .client
            .start_historical_endpoint_pool_probe(start, end.saturating_add(1))
            .await
        {
            tracing::debug!("Canonical lightwalletd endpoint validation started beside sync setup");
        }
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
        let mut historical_prefill_task: Option<HistoricalPrefillTask> = None;
        let mut current_height = start;
        let mut last_checkpoint_height = start.saturating_sub(1);
        let mut last_major_checkpoint_height = start.saturating_sub(1);
        let mut tree_replay_target = None;
        let mut batches_since_mini_checkpoint = 0u32;

        // Local scan batching is independent of the server-facing stream. The
        // profile supplies safety bounds and an initial hint; sustained work
        // measurements control the target inside those bounds.
        let mut current_target_bytes = self.config.target_batch_bytes;
        let mut adaptive_scan_batcher = AdaptiveScanBatcher::with_parallelism(
            self.config.batch_size,
            self.config.min_batch_size,
            self.config.max_batch_size,
            DEFAULT_NETWORK_SCAN_BATCH_TARGET,
            prefetched_batch_encoded_byte_cap(&self.config),
            self.decrypt_pool.current_num_threads(),
        );
        let mut adaptive_shielded_work_batcher =
            AdaptiveShieldedWorkBatcher::new(self.decrypt_pool.current_num_threads());
        let cached_target_work_items = adaptive_shielded_work_batcher.cached_target_items();
        let prefetch_flow = Arc::new(PrefetchFlowControl {
            target_blocks: AtomicU64::new(adaptive_scan_batcher.target_blocks()),
            target_bytes: AtomicU64::new(
                current_target_bytes.min(prefetched_batch_encoded_byte_cap(&self.config)),
            ),
            target_work_items: AtomicU64::new(adaptive_shielded_work_batcher.target_items()),
            cached_target_work_items,
            sapling_work_factor: self.trial_decrypt_keys.sapling_ivks.len().max(1) as u64,
            ironwood_work_factor: self.trial_decrypt_keys.orchard_ivks.len().max(1) as u64,
            durable_segment_blocks: Arc::new(AtomicU64::new(DEFAULT_DURABLE_SEGMENT_BLOCKS)),
            watermarks: PrefetchWatermarks::new(
                self.config.prefetch_queue_max_bytes,
                self.config.prefetch_queue_max_bytes / 2,
            ),
        });
        let mut network_max_batch_blocks = adaptive_scan_batcher.target_blocks();
        let mut consecutive_fetch_failures = 0u32;
        let mut consecutive_heavy_batches = 0u32;
        let initial_block_size_estimate =
            (self.config.target_batch_bytes / self.config.batch_size.max(1)).max(1);
        let mut avg_block_size_estimate = if self.config.use_server_batch_recommendations {
            // Until the first real batch is measured, assume we might be entering
            // a dense range. This prevents a speculative multi-thousand block
            // fetch from landing on memory-constrained mobile devices before
            // adaptive byte sizing has any telemetry.
            initial_block_size_estimate.max(self.config.heavy_block_threshold_bytes.max(1))
        } else {
            initial_block_size_estimate
        };
        let mut prefetch_queue: VecDeque<PrefetchTask> = VecDeque::new();
        let mut validated_cache_range: Option<ValidatedCacheRange>;
        let mut previous_processed_hash: Option<Vec<u8>> = None;
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
            self.progress.write().await.set_stage(SyncStage::TreeState);
            match self.initialize_shardtrees_for_sync(start).await? {
                FrontierInitSource::RemoteTreeState => {
                    tracing::info!(
                        "ShardTrees seeded from remote tree state for sync start {}",
                        start
                    );
                }
                FrontierInitSource::ReplayFrom(replay_start) => {
                    current_height = replay_start;
                    last_checkpoint_height = replay_start.saturating_sub(1);
                    last_major_checkpoint_height = replay_start.saturating_sub(1);
                    tree_replay_target = (replay_start < start).then_some(start.saturating_sub(1));
                    aux_state = Some(SyncAuxState::new(replay_start));
                    let progress = self.progress.write().await;
                    progress.set_current(replay_start.saturating_sub(1));
                    progress.set_stage(SyncStage::Headers);
                    tracing::info!(
                        "Rebuilding commitment trees from compact blocks at height {} before wallet scanning begins at {}",
                        replay_start,
                        self.birthday_height
                    );
                }
                FrontierInitSource::LocalSnapshot => {}
            }
            self.progress.write().await.set_stage(SyncStage::Headers);
        }

        if let Some(db) = run_db.as_ref() {
            let sapling_position = *self.sapling_tree_position.read().await;
            let orchard_position = *self.orchard_tree_position.read().await;
            let prefill_timeout = match self.client.transport_mode() {
                TransportMode::Direct => Duration::from_secs(2),
                TransportMode::Tor | TransportMode::I2p | TransportMode::Socks5 => {
                    Duration::from_secs(8)
                }
            };
            let network = PirateParamsNetwork::from_type(self.network_type);
            let fetch_ironwood = network
                .ironwood_activation_height
                .or(self.resolved_ironwood_activation_height())
                .is_some_and(|height| end >= u64::from(height));
            let (local_prefill, remote_request) = prepare_historical_subtree_roots(
                db.conn(),
                sapling_position,
                orchard_position,
                end,
                &self.historical_sapling_mark_subtrees,
                &self.historical_ironwood_mark_subtrees,
                fetch_ironwood,
            )?;
            historical_prefill_state = Some(local_prefill);
            let remote_request = remote_request.and_then(|mut request| {
                let (sapling_requested, ironwood_requested) = request.requested_pools();
                let sapling_allowed = !sapling_requested
                    || self
                        .client
                        .subtree_root_probe_allowed(crate::proto_types::ShieldedProtocol::Sapling);
                let ironwood_allowed = !ironwood_requested
                    || self
                        .client
                        .subtree_root_probe_allowed(crate::proto_types::ShieldedProtocol::Ironwood);
                if (!sapling_allowed && sapling_requested)
                    || (!ironwood_allowed && ironwood_requested)
                {
                    append_sync_decision_log(
                        "sync.rs:sync_range_internal",
                        "subtree-root probe suppressed by endpoint capability cache",
                        format!(
                            "\"sapling_suppressed\":{},\"ironwood_suppressed\":{}",
                            sapling_requested && !sapling_allowed,
                            ironwood_requested && !ironwood_allowed,
                        ),
                    );
                }
                request
                    .retain_capabilities(sapling_allowed, ironwood_allowed)
                    .then_some(request)
            });
            historical_prefill_task = remote_request.map(|request| {
                HistoricalPrefillTask::spawn(
                    self.client.clone(),
                    request,
                    prefill_timeout,
                    self.cancel.clone(),
                )
            });

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
            let warm_cache_requested = warm_shardtree_cache_with_subtrees_enabled();
            // The ordered persistence worker owns all scan-time ShardTree writes.
            // The older borrowed-connection cache cannot coexist with that ownership
            // model without creating a second writer.
            let warm_cache_opt_in = false;

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
                let reason = if warm_cache_requested {
                    "ordered_persistence_worker_owns_shardtree_writes"
                } else if subtree_roots_used {
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
                        warm_cache_requested
                    ),
                );
            }
        }

        let cache_validation_start = Instant::now();
        let ((), cache_range) = tokio::try_join!(
            self.cleanup_orchard_false_positives(),
            Self::validate_cache_horizon(&self.client, current_height, end),
        )?;
        validated_cache_range = cache_range;
        let cache_validation_ms = cache_validation_start.elapsed().as_millis();
        let roots_ready_before_scan = merge_ready_historical_prefill(
            &mut historical_prefill_task,
            &mut historical_prefill_state,
        )
        .await;
        if !roots_ready_before_scan && historical_prefill_task.is_some() {
            append_sync_decision_log(
                "sync.rs:sync_range_internal",
                "remote subtree roots still loading; scan continuing",
                format!("\"cache_validation_ms\":{}", cache_validation_ms),
            );
        }
        if verbose_sync_batch_logging_enabled() {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let (validated_start, validated_end) = validated_cache_range
                .map(|range| (range.start, range.end))
                .unwrap_or((0, 0));
            append_debug_log_line(&format!(
                r#"{{"id":"log_cache_horizon","timestamp":{},"location":"sync.rs:sync_range_internal","message":"block cache horizon validation","data":{{"requested_start":{},"requested_end":{},"validated_start":{},"validated_end":{},"ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                ts, current_height, end, validated_start, validated_end, cache_validation_ms
            ));
        }

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

        let shardtree_cache_limit_bytes =
            persistence_shardtree_cache_limit(self.config.max_batch_memory_bytes);
        let persistence_worker = self
            .storage
            .clone()
            .map(|sink| {
                PersistenceWorker::start_with_pool(
                    sink,
                    shardtree_cache_limit_bytes,
                    Arc::clone(&self.decrypt_pool),
                )
            })
            .transpose()?
            .map(Arc::new);

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

        let mut repair_attempts: HashMap<(u64, u64), u8> = HashMap::new();

        // Outer loop: Keep syncing until we're fully caught up with no new blocks
        'sync_outer: loop {
            if let Some((repair_start, repair_end_exclusive)) = self
                .activate_queued_found_note_range(persistence_worker.as_deref())
                .await?
            {
                let attempts = repair_attempts
                    .entry((repair_start, repair_end_exclusive))
                    .or_default();
                *attempts = attempts.saturating_add(1);
                if *attempts > 1 {
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_repair_repeat_guard","timestamp":{},"location":"sync.rs:sync_range_internal","message":"stopped repeated witness repair loop","data":{{"repair_start":{},"repair_end_exclusive":{},"attempts":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"B"}}"#,
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis(),
                        repair_start,
                        repair_end_exclusive,
                        attempts
                    ));
                    return Err(Error::Sync(format!(
                        "Witness repair range {}..{} remained unresolved after a complete replay",
                        repair_start, repair_end_exclusive
                    )));
                }
                let repair_end_height = repair_end_exclusive.saturating_sub(1).max(repair_start);
                let rollback_target = repair_start.saturating_sub(1);
                let rollback_height = self
                    .rollback_to_checkpoint(rollback_target, persistence_worker.as_deref())
                    .await?;
                // IMPORTANT:
                // Repairs must replay deterministically from the rollback point, regardless
                // of wallet birthday / resume height heuristics. Skipping blocks here will
                // corrupt shardtree state (missing commitments) and can lead to
                // "unknown-anchor" rejections at broadcast time.
                let replay_start = rollback_height.saturating_add(1).max(1);

                // A FoundNote repair must expand compact subtree roots into
                // their underlying leaves so the owned position is marked and
                // witnessable. Reusing the historical graft/skip state would
                // skip the same subtree again and queue the same repair forever.
                historical_prefill_state = None;

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
                previous_processed_hash = None;
                {
                    let progress = self.progress.write().await;
                    progress.set_current(rollback_height);
                    progress.set_checkpoint(rollback_height);
                    progress.set_stage(SyncStage::Headers);
                }
                Self::abort_prefetch_queue(&mut prefetch_queue);
            }

            // Main sync loop: sync from current_height to end
            'sync_main: while current_height <= end {
                // Remote roots are optional and may arrive after scanning starts.
                // Merge only between committed batches so leaf/root ordering is unchanged.
                merge_ready_historical_prefill(
                    &mut historical_prefill_task,
                    &mut historical_prefill_state,
                )
                .await;

                // Check for cancellation
                if self.is_cancelled().await {
                    tracing::warn!("Sync cancelled at height {}", current_height);
                    Self::abort_prefetch_queue(&mut prefetch_queue);
                    return Err(Error::Cancelled);
                }

                let batch_start_time = Instant::now();
                let mut persist_ms: u128 = 0;
                let mut apply_spends_ms: u128 = 0;
                let mut chain_blocks_ms: u128 = 0;
                let mut tx_meta_prepare_ms: u128 = 0;
                let mut checkpoint_ms: u128 = 0;
                let mut checkpoint_written_this_batch = false;
                let mut emergency_checkpoint_requested = false;

                let prefetch_plan_start = Instant::now();
                let prefetch_end =
                    tree_replay_prefetch_end(tree_replay_target, current_height, end);
                self.fill_prefetch_queue(
                    &mut prefetch_queue,
                    (current_height, prefetch_end),
                    validated_cache_range,
                    Arc::clone(&prefetch_flow),
                );
                let prefetch_plan_ms = prefetch_plan_start.elapsed().as_millis();

                let mut prefetch_task = prefetch_queue.pop_front().ok_or_else(|| {
                    Error::Sync(format!(
                        "Prefetch queue unexpectedly empty at height {}",
                        current_height
                    ))
                })?;
                let planned_batch_start = prefetch_task.start;
                let planned_batch_end = prefetch_task.end;
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
                        planned_batch_start,
                        planned_batch_end,
                        planned_batch_end - planned_batch_start + 1
                    ));
                }
                // #endregion

                let fetch_wait_start = Instant::now();
                let fetched_result = self.receive_prefetch_batch(&mut prefetch_task).await;
                let fetch_wait_ms = fetch_wait_start.elapsed().as_millis();

                let received_batch = match fetched_result {
                    Ok(batch) => batch,
                    Err(Error::Cancelled) => {
                        Self::abort_prefetch_task(&mut prefetch_task);
                        Self::abort_prefetch_queue(&mut prefetch_queue);
                        return Err(Error::Cancelled);
                    }
                    Err(error) => {
                        Self::abort_prefetch_task(&mut prefetch_task);
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let id = format!("{:08x}", ts);
                        append_debug_log_line(&format!(
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:fetch_blocks","message":"fetch batch error","data":{{"start":{},"end":{},"error":"{}","non_retryable":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"D"}}"#,
                            id,
                            ts,
                            planned_batch_start,
                            planned_batch_end,
                            format!("{}", error).replace('"', "'"),
                            Self::is_non_retryable_fetch_error(&error)
                        ));

                        if Self::is_non_retryable_fetch_error(&error) {
                            if planned_batch_start <= LOW_HEIGHT_BATCH_CAP_HEIGHT {
                                if let Some(floor) = self.server_compact_floor_hint().await {
                                    if floor > planned_batch_start && floor <= end {
                                        tracing::warn!(
                                            "Non-retryable block-range failure at {}-{}; jumping to server compact floor {}",
                                            planned_batch_start,
                                            planned_batch_end,
                                            floor
                                        );
                                        Self::abort_prefetch_queue(&mut prefetch_queue);
                                        current_height = floor;
                                        {
                                            let progress = self.progress.write().await;
                                            progress.set_stage(SyncStage::Headers);
                                            progress.set_current(current_height.saturating_sub(1));
                                        }
                                        continue 'sync_main;
                                    }
                                }
                            }

                            return Err(Error::Sync(format!(
                                "NON_RETRYABLE: block fetch failed for {}-{}: {}",
                                planned_batch_start, planned_batch_end, error
                            )));
                        }

                        consecutive_fetch_failures = consecutive_fetch_failures.saturating_add(1);
                        let batch_blocks = planned_batch_end
                            .saturating_sub(planned_batch_start)
                            .saturating_add(1);
                        let reduced_blocks = std::cmp::max(
                            self.config.min_batch_size,
                            batch_blocks.saturating_add(1) / 2,
                        );
                        network_max_batch_blocks = network_max_batch_blocks.min(reduced_blocks);
                        prefetch_flow
                            .target_blocks
                            .store(network_max_batch_blocks, Ordering::Release);
                        current_target_bytes = current_target_bytes.min(
                            avg_block_size_estimate
                                .saturating_mul(reduced_blocks)
                                .max(1),
                        );
                        tracing::warn!(
                            "Block fetch failed for {}-{}: {}; retrying from {} with max_blocks={} and target_bytes={}",
                            planned_batch_start,
                            planned_batch_end,
                            error,
                            current_height,
                            network_max_batch_blocks,
                            current_target_bytes
                        );
                        self.client.disconnect().await;
                        let _ = self.client.connect().await;
                        Self::abort_prefetch_queue(&mut prefetch_queue);
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                            _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                        }
                        continue 'sync_main;
                    }
                };
                let prepared_notes = received_batch.prepared_notes;
                let prepared_commitments = received_batch.prepared_commitments;
                let fetched_batch = received_batch.fetched;

                let batch_start = fetched_batch
                    .blocks
                    .first()
                    .map(|block| block.height)
                    .ok_or_else(|| {
                        Error::Sync("bounded compact block batch was empty".to_string())
                    })?;
                let batch_end = fetched_batch
                    .blocks
                    .last()
                    .map(|block| block.height)
                    .ok_or_else(|| {
                        Error::Sync("bounded compact block batch was empty".to_string())
                    })?;
                if batch_start != planned_batch_start || batch_end > planned_batch_end {
                    Self::abort_prefetch_task(&mut prefetch_task);
                    return Err(Error::Sync(format!(
                        "bounded compact block producer returned {}-{} for planned range {}-{}",
                        batch_start, batch_end, planned_batch_start, planned_batch_end
                    )));
                }
                if batch_end < planned_batch_end {
                    prefetch_task.start = batch_end.saturating_add(1);
                    prefetch_queue.push_front(prefetch_task);
                } else {
                    Self::abort_prefetch_task(&mut prefetch_task);
                }

                let FetchedBlockBatch {
                    blocks,
                    encoded_bytes,
                    shielded_work_items,
                    requested_blocks,
                    requested_bytes,
                    requested_work_items,
                    source: fetch_source,
                    elapsed: fetch_elapsed,
                    network_elapsed: network_fetch_elapsed,
                    cache_write_elapsed,
                    spool_reservations,
                } = fetched_batch;
                let _spool_reservations = spool_reservations;

                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:506","message":"fetch_blocks_with_retry result","data":{{"current_height":{},"batch_end":{},"blocks_count":{},"wait_ms":{},"fetch_ms":{},"network_ms":{},"cache_write_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"E"}}"#,
                        id,
                        ts,
                        batch_start,
                        batch_end,
                        blocks.len(),
                        fetch_wait_ms,
                        fetch_elapsed.as_millis(),
                        network_fetch_elapsed.as_millis(),
                        cache_write_elapsed.as_millis()
                    ));
                }
                // #endregion

                if blocks.is_empty() {
                    return Err(Error::Sync(format!(
                        "lightwalletd returned empty compact block batch for {}-{}",
                        batch_start, batch_end
                    )));
                }

                let boundary_validation_start = Instant::now();
                let boundary_is_valid = self.validate_batch_boundary(
                    batch_start,
                    &blocks,
                    run_db.as_ref(),
                    previous_processed_hash.as_deref(),
                )?;
                let boundary_validation_ms = boundary_validation_start.elapsed().as_millis();
                if !boundary_is_valid {
                    tracing::warn!(
                        "Reorg detected at batch boundary before height {}; rolling back to common ancestor",
                        batch_start
                    );
                    let resume_height = self
                        .rollback_to_common_ancestor(
                            batch_start.saturating_sub(1),
                            persistence_worker.as_deref(),
                        )
                        .await?;
                    current_height = resume_height;
                    last_checkpoint_height = resume_height.saturating_sub(1);
                    last_major_checkpoint_height = resume_height.saturating_sub(1);
                    batches_since_mini_checkpoint = 0;
                    batches_since_sync_state_flush = 0;
                    Self::abort_prefetch_queue(&mut prefetch_queue);
                    validated_cache_range = None;
                    previous_processed_hash = None;
                    continue 'sync_main;
                }
                consecutive_fetch_failures = 0;

                // Detect unusually large blocks and adapt batch size using the exact
                // protobuf bytes admitted by the bounded stream.
                let batch_sizing_start = Instant::now();
                let total_block_size = encoded_bytes.max(1);
                let avg_block_size = total_block_size / blocks.len().max(1) as u64;
                avg_block_size_estimate = avg_block_size.max(1);
                let is_heavy_batch = avg_block_size > self.config.heavy_block_threshold_bytes;
                if is_heavy_batch {
                    consecutive_heavy_batches += 1;
                    // Reduce target bytes significantly for unusually large blocks.
                    current_target_bytes =
                        std::cmp::max(self.config.min_batch_bytes, current_target_bytes / 4);
                    prefetch_flow.target_bytes.store(
                        current_target_bytes.min(prefetched_batch_encoded_byte_cap(&self.config)),
                        Ordering::Release,
                    );
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
                    // Byte density remains a local safety input. Server latency
                    // no longer changes server-visible range boundaries.
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
                        prefetch_flow.target_bytes.store(
                            current_target_bytes
                                .min(prefetched_batch_encoded_byte_cap(&self.config)),
                            Ordering::Release,
                        );
                    }
                }
                let batch_sizing_ms = batch_sizing_start.elapsed().as_millis();

                // Prefetch next batch while we process this one.
                let next_prefetch_start = Instant::now();
                let next_height = batch_end.saturating_add(1);
                let next_prefetch_end =
                    tree_replay_prefetch_end(tree_replay_target, next_height, end);
                self.fill_prefetch_queue(
                    &mut prefetch_queue,
                    (next_height, next_prefetch_end),
                    validated_cache_range,
                    Arc::clone(&prefetch_flow),
                );
                let next_prefetch_ms = next_prefetch_start.elapsed().as_millis();

                // Stage 2: Trial decryption (batched with parallelism)
                self.progress.write().await.set_stage(SyncStage::Notes);
                let wallet_blocks = wallet_relevant_blocks(&blocks, self.birthday_height);
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
                        wallet_blocks.len()
                    ));
                }
                // #endregion
                let decrypt_start = Instant::now();
                let decrypt_was_prepared = prepared_notes.is_some();
                let commitment_preparation_was_ahead = prepared_commitments.is_some();
                let ((mut notes, decrypt_telemetry), prepared_commitments) =
                    match (prepared_notes, prepared_commitments) {
                        (Some(decrypted), Some(commitments)) => (decrypted, commitments),
                        (None, None) => {
                            let (decrypted, commitments) = self.decrypt_pool.install(|| {
                                rayon::join(
                                    || self.trial_decrypt_batch_sync(wallet_blocks),
                                    || prepare_commitment_batch(&blocks),
                                )
                            });
                            (decrypted?, commitments)
                        }
                        _ => {
                            return Err(Error::Sync(
                                "One-batch lookahead returned incomplete immutable work"
                                    .to_string(),
                            ));
                        }
                    };
                let commitment_prepare_ms = prepared_commitments.elapsed.as_millis();
                let prepared_sapling_commitments = prepared_commitments.sapling_count;
                let prepared_ironwood_commitments = prepared_commitments.ironwood_count;
                let decrypt_pool_wall_ms = decrypt_telemetry.pool_wall.as_millis();
                let decrypt_worker_active_ms = decrypt_telemetry.worker_active.as_millis();
                let decrypt_worker_tasks = decrypt_telemetry.task_count;
                let decrypt_ms = if decrypt_was_prepared {
                    0
                } else {
                    decrypt_start.elapsed().as_millis()
                };
                self.start_one_batch_decrypt_lookahead(&mut prefetch_queue);
                // #region agent log
                if verbose_sync_batch_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:852","message":"trial_decrypt done","data":{{"start":{},"end":{},"notes":{},"sync_wait_ms":{},"pool_wall_ms":{},"worker_active_ms":{},"worker_tasks":{},"commitment_prepare_ahead":{},"commitment_prepare_ms":{},"prepared_sapling_commitments":{},"prepared_ironwood_commitments":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        current_height,
                        batch_end,
                        notes.len(),
                        decrypt_ms,
                        decrypt_pool_wall_ms,
                        decrypt_worker_active_ms,
                        decrypt_worker_tasks,
                        commitment_preparation_was_ahead,
                        commitment_prepare_ms,
                        prepared_sapling_commitments,
                        prepared_ironwood_commitments,
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
                let (
                    commitments_applied,
                    position_mappings,
                    frontier_checkpointed_batch_end,
                    shardtree_work,
                ) = self
                    .update_commitment_trees(
                        &blocks,
                        prepared_commitments,
                        &notes,
                        checkpoint_mode,
                        warm_trees.as_mut(),
                        historical_prefill_state.as_mut(),
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await?;
                let common_checkpoint_safe = historical_prefill_state
                    .as_ref()
                    .is_none_or(HistoricalPrefillState::common_checkpoint_safe);
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
                    self.apply_diversifier_indices(&mut notes);
                }

                let require_memos = !self.config.lazy_memo_decode;
                if !notes.is_empty() && !self.config.defer_full_tx_fetch {
                    self.fetch_and_enrich_notes(
                        &mut notes,
                        require_memos,
                        persistence_worker.clone(),
                    )
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
                            && note.note_type == NoteType::Ironwood
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

                        let persist_result = if let Some(worker) = persistence_worker.as_ref() {
                            let sink = sink.clone();
                            let persisted_notes = notes.clone();
                            let persisted_positions = position_mappings.clone();
                            worker
                                .execute(move |db| {
                                    sink.persist_notes_with_db(
                                        db,
                                        &persisted_notes,
                                        &tx_times,
                                        &tx_fees,
                                        &persisted_positions,
                                    )
                                })
                                .await?
                        } else if let Some(db) = run_db.as_ref() {
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

                if !(wallet_blocks.is_empty()
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
                    self.apply_spends(
                        wallet_blocks,
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await?;
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

                if self.config.defer_full_tx_fetch && !notes.is_empty() {
                    self.spawn_background_enrich(
                        notes.clone(),
                        require_memos,
                        persistence_worker.clone(),
                    );
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

                if emergency_checkpoint_requested && common_checkpoint_safe {
                    if !checkpoint_written_this_batch {
                        let emergency_checkpoint_start = Instant::now();
                        self.create_checkpoint(
                            batch_end,
                            warm_trees.as_mut(),
                            run_db.as_ref(),
                            persistence_worker.as_deref(),
                        )
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
                        "Emergency checkpoint at {} after dense blocks (target bytes: {})",
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
                    && common_checkpoint_safe
                {
                    if !checkpoint_written_this_batch {
                        let mini_checkpoint_start = Instant::now();
                        self.create_checkpoint(
                            batch_end,
                            warm_trees.as_mut(),
                            run_db.as_ref(),
                            persistence_worker.as_deref(),
                        )
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
                if blocks_since_major_checkpoint >= major_checkpoint_interval
                    && common_checkpoint_safe
                {
                    if !checkpoint_written_this_batch {
                        let major_checkpoint_start = Instant::now();
                        self.create_checkpoint(
                            batch_end,
                            warm_trees.as_mut(),
                            run_db.as_ref(),
                            persistence_worker.as_deref(),
                        )
                        .await?;
                        checkpoint_ms += major_checkpoint_start.elapsed().as_millis();
                        checkpoint_written_this_batch = true;
                    }
                    // Sparse historical checkpoints are recovery anchors, not
                    // merely progress markers. Pin them so the dense tip
                    // checkpoint window cannot prune every rollback point and
                    // turn a local witness repair into a birthday-wide replay.
                    self.retain_checkpoint(
                        batch_end,
                        warm_trees.as_mut(),
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await?;
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

                // Keep the persisted cursor aligned with a durable tree checkpoint
                // while a one-time historical frontier replay is in progress. The
                // exact requested frontier is also checkpointed at a forced batch
                // boundary so later starts can reuse it directly.
                let periodic_sync_state_flush_due = batch_end >= end
                    || batches_since_sync_state_flush >= self.config.sync_state_flush_every_batches
                    || last_sync_state_flush.elapsed().as_millis()
                        >= self.config.sync_state_flush_interval_ms as u128;
                if tree_replay_checkpoint_due(
                    tree_replay_target,
                    batch_end,
                    periodic_sync_state_flush_due,
                    checkpoint_written_this_batch,
                ) && common_checkpoint_safe
                {
                    self.create_checkpoint(
                        batch_end,
                        warm_trees.as_mut(),
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await?;
                    checkpoint_written_this_batch = true;
                    last_checkpoint_height = batch_end;
                    self.progress.write().await.set_checkpoint(batch_end);
                }

                if tree_replay_target.is_some_and(|target| batch_end >= target) {
                    self.retain_checkpoint(
                        batch_end,
                        warm_trees.as_mut(),
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await?;
                    tree_replay_target = None;
                }

                // Save sync state periodically
                let should_flush_sync_state =
                    checkpoint_written_this_batch || periodic_sync_state_flush_due;
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
                    let (chain_elapsed_ms, state_elapsed_ms) = self
                        .save_sync_state(
                            batch_end,
                            end,
                            last_checkpoint_height,
                            canonical_block_window(&blocks),
                            include_aux_state_update,
                            run_db.as_ref(),
                            persistence_worker.as_deref(),
                        )
                        .await?;
                    chain_blocks_ms = chain_elapsed_ms;
                    batches_since_sync_state_flush = 0;
                    last_sync_state_flush = Instant::now();
                    if include_aux_state_update {
                        if let Some(aux) = aux_state.as_mut() {
                            aux.mark_flushed(batch_end);
                        }
                    }
                    state_elapsed_ms
                } else {
                    batches_since_sync_state_flush =
                        batches_since_sync_state_flush.saturating_add(1);
                    0
                };

                let batch_full_elapsed = batch_start_time.elapsed();
                let previous_scan_target = network_max_batch_blocks;
                let tree_parallel_wall = shardtree_work
                    .sapling_work
                    .parallel_construction
                    .saturating_add(shardtree_work.ironwood_work.parallel_construction);
                let tree_parallel_worker_active = shardtree_work
                    .sapling_work
                    .parallel_worker_active
                    .saturating_add(shardtree_work.ironwood_work.parallel_worker_active);
                let local_processing_time =
                    batch_full_elapsed.saturating_sub(Duration::from_millis(fetch_wait_ms as u64));
                network_max_batch_blocks = adaptive_scan_batcher.observe(ScanBatchObservation {
                    requested_blocks,
                    requested_bytes,
                    blocks: blocks.len() as u64,
                    encoded_bytes: total_block_size,
                    processing_time: local_processing_time,
                    intake_wait: Duration::from_millis(fetch_wait_ms as u64),
                    queued_bytes: prefetch_flow.watermarks.queued_bytes(),
                    source: match fetch_source {
                        BlockFetchSource::Cache => ScanBatchSource::Cache,
                        BlockFetchSource::Network => ScanBatchSource::Network,
                    },
                    tree_parallel_wall,
                    tree_parallel_worker_active,
                    stream_tail: batch_end >= end,
                });
                prefetch_flow
                    .target_blocks
                    .store(network_max_batch_blocks, Ordering::Release);
                let (previous_work_target, next_work_target) = match fetch_source {
                    BlockFetchSource::Cache => (
                        prefetch_flow.cached_target_work_items,
                        prefetch_flow.cached_target_work_items,
                    ),
                    BlockFetchSource::Network => {
                        let previous = adaptive_shielded_work_batcher.target_items();
                        let next = adaptive_shielded_work_batcher.observe(
                            requested_work_items,
                            shielded_work_items,
                            local_processing_time,
                            batch_end >= end,
                        );
                        prefetch_flow
                            .target_work_items
                            .store(next, Ordering::Release);
                        (previous, next)
                    }
                };
                if previous_scan_target != network_max_batch_blocks {
                    tracing::debug!(
                        previous = previous_scan_target,
                        next = network_max_batch_blocks,
                        source = fetch_source.as_str(),
                        decision = adaptive_scan_batcher.last_decision().as_str(),
                        queued_bytes = prefetch_flow.watermarks.queued_bytes(),
                        "adapted local scan batch target"
                    );
                }
                if previous_work_target != next_work_target {
                    tracing::debug!(
                        previous = previous_work_target,
                        next = next_work_target,
                        completed = shielded_work_items,
                        processing_ms = local_processing_time.as_millis(),
                        "adapted local shielded-work target"
                    );
                }

                // #region agent log
                if sync_performance_logging_enabled() {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    let id = format!("{:08x}", ts);
                    let wallet_id = self.wallet_id.as_deref().unwrap_or("unknown");
                    let avg_block_size = total_block_size / blocks.len().max(1) as u64;
                    let tree_effective_workers_milli = tree_parallel_worker_active
                        .as_nanos()
                        .saturating_mul(1_000)
                        .checked_div(tree_parallel_wall.as_nanos().max(1))
                        .unwrap_or(u128::from(u64::MAX))
                        .min(u128::from(u64::MAX))
                        as u64;
                    let known_processing_ms = fetch_wait_ms
                        + decrypt_ms
                        + frontier_ms
                        + persist_ms
                        + apply_spends_ms
                        + chain_blocks_ms
                        + prefetch_plan_ms
                        + batch_sizing_ms
                        + next_prefetch_ms
                        + note_post_ms
                        + tx_meta_prepare_ms;
                    let known_processing_ms = known_processing_ms + boundary_validation_ms;
                    let residual_processing_other_ms =
                        batch_processing_ms.saturating_sub(known_processing_ms);
                    let batch_full_ms = batch_full_elapsed.as_millis();
                    let known_full_ms =
                        known_processing_ms + perf_progress_ms + checkpoint_ms + sync_state_ms;
                    let residual_full_other_ms = batch_full_ms.saturating_sub(known_full_ms);
                    append_debug_log_line(&format!(
                        r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:915","message":"batch_stage_timing","data":{{"wallet_id":"{}","start":{},"end":{},"blocks":{},"notes":{},"total_bytes":{},"avg_block_bytes":{},"shielded_work_items":{},"requested_work_items":{},"shielded_work_target":{},"fetch_source":"{}","scan_requested_blocks":{},"scan_requested_bytes":{},"scan_controller_decision":"{}","scan_cached_blocks_per_second":{},"scan_cached_parallel_saturation_ppm":{},"scan_target_blocks":{},"durable_segment_blocks":{},"prefetch_queued_bytes":{},"prefetch_high_bytes":{},"fetch_wait_ms":{},"fetch_total_ms":{},"network_fetch_ms":{},"cache_write_ms":{},"boundary_validation_ms":{},"decrypt_ms":{},"decrypt_pool_wall_ms":{},"decrypt_worker_active_ms":{},"decrypt_worker_tasks":{},"tree_parallel_wall_ms":{},"tree_parallel_worker_active_ms":{},"tree_effective_workers_milli":{},"frontier_ms":{},"persist_ms":{},"apply_spends_ms":{},"chain_blocks_ms":{},"prefetch_plan_ms":{},"batch_sizing_ms":{},"next_prefetch_ms":{},"note_post_ms":{},"tx_meta_prepare_ms":{},"perf_progress_ms":{},"checkpoint_ms":{},"sync_state_ms":{},"residual_processing_other_ms":{},"residual_full_other_ms":{},"batch_total_ms":{},"batch_full_ms":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                        id,
                        ts,
                        wallet_id,
                        current_height,
                        batch_end,
                        blocks.len(),
                        notes.len(),
                        total_block_size,
                        avg_block_size,
                        shielded_work_items,
                        requested_work_items,
                        next_work_target,
                        fetch_source.as_str(),
                        requested_blocks,
                        requested_bytes,
                        adaptive_scan_batcher.last_decision().as_str(),
                        adaptive_scan_batcher.cached_blocks_per_second(),
                        adaptive_scan_batcher.cached_parallel_saturation_ppm(),
                        network_max_batch_blocks,
                        prefetch_flow.durable_segment_blocks.load(Ordering::Acquire),
                        prefetch_flow.watermarks.queued_bytes(),
                        prefetch_flow.watermarks.high_bytes(),
                        fetch_wait_ms,
                        fetch_elapsed.as_millis(),
                        network_fetch_elapsed.as_millis(),
                        cache_write_elapsed.as_millis(),
                        boundary_validation_ms,
                        decrypt_ms,
                        decrypt_pool_wall_ms,
                        decrypt_worker_active_ms,
                        decrypt_worker_tasks,
                        tree_parallel_wall.as_millis(),
                        tree_parallel_worker_active.as_millis(),
                        tree_effective_workers_milli,
                        frontier_ms,
                        persist_ms,
                        apply_spends_ms,
                        chain_blocks_ms,
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

                previous_processed_hash = blocks.last().map(|block| block.hash.clone());
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

                    // A sampled subtree can end beyond the historical grafting
                    // ceiling. Materialize any remaining leaves before creating
                    // the tip checkpoint or validating owned-note witnesses.
                    self.flush_historical_prefill_buffers(
                        &mut historical_prefill_state,
                        &mut warm_trees,
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await?;

                    if tip_height > last_checkpoint_height {
                        match self
                            .create_checkpoint(
                                tip_height,
                                warm_trees.as_mut(),
                                run_db.as_ref(),
                                persistence_worker.as_deref(),
                            )
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
                        .check_witnesses_and_queue_rescans(
                            tip_height,
                            run_db.as_ref(),
                            persistence_worker.as_deref(),
                        )
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

            self.flush_historical_prefill_buffers(
                &mut historical_prefill_state,
                &mut warm_trees,
                run_db.as_ref(),
                persistence_worker.as_deref(),
            )
            .await?;

            // For bounded ranges (e.g. witness repair replay), persist a final
            // frontier checkpoint before returning so anchor-hydrated selection
            // can immediately use the repaired range.
            //
            // Without this forced checkpoint, short replay runs can finish before
            // periodic mini/major checkpoint thresholds are met, leaving send
            // selection with stale snapshot coverage.
            if !follow_tip {
                if let Err(e) = self
                    .create_checkpoint(
                        end,
                        warm_trees.as_mut(),
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await
                {
                    tracing::warn!(
                        "Failed to persist bounded-range final checkpoint at {}: {}",
                        end,
                        e
                    );
                } else if let (Some(db), Some(trees)) = (run_db.as_ref(), warm_trees.take()) {
                    warm_trees = Some(trees.flush_and_reload(db.conn())?);
                }
                match self
                    .run_tip_witness_validation(
                        end,
                        "bounded_sync_complete",
                        persistence_worker.as_deref(),
                    )
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
                Self::abort_prefetch_queue(&mut prefetch_queue);
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
                Self::abort_prefetch_queue(&mut prefetch_queue);
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
                match self
                    .create_checkpoint(
                        current,
                        warm_trees.as_mut(),
                        run_db.as_ref(),
                        persistence_worker.as_deref(),
                    )
                    .await
                {
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
                .check_witnesses_and_queue_rescans(
                    current,
                    run_db.as_ref(),
                    persistence_worker.as_deref(),
                )
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
                        .validate_resume_chain(
                            current.saturating_add(1),
                            latest_height,
                            ResumeChainPolicy::RepairMetadataGap,
                        )
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
                        Self::abort_prefetch_queue(&mut prefetch_queue);
                        continue 'sync_outer;
                    }

                    if latest_height > current {
                        self.require_server_consensus_branch().await?;
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
                            Self::abort_prefetch_queue(&mut prefetch_queue);
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
                    match self.client.connect().await {
                        Ok(()) => self.require_server_consensus_branch().await?,
                        Err(conn_err) => tracing::warn!("Reconnect failed: {}", conn_err),
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                        _ = self.cancel.cancelled() => {
                            Self::abort_prefetch_queue(&mut prefetch_queue);
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
            for (tx_index, tx) in block.transactions.iter().enumerate() {
                if tx.hash.len() != 32 {
                    return Err(Error::Sync(format!(
                        "compact block {} transaction {} has invalid hash length {}",
                        block.height,
                        tx_index,
                        tx.hash.len()
                    )));
                }
                for spend in &tx.spends {
                    if spend.nf.len() != 32 {
                        return Err(Error::Sync(format!(
                            "compact block {} transaction {} has an invalid Sapling nullifier",
                            block.height, tx_index
                        )));
                    }
                }
                for output in &tx.outputs {
                    if output.cmu.len() != 32
                        || output.ephemeral_key.len() != 32
                        || output.ciphertext.len() < 52
                    {
                        return Err(Error::Sync(format!(
                            "compact block {} transaction {} has an invalid Sapling output",
                            block.height, tx_index
                        )));
                    }
                }
                for action in &tx.actions {
                    if action.nullifier.len() != 32
                        || action.cmx.len() != 32
                        || action.ephemeral_key.len() != 32
                        || action.enc_ciphertext.len() < 52
                    {
                        return Err(Error::Sync(format!(
                            "compact block {} transaction {} has an invalid Ironwood action",
                            block.height, tx_index
                        )));
                    }
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
        validated_cache_range: Option<ValidatedCacheRange>,
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

        if validated_cache_range.is_some_and(|range| range.contains(start, end)) {
            return Ok(true);
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

    async fn validate_cache_horizon(
        client: &LightClient,
        start: u64,
        end: u64,
    ) -> Result<Option<ValidatedCacheRange>> {
        let cache = match BlockCache::for_endpoint(client.endpoint()) {
            Ok(cache) => cache,
            Err(e) => {
                tracing::debug!("Block cache unavailable for range validation: {}", e);
                return Ok(None);
            }
        };
        let Some(cached_end) = cache.contiguous_end(start, end)? else {
            return Ok(None);
        };
        let anchor = match cache.load_range_for_upgrade(cached_end, cached_end) {
            Ok(anchor) => anchor,
            Err(error) => {
                tracing::warn!(
                    "Could not decode cached compact block anchor at {}; invalidating {}-{}: {}",
                    cached_end,
                    start,
                    cached_end,
                    error
                );
                let _ = cache.delete_range(start, cached_end);
                return Ok(None);
            }
        };
        if let Err(e) = Self::validate_compact_block_range(cached_end, cached_end, &anchor.blocks) {
            tracing::warn!(
                "Invalid cached compact block anchor at {}; invalidating {}-{}: {}",
                cached_end,
                start,
                cached_end,
                e
            );
            let _ = cache.delete_range(start, cached_end);
            return Ok(None);
        }

        let cached_hash = &anchor.blocks[0].hash;
        match client.get_block(height_to_u32(cached_end)?).await {
            Ok(remote) if remote.hash == *cached_hash => {
                tracing::debug!(
                    "Validated contiguous block cache range {}-{} against remote anchor",
                    start,
                    cached_end
                );
                Ok(Some(ValidatedCacheRange {
                    start,
                    end: cached_end,
                }))
            }
            Ok(remote) => {
                tracing::warn!(
                    "Cached compact block anchor {} is stale (cache={}, remote={}); invalidating {}-{}",
                    cached_end,
                    hex::encode(cached_hash),
                    hex::encode(&remote.hash),
                    start,
                    cached_end
                );
                let _ = cache.delete_range(start, cached_end);
                Ok(None)
            }
            Err(e) => {
                tracing::warn!(
                    "Could not validate cached compact block horizon {}-{} against remote: {}; retaining per-batch validation",
                    start,
                    cached_end,
                    e
                );
                Ok(None)
            }
        }
    }

    #[cfg(test)]
    async fn fetch_blocks_with_retry_inner(
        client: LightClient,
        start: u64,
        end: u64,
        cancel: CancelToken,
        wallet_id: Option<String>,
        validated_cache_range: Option<ValidatedCacheRange>,
        max_chunk_bytes: u64,
    ) -> Result<FetchedBlockBatch> {
        let started = Instant::now();
        let (blocks, source, timings) = Self::fetch_blocks_with_retry_unmeasured(
            client,
            start,
            end,
            cancel,
            wallet_id,
            validated_cache_range,
            max_chunk_bytes,
        )
        .await?;
        let shielded_work_items = blocks
            .iter()
            .map(|block| block.shielded_work_items(1, 1))
            .sum();
        Ok(FetchedBlockBatch {
            encoded_bytes: timings.encoded_bytes,
            blocks,
            shielded_work_items,
            requested_blocks: end.saturating_sub(start).saturating_add(1),
            requested_bytes: max_chunk_bytes,
            requested_work_items: u64::MAX,
            source,
            elapsed: started.elapsed(),
            network_elapsed: timings.network_elapsed,
            cache_write_elapsed: timings.cache_write_elapsed,
            spool_reservations: Vec::new(),
        })
    }

    #[cfg(test)]
    async fn fetch_blocks_with_retry_unmeasured(
        client: LightClient,
        start: u64,
        end: u64,
        cancel: CancelToken,
        wallet_id: Option<String>,
        validated_cache_range: Option<ValidatedCacheRange>,
        max_chunk_bytes: u64,
    ) -> Result<(Vec<CompactBlockData>, BlockFetchSource, BlockFetchTimings)> {
        if start > end {
            return Ok((
                Vec::new(),
                BlockFetchSource::Cache,
                BlockFetchTimings::default(),
            ));
        }

        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }

        let expected_blocks = end.saturating_sub(start).saturating_add(1) as usize;

        if let Ok(cache) = BlockCache::for_endpoint(client.endpoint()) {
            match cache.load_range_for_upgrade(start, end) {
                Ok(cached_range) if cached_range.blocks.len() == expected_blocks => {
                    let blocks = cached_range.blocks;
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
                    if Self::cached_blocks_are_canonical(
                        &client,
                        &cache,
                        start,
                        end,
                        &blocks,
                        validated_cache_range,
                    )
                    .await?
                    {
                        if !cached_range.legacy_heights.is_empty() {
                            match cache.upgrade_legacy_rows(&blocks, &cached_range.legacy_heights) {
                                Ok(upgraded) if upgraded > 0 => tracing::debug!(
                                    "Upgraded {} canonical cache rows to protobuf for {}-{}",
                                    upgraded,
                                    start,
                                    end
                                ),
                                Ok(_) => {}
                                Err(e) => tracing::debug!(
                                    "Cache codec upgrade failed for {}-{}: {}",
                                    start,
                                    end,
                                    e
                                ),
                            }
                        }
                        return Ok((
                            blocks,
                            BlockFetchSource::Cache,
                            BlockFetchTimings::default(),
                        ));
                    }
                }
                Ok(cached_range) if !cached_range.blocks.is_empty() => {
                    tracing::debug!(
                        "Block cache partial hit for {}-{} ({} of {})",
                        start,
                        end,
                        cached_range.blocks.len(),
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
                            cached_range.blocks.len(),
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
                InflightLease::Follower(waiter) => {
                    tokio::select! {
                        _ = waiter.wait() => {}
                        _ = cancel.cancelled() => return Err(Error::Cancelled),
                    }
                    if let Ok(cache) = BlockCache::for_endpoint(client.endpoint()) {
                        if let Ok(cached_range) = cache.load_range_for_upgrade(start, end) {
                            if cached_range.blocks.len() == expected_blocks
                                && Self::cached_blocks_are_canonical(
                                    &client,
                                    &cache,
                                    start,
                                    end,
                                    &cached_range.blocks,
                                    validated_cache_range,
                                )
                                .await?
                            {
                                if let Err(e) = cache.upgrade_legacy_rows(
                                    &cached_range.blocks,
                                    &cached_range.legacy_heights,
                                ) {
                                    tracing::debug!(
                                        "Cache codec upgrade failed for {}-{}: {}",
                                        start,
                                        end,
                                        e
                                    );
                                }
                                return Ok((
                                    cached_range.blocks,
                                    BlockFetchSource::Cache,
                                    BlockFetchTimings::default(),
                                ));
                            }
                        }
                    }
                    continue;
                }
                InflightLease::Leader(token) => {
                    let mut attempts = 0;
                    let mut timings = BlockFetchTimings::default();
                    let mut blocks: Vec<CompactBlockData> = Vec::with_capacity(expected_blocks);
                    let mut next_height = start;
                    let result = loop {
                        let stream_start = next_height;
                        let mut receiver = client.compact_block_chunk_stream(
                            height_to_u32(stream_start)?..height_to_u32(end.saturating_add(1))?,
                            max_chunk_bytes,
                            wallet_id.clone(),
                        );
                        let network_started = Instant::now();
                        let mut stream_error = None;
                        loop {
                            let received = tokio::select! {
                                chunk = receiver.recv() => chunk,
                                _ = cancel.cancelled() => {
                                    stream_error = Some(Error::Cancelled);
                                    None
                                }
                            };
                            let Some(received) = received else {
                                break;
                            };
                            let chunk = match received {
                                Ok(chunk) => chunk,
                                Err(error) => {
                                    stream_error = Some(error);
                                    break;
                                }
                            };
                            let Some(chunk_start) = chunk.start_height() else {
                                continue;
                            };
                            let chunk_end = chunk.end_height().ok_or_else(|| {
                                Error::Sync("bounded compact block chunk had no end".to_string())
                            })?;
                            if chunk_start != next_height {
                                stream_error = Some(Error::Sync(format!(
                                    "bounded compact block stream expected {}, received {}",
                                    next_height, chunk_start
                                )));
                                break;
                            }
                            if let Err(error) = Self::validate_compact_block_range(
                                chunk_start,
                                chunk_end,
                                &chunk.blocks,
                            ) {
                                stream_error = Some(error);
                                break;
                            }
                            if let (Some(previous), Some(first)) =
                                (blocks.last(), chunk.blocks.first())
                            {
                                if first.prev_hash != previous.hash {
                                    stream_error = Some(Error::Sync(format!(
                                        "bounded compact block chunks disconnected at height {}",
                                        first.height
                                    )));
                                    break;
                                }
                            }

                            timings.encoded_bytes =
                                timings.encoded_bytes.saturating_add(chunk.encoded_bytes);
                            next_height = chunk_end.saturating_add(1);
                            let cache_write_started = Instant::now();
                            let endpoint = client.endpoint().to_string();
                            let chunk_blocks = chunk.blocks;
                            let (chunk_blocks, cache_result) =
                                tokio::task::spawn_blocking(move || {
                                    let result = BlockCache::for_endpoint(&endpoint)
                                        .and_then(|cache| cache.store_blocks(&chunk_blocks));
                                    (chunk_blocks, result)
                                })
                                .await
                                .map_err(|error| {
                                    Error::Sync(format!(
                                        "compact block cache worker failed for {}-{}: {}",
                                        chunk_start, chunk_end, error
                                    ))
                                })?;
                            timings.cache_write_elapsed += cache_write_started.elapsed();
                            if let Err(error) = cache_result {
                                tracing::debug!(
                                    "Block cache store failed for {}-{}: {}",
                                    chunk_start,
                                    chunk_end,
                                    error
                                );
                            }
                            blocks.extend(chunk_blocks);
                        }
                        timings.network_elapsed += network_started.elapsed();

                        if next_height > end {
                            break Self::validate_compact_block_range(start, end, &blocks)
                                .map(|_| blocks);
                        }

                        let error = stream_error.unwrap_or_else(|| {
                            Error::Network(format!(
                                "compact block stream ended at {}, expected {}",
                                next_height,
                                end.saturating_add(1)
                            ))
                        });
                        match error {
                            Error::Cancelled => break Err(Error::Cancelled),
                            error if attempts < MAX_RETRY_ATTEMPTS => {
                                attempts += 1;
                                let backoff = RETRY_BACKOFF_MS * (1 << attempts);
                                tracing::warn!(
                                    "Compact stream stopped after durable height {} (attempt {}/{}); resuming in {}ms: {}",
                                    next_height.saturating_sub(1),
                                    attempts,
                                    MAX_RETRY_ATTEMPTS,
                                    backoff,
                                    error
                                );
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_millis(backoff)) => {}
                                    _ = cancel.cancelled() => break Err(Error::Cancelled),
                                }
                            }
                            error => break Err(error),
                        }
                    };
                    token.complete();
                    return result.map(|blocks| (blocks, BlockFetchSource::Network, timings));
                }
            }
        }
    }

    fn trial_decrypt_batch_sync(
        &self,
        blocks: &[CompactBlockData],
    ) -> Result<(Vec<DecryptedNote>, TrialDecryptTelemetry)> {
        let decrypt_keys = &self.trial_decrypt_keys;

        let has_sapling_ivk = !decrypt_keys.sapling_ivks.is_empty();
        let has_orchard_ivk = !decrypt_keys.orchard_ivks.is_empty();
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
            sapling_ivks: &decrypt_keys.sapling_ivks,
            sapling_key_ids: &decrypt_keys.sapling_key_ids,
            sapling_scopes: &decrypt_keys.sapling_scopes,
            orchard_ivks: &decrypt_keys.orchard_ivks,
            orchard_key_ids: &decrypt_keys.orchard_key_ids,
            orchard_scopes: &decrypt_keys.orchard_scopes,
            orchard_fvks: &decrypt_keys.orchard_fvks,
            decrypt_pool: self.decrypt_pool.as_ref(),
            max_parallel: self.config.max_parallel_decrypt,
            task_multiplier: 1,
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
                .filter(|note| note.note_type == crate::pipeline::NoteType::Ironwood)
                .count();
            let sapling_notes = all_notes
                .iter()
                .filter(|note| note.note_type == crate::pipeline::NoteType::Sapling)
                .count();
            append_debug_log_line(&format!(
                r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:trial_decrypt_batch","message":"trial_decrypt batch summary","data":{{"start":{},"end":{},"blocks":{},"sapling_outputs":{},"orchard_actions":{},"sapling_notes":{},"orchard_notes":{},"decrypt_pool_wall_ms":{},"decrypt_worker_active_ms":{},"decrypt_worker_tasks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"T"}}"#,
                id,
                ts,
                min_height.unwrap_or(0),
                max_height,
                blocks.len(),
                sapling_outputs_total,
                orchard_actions_total,
                sapling_notes,
                orchard_notes,
                decrypt_result.telemetry.pool_wall.as_millis(),
                decrypt_result.telemetry.worker_active.as_millis(),
                decrypt_result.telemetry.task_count,
            ));
        }

        Ok((all_notes, decrypt_result.telemetry))
    }

    #[cfg(test)]
    async fn trial_decrypt_batch(
        &self,
        blocks: &[CompactBlockData],
    ) -> Result<(Vec<DecryptedNote>, TrialDecryptTelemetry)> {
        self.trial_decrypt_batch_sync(blocks)
    }

    #[allow(clippy::too_many_arguments)]
    async fn produce_prefetched_batches(
        client: LightClient,
        start: u64,
        end: u64,
        cancel: CancelToken,
        wallet_id: Option<String>,
        validated_cache_range: Option<ValidatedCacheRange>,
        max_chunk_bytes: u64,
        flow: Arc<PrefetchFlowControl>,
        sender: &mpsc::Sender<Result<FetchedBlockBatch>>,
    ) -> Result<()> {
        if start > end {
            return Ok(());
        }
        let max_chunk_bytes = max_chunk_bytes.max(1);
        let mut current = start;

        while current <= end {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }

            if let Ok(cache) = BlockCache::for_endpoint(client.endpoint()) {
                let target_blocks = flow.target_blocks.load(Ordering::Acquire).max(1);
                let target_bytes = flow
                    .target_bytes
                    .load(Ordering::Acquire)
                    .clamp(1, max_chunk_bytes);
                let target_work_items = flow.cached_target_work_items.max(1);
                let local_end = end.min(current.saturating_add(target_blocks.saturating_sub(1)));
                match cache.load_bounded_work_range_for_upgrade(
                    current,
                    local_end,
                    target_bytes,
                    target_work_items,
                    flow.sapling_work_factor,
                    flow.ironwood_work_factor,
                ) {
                    Ok(mut cached) if !cached.blocks.is_empty() => {
                        let chunk_end =
                            cached
                                .blocks
                                .last()
                                .map(|block| block.height)
                                .ok_or_else(|| {
                                    Error::Sync("bounded cache chunk had no end height".to_string())
                                })?;
                        if Self::cached_blocks_are_canonical(
                            &client,
                            &cache,
                            current,
                            chunk_end,
                            &cached.blocks,
                            validated_cache_range,
                        )
                        .await?
                        {
                            cached.legacy_heights.retain(|height| *height <= chunk_end);
                            if !cached.legacy_heights.is_empty() {
                                cache
                                    .upgrade_legacy_rows(&cached.blocks, &cached.legacy_heights)?;
                            }
                            let reservation = flow
                                .watermarks
                                .reserve(cached.encoded_bytes, &cancel)
                                .await?;
                            let batch = FetchedBlockBatch {
                                blocks: cached.blocks,
                                encoded_bytes: cached.encoded_bytes,
                                shielded_work_items: cached.shielded_work_items,
                                requested_blocks: target_blocks,
                                requested_bytes: target_bytes,
                                requested_work_items: target_work_items,
                                source: BlockFetchSource::Cache,
                                elapsed: Duration::ZERO,
                                network_elapsed: Duration::ZERO,
                                cache_write_elapsed: Duration::ZERO,
                                spool_reservations: vec![reservation],
                            };
                            Self::send_prefetched_batch(sender, batch, &cancel).await?;
                            current = chunk_end.saturating_add(1);
                            continue;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => tracing::debug!(
                        "Bounded block cache read failed for {}-{}: {}",
                        current,
                        end,
                        error
                    ),
                }
            }

            match acquire_inflight(client.endpoint(), current, end) {
                InflightLease::Follower(waiter) => {
                    tokio::select! {
                        _ = waiter.wait() => {}
                        _ = cancel.cancelled() => return Err(Error::Cancelled),
                    }
                    continue;
                }
                InflightLease::Leader(token) => {
                    let max_segment_bytes = max_chunk_bytes
                        .min(flow.watermarks.segment_admission_bytes())
                        .max(1);
                    let (segment_sender, mut segment_receiver) =
                        mpsc::channel(DURABLE_SEGMENT_CHANNEL_CAPACITY);
                    let segment_handle = tokio::spawn(Self::produce_durable_network_segments(
                        client.clone(),
                        current,
                        end,
                        cancel.clone(),
                        wallet_id.clone(),
                        max_segment_bytes,
                        Arc::clone(&flow),
                        token,
                        segment_sender,
                    ));
                    let _segment_abort = AbortTaskOnDrop(segment_handle.abort_handle());
                    let mut pending = NetworkBatchAccumulator::default();
                    while let Some(segment) = tokio::select! {
                        segment = segment_receiver.recv() => segment,
                        _ = cancel.cancelled() => return Err(Error::Cancelled),
                    } {
                        let DurableBlockSegment {
                            blocks: mut segment_blocks,
                            encoded_block_bytes: mut segment_block_bytes,
                            encoded_bytes: segment_bytes,
                            network_elapsed,
                            cache_write_elapsed,
                            reservation,
                        } = segment;
                        let segment_start = segment_blocks
                            .first()
                            .map(|block| block.height)
                            .ok_or_else(|| {
                                Error::Sync("durable compact block segment was empty".to_string())
                            })?;
                        let segment_end = segment_blocks
                            .last()
                            .map(|block| block.height)
                            .ok_or_else(|| {
                                Error::Sync("durable compact block segment was empty".to_string())
                            })?;
                        if segment_start != current {
                            return Err(Error::Sync(format!(
                                "durable compact block router expected {}, received {}",
                                current, segment_start
                            )));
                        }
                        debug_assert_eq!(
                            segment_block_bytes.iter().copied().sum::<u64>(),
                            segment_bytes
                        );
                        let mut segment_block_work = block_shielded_work_items(
                            &segment_blocks,
                            flow.sapling_work_factor,
                            flow.ironwood_work_factor,
                        );
                        let mut attribute_network = Some(network_elapsed);
                        let mut attribute_cache_write = Some(cache_write_elapsed);
                        while !segment_blocks.is_empty() {
                            let target = (
                                flow.target_blocks.load(Ordering::Acquire).max(1),
                                flow.target_bytes
                                    .load(Ordering::Acquire)
                                    .clamp(1, max_chunk_bytes),
                                flow.target_work_items.load(Ordering::Acquire).max(1),
                            );
                            let take = local_batch_prefix_len(
                                &segment_block_bytes,
                                &segment_block_work,
                                LocalBatchWeight {
                                    blocks: pending.blocks.len() as u64,
                                    encoded_bytes: pending.encoded_bytes,
                                    shielded_work_items: pending.shielded_work_items,
                                },
                                LocalBatchWeight {
                                    blocks: target.0,
                                    encoded_bytes: target.1,
                                    shielded_work_items: target.2,
                                },
                            );
                            if take == 0 {
                                let batch = pending.take_batch(target.0, target.1, target.2);
                                Self::send_prefetched_batch(sender, batch, &cancel).await?;
                                continue;
                            }

                            let remaining_blocks = segment_blocks.split_off(take);
                            let piece_blocks =
                                std::mem::replace(&mut segment_blocks, remaining_blocks);
                            let remaining_sizes = segment_block_bytes.split_off(take);
                            let piece_sizes =
                                std::mem::replace(&mut segment_block_bytes, remaining_sizes);
                            let remaining_work = segment_block_work.split_off(take);
                            let piece_work =
                                std::mem::replace(&mut segment_block_work, remaining_work);
                            let piece_bytes = piece_sizes.iter().copied().sum::<u64>();
                            pending.push(
                                piece_blocks,
                                piece_bytes,
                                piece_work.iter().copied().sum(),
                                attribute_network.take().unwrap_or_default(),
                                attribute_cache_write.take().unwrap_or_default(),
                                reservation.clone(),
                            );
                            if pending.reached(target) {
                                let batch = pending.take_batch(target.0, target.1, target.2);
                                Self::send_prefetched_batch(sender, batch, &cancel).await?;
                            }
                        }
                        current = segment_end.saturating_add(1);
                    }
                    if !pending.is_empty() {
                        let batch = pending.take_batch(
                            flow.target_blocks.load(Ordering::Acquire).max(1),
                            flow.target_bytes
                                .load(Ordering::Acquire)
                                .clamp(1, max_chunk_bytes),
                            flow.target_work_items.load(Ordering::Acquire).max(1),
                        );
                        Self::send_prefetched_batch(sender, batch, &cancel).await?;
                    }
                    segment_handle.await.map_err(|error| {
                        Error::Sync(format!("durable block spool task failed: {}", error))
                    })??;
                    if current <= end {
                        return Err(Error::Network(format!(
                            "durable compact block router ended at {}, expected {}",
                            current,
                            end.saturating_add(1)
                        )));
                    }
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn produce_durable_network_segments(
        client: LightClient,
        start: u64,
        end: u64,
        cancel: CancelToken,
        wallet_id: Option<String>,
        max_segment_bytes: u64,
        flow: Arc<PrefetchFlowControl>,
        token: InflightToken,
        sender: mpsc::Sender<DurableBlockSegment>,
    ) -> Result<()> {
        let mut current = start;
        let mut segment_controller = AdaptiveDurableSegmentController::new(max_segment_bytes);
        flow.durable_segment_blocks
            .store(segment_controller.target_blocks(), Ordering::Release);
        let mut receiver = client.compact_block_adaptive_segment_stream(
            height_to_u32(start)?..height_to_u32(end.saturating_add(1))?,
            max_segment_bytes,
            Arc::clone(&flow.durable_segment_blocks),
            1,
            wallet_id,
        );

        while current <= end {
            let network_wait_start = Instant::now();
            let received = tokio::select! {
                chunk = receiver.recv() => chunk,
                _ = cancel.cancelled() => return Err(Error::Cancelled),
            };
            let network_elapsed = network_wait_start.elapsed();
            let Some(received) = received else {
                break;
            };
            let chunk = received?;
            if chunk.blocks.len() != chunk.encoded_block_bytes.len()
                || chunk.encoded_block_bytes.iter().copied().sum::<u64>() != chunk.encoded_bytes
            {
                return Err(Error::Sync(
                    "compact block segment byte accounting mismatch".to_string(),
                ));
            }
            let chunk_start = chunk.start_height().ok_or_else(|| {
                Error::Sync("network compact block chunk had no start".to_string())
            })?;
            let chunk_end = chunk
                .end_height()
                .ok_or_else(|| Error::Sync("network compact block chunk had no end".to_string()))?;
            if chunk_start != current {
                return Err(Error::Sync(format!(
                    "network compact block chunk expected {}, received {}",
                    current, chunk_start
                )));
            }
            Self::validate_compact_block_range(chunk_start, chunk_end, &chunk.blocks)?;

            let cache_write_start = Instant::now();
            let endpoint = client.endpoint().to_string();
            let encoded_bytes = chunk.encoded_bytes;
            let encoded_block_bytes = chunk.encoded_block_bytes;
            let chunk_blocks = chunk.blocks;
            let (blocks, cache_result) = tokio::task::spawn_blocking(move || {
                let result = BlockCache::for_endpoint(&endpoint)
                    .and_then(|cache| cache.store_blocks(&chunk_blocks));
                (chunk_blocks, result)
            })
            .await
            .map_err(|error| {
                Error::Sync(format!(
                    "compact block cache worker failed for {}-{}: {}",
                    chunk_start, chunk_end, error
                ))
            })?;
            let cache_write_elapsed = cache_write_start.elapsed();
            if let Err(error) = cache_result {
                tracing::warn!(
                    "Durable block spool store failed for {}-{}; continuing with the bounded in-memory segment: {}",
                    chunk_start,
                    chunk_end,
                    error
                );
            }

            let reservation = flow
                .watermarks
                .reserve_durable_segment(encoded_bytes, &cancel)
                .await?;
            let previous_segment_blocks = segment_controller.target_blocks();
            let next_segment_blocks = segment_controller.observe(DurableSegmentObservation {
                blocks: blocks.len() as u64,
                encoded_bytes,
                network_wait: network_elapsed,
                cache_write: cache_write_elapsed,
                queued_bytes: flow.watermarks.queued_bytes(),
                high_water_bytes: flow.watermarks.high_bytes(),
                stream_tail: chunk_end == end,
            });
            flow.durable_segment_blocks
                .store(next_segment_blocks, Ordering::Release);
            if next_segment_blocks != previous_segment_blocks {
                tracing::debug!(
                    previous_segment_blocks,
                    next_segment_blocks,
                    network_wait_ms = network_elapsed.as_millis(),
                    cache_write_ms = cache_write_elapsed.as_millis(),
                    queued_bytes = flow.watermarks.queued_bytes(),
                    "Adjusted durable compact-block segment ceiling"
                );
            }
            let segment = DurableBlockSegment {
                blocks,
                encoded_block_bytes,
                encoded_bytes,
                network_elapsed,
                cache_write_elapsed,
                reservation,
            };
            tokio::select! {
                result = sender.send(segment) => result.map_err(|_| Error::Cancelled)?,
                _ = cancel.cancelled() => return Err(Error::Cancelled),
            }
            current = chunk_end.saturating_add(1);
        }

        if current <= end {
            return Err(Error::Network(format!(
                "durable compact block spool ended at {}, expected {}",
                current,
                end.saturating_add(1)
            )));
        }
        token.complete();
        Ok(())
    }

    async fn send_prefetched_batch(
        sender: &mpsc::Sender<Result<FetchedBlockBatch>>,
        batch: FetchedBlockBatch,
        cancel: &CancelToken,
    ) -> Result<()> {
        tokio::select! {
            result = sender.send(Ok(batch)) => result.map_err(|_| Error::Cancelled),
            _ = cancel.cancelled() => Err(Error::Cancelled),
        }
    }

    fn spawn_prefetch(
        &self,
        start: u64,
        end: u64,
        validated_cache_range: Option<ValidatedCacheRange>,
        flow: Arc<PrefetchFlowControl>,
    ) -> PrefetchTask {
        let client = self.client.clone();
        let cancel = self.cancel.clone();
        let wallet_id = self.wallet_id.clone();
        let max_chunk_bytes = prefetched_batch_encoded_byte_cap(&self.config);
        let (sender, receiver) = mpsc::channel(LOCAL_SCAN_BATCH_CHANNEL_CAPACITY);
        let handle = tokio::spawn(async move {
            if let Err(error) = SyncEngine::produce_prefetched_batches(
                client,
                start,
                end,
                cancel,
                wallet_id,
                validated_cache_range,
                max_chunk_bytes,
                flow,
                &sender,
            )
            .await
            {
                let _ = sender.send(Err(error)).await;
            }
        });
        PrefetchTask {
            start,
            end,
            payload: Some(PrefetchPayload::Fetch { receiver, handle }),
        }
    }

    fn abort_prefetch_task(task: &mut PrefetchTask) {
        match task.payload.take() {
            None => {}
            Some(PrefetchPayload::Fetch { handle, .. }) => handle.abort(),
            Some(PrefetchPayload::Decrypt {
                handle,
                producer_abort,
            }) => {
                handle.abort();
                producer_abort.abort();
            }
        }
    }

    async fn receive_prefetch_batch(
        &self,
        task: &mut PrefetchTask,
    ) -> Result<ReceivedPrefetchBatch> {
        let payload = task.payload.take().ok_or_else(|| {
            Error::Sync(format!(
                "prefetch task {}-{} had no active payload",
                task.start, task.end
            ))
        })?;
        match payload {
            PrefetchPayload::Fetch {
                mut receiver,
                handle,
            } => {
                let received = tokio::select! {
                    received = receiver.recv() => received,
                    _ = self.cancel.cancelled() => {
                        handle.abort();
                        return Err(Error::Cancelled);
                    }
                };
                task.payload = Some(PrefetchPayload::Fetch { receiver, handle });
                let fetched = received.unwrap_or_else(|| {
                    Err(Error::Sync(format!(
                        "bounded compact block producer ended before {}-{} completed",
                        task.start, task.end
                    )))
                })?;
                Ok(ReceivedPrefetchBatch {
                    fetched,
                    prepared_notes: None,
                    prepared_commitments: None,
                })
            }
            PrefetchPayload::Decrypt {
                mut handle,
                producer_abort,
            } => {
                // The next batch has already crossed the network and durable
                // cache boundary. Report the work actually blocking progress.
                self.progress.write().await.set_stage(SyncStage::Notes);
                let output = tokio::select! {
                    joined = &mut handle => joined.map_err(|error| {
                        Error::Sync(format!("trial-decrypt lookahead task failed: {}", error))
                    })?,
                    _ = self.cancel.cancelled() => {
                        handle.abort();
                        producer_abort.abort();
                        return Err(Error::Cancelled);
                    }
                }?;
                task.payload = Some(PrefetchPayload::Fetch {
                    receiver: output.receiver,
                    handle: output.producer_handle,
                });
                Ok(ReceivedPrefetchBatch {
                    fetched: output.fetched,
                    prepared_notes: Some((output.notes, output.telemetry)),
                    prepared_commitments: Some(output.prepared_commitments),
                })
            }
        }
    }

    fn start_one_batch_decrypt_lookahead(&self, queue: &mut VecDeque<PrefetchTask>) {
        if !self.config.one_batch_ahead_decryption {
            return;
        }
        let Some(mut task) = queue.pop_front() else {
            return;
        };
        let Some(PrefetchPayload::Fetch {
            mut receiver,
            handle: producer_handle,
        }) = task.payload.take()
        else {
            queue.push_front(task);
            return;
        };

        let producer_abort = producer_handle.abort_handle();
        let cancel = self.cancel.clone();
        let decrypt_keys = self.trial_decrypt_keys.clone();
        let decrypt_pool = Arc::clone(&self.decrypt_pool);
        let birthday_height = self.birthday_height;
        let max_parallel = self.config.max_parallel_decrypt;
        let task_multiplier = if self.config.stage_aware_cpu_scheduling {
            LOOKAHEAD_DECRYPT_TASK_MULTIPLIER
        } else {
            1
        };
        let planned_start = task.start;
        let planned_end = task.end;
        let lookahead = tokio::spawn(async move {
            let fetched = tokio::select! {
                received = receiver.recv() => {
                    received.unwrap_or_else(|| {
                        Err(Error::Sync(format!(
                            "bounded compact block producer ended before lookahead {}-{} completed",
                            planned_start, planned_end
                        )))
                    })?
                }
                _ = cancel.cancelled() => {
                    producer_handle.abort();
                    return Err(Error::Cancelled);
                }
            };
            let fetched_start = fetched
                .blocks
                .first()
                .map(|block| block.height)
                .unwrap_or(planned_start);
            let fetched_end = fetched
                .blocks
                .last()
                .map(|block| block.height)
                .unwrap_or(planned_start);
            tracing::debug!(
                start = fetched_start,
                end = fetched_end,
                blocks = fetched.blocks.len(),
                encoded_bytes = fetched.encoded_bytes,
                shielded_work_items = fetched.shielded_work_items,
                "starting one-batch-ahead immutable work"
            );
            if verbose_sync_batch_logging_enabled() {
                append_sync_decision_log(
                    "sync.rs:decrypt_lookahead",
                    "starting one-batch-ahead immutable work",
                    format!(
                        "\"start\":{},\"end\":{},\"blocks\":{},\"encoded_bytes\":{},\"shielded_work_items\":{}",
                        fetched_start,
                        fetched_end,
                        fetched.blocks.len(),
                        fetched.encoded_bytes,
                        fetched.shielded_work_items,
                    ),
                );
            }

            let decrypt_cancel = cancel.clone();
            let (fetched, result, prepared_commitments) = tokio::task::spawn_blocking(move || {
                let relevant = wallet_relevant_blocks(&fetched.blocks, birthday_height);
                let (result, prepared_commitments) = decrypt_pool.install(|| {
                    rayon::join(
                        || {
                            if decrypt_keys.sapling_ivks.is_empty()
                                && decrypt_keys.orchard_ivks.is_empty()
                            {
                                Ok(TrialDecryptBatchResult {
                                    notes: Vec::new(),
                                    telemetry: TrialDecryptTelemetry::default(),
                                })
                            } else {
                                trial_decrypt_batch_impl(TrialDecryptBatchInputs {
                                    blocks: relevant,
                                    sapling_ivks: &decrypt_keys.sapling_ivks,
                                    sapling_key_ids: &decrypt_keys.sapling_key_ids,
                                    sapling_scopes: &decrypt_keys.sapling_scopes,
                                    orchard_ivks: &decrypt_keys.orchard_ivks,
                                    orchard_key_ids: &decrypt_keys.orchard_key_ids,
                                    orchard_scopes: &decrypt_keys.orchard_scopes,
                                    orchard_fvks: &decrypt_keys.orchard_fvks,
                                    decrypt_pool: decrypt_pool.as_ref(),
                                    max_parallel,
                                    task_multiplier,
                                })
                            }
                        },
                        || prepare_commitment_batch(&fetched.blocks),
                    )
                });
                (fetched, result, prepared_commitments)
            })
            .await
            .map_err(|error| {
                Error::Sync(format!("trial-decrypt lookahead worker failed: {}", error))
            })?;
            if decrypt_cancel.is_cancelled() {
                producer_handle.abort();
                return Err(Error::Cancelled);
            }
            let decrypted = result?;
            Ok(DecryptLookaheadOutput {
                fetched,
                notes: decrypted.notes,
                telemetry: decrypted.telemetry,
                prepared_commitments,
                receiver,
                producer_handle,
            })
        });
        task.payload = Some(PrefetchPayload::Decrypt {
            handle: lookahead,
            producer_abort,
        });
        queue.push_front(task);
    }

    #[cfg(test)]
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

    #[cfg(test)]
    async fn resolve_server_batch_hint_task(
        &self,
        mut task: ServerBatchHintTask,
    ) -> Result<(Option<u64>, Option<ServerBatchHintTask>)> {
        if !task.handle.is_finished() {
            return Ok((None, Some(task)));
        }
        tokio::select! {
            joined = &mut task.handle => {
                let value = joined
                    .map_err(|e| Error::Sync(format!("server batch hint task failed: {}", e)))?;
                Ok((value.filter(|end| *end >= task.start), None))
            }
            _ = self.cancel.cancelled() => {
                task.handle.abort();
                Err(Error::Cancelled)
            }
        }
    }

    fn abort_prefetch_queue(prefetch_queue: &mut VecDeque<PrefetchTask>) {
        while let Some(prefetch) = prefetch_queue.pop_front() {
            let mut prefetch = prefetch;
            Self::abort_prefetch_task(&mut prefetch);
        }
    }

    fn fill_prefetch_queue(
        &self,
        prefetch_queue: &mut VecDeque<PrefetchTask>,
        sync_bounds: (u64, u64),
        validated_cache_range: Option<ValidatedCacheRange>,
        flow: Arc<PrefetchFlowControl>,
    ) {
        let (start_height, end_height) = sync_bounds;

        if start_height > end_height || !prefetch_queue.is_empty() {
            return;
        }

        // One producer owns the whole bounded sync horizon. Its gRPC request is
        // therefore independent of device speed and local scan-batch decisions.
        // Tree-replay boundaries still split the horizon where correctness
        // requires an intermediate checkpoint.
        prefetch_queue.push_back(self.spawn_prefetch(
            start_height,
            end_height,
            validated_cache_range,
            flow,
        ));
    }

    #[cfg(test)]
    async fn compute_batch_end(
        &self,
        current_height: u64,
        end: u64,
        batch_tuning: BatchTuning,
        server_group_end_hint: &mut Option<u64>,
        pending_server_group_hint: &mut Option<ServerBatchHintTask>,
        allow_server_recommendation: bool,
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
        let mut max_batch_blocks = batch_tuning
            .max_batch_blocks
            .max(self.config.min_batch_size);
        let mut min_batch_blocks = self.config.min_batch_size.max(1).min(max_batch_blocks);
        if let Some(max_memory) = self.config.max_batch_memory_bytes {
            let memory_safe_blocks = (max_memory / estimated_block_bytes).max(1);
            max_batch_blocks = max_batch_blocks.min(memory_safe_blocks);
            min_batch_blocks = min_batch_blocks.min(max_batch_blocks);
        }
        desired_blocks = desired_blocks.clamp(min_batch_blocks, max_batch_blocks);

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

        if !self.config.use_server_batch_recommendations || !allow_server_recommendation {
            return Ok((desired_end, desired_blocks));
        }

        let server_end = match *server_group_end_hint {
            Some(cached_end) if cached_end >= current_height => cached_end,
            _ => {
                if let Some(task) = pending_server_group_hint.take() {
                    if task.start == current_height {
                        match self.resolve_server_batch_hint_task(task).await? {
                            (Some(value), None) => {
                                *server_group_end_hint = Some(value);
                                value
                            }
                            (None, Some(task)) => {
                                *pending_server_group_hint = Some(task);
                                return Ok((desired_end, desired_blocks));
                            }
                            (None, None) => {
                                *server_group_end_hint = None;
                                *pending_server_group_hint =
                                    Some(self.spawn_server_batch_hint_prefetch(current_height));
                                return Ok((desired_end, desired_blocks));
                            }
                            (Some(_), Some(_)) => {
                                unreachable!(
                                    "server batch hint resolver cannot return both a value and a pending task"
                                )
                            }
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
                    let task = self.spawn_server_batch_hint_prefetch(current_height);
                    *pending_server_group_hint = Some(task);
                    return Ok((desired_end, desired_blocks));
                }
            }
        };

        match server_end {
            server_end if server_end >= current_height => {
                let optimal_end = std::cmp::min(server_end, end);
                let server_batch_size =
                    optimal_end.saturating_sub(current_height).saturating_add(1);
                let server_group_multiplier = target_bytes
                    .div_ceil(SERVER_BATCH_GROUP_TARGET_BYTES)
                    .max(1);
                let server_profile_cap_blocks = server_batch_size
                    .saturating_mul(server_group_multiplier)
                    .max(server_batch_size)
                    .min(max_batch_blocks);
                let max_capped_end = std::cmp::min(
                    end,
                    current_height.saturating_add(server_profile_cap_blocks.saturating_sub(1)),
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
                            r#"{{"id":"log_{}","timestamp":{},"location":"sync.rs:compute_batch_end","message":"server batch recommendation","data":{{"server_batch_size":{},"server_group_multiplier":{},"desired_blocks":{},"max_batch_size":{},"chosen_blocks":{}}},"sessionId":"debug-session","runId":"run1","hypothesisId":"G"}}"#,
                            id,
                            ts,
                            server_batch_size,
                            server_group_multiplier,
                            desired_blocks,
                            max_batch_blocks,
                            batch_end - current_height + 1
                        );
                    });
                    // #endregion
                    tracing::debug!(
                        "Batch sizing: server {} blocks x{} groups, desired {} blocks, chosen {} blocks",
                        server_batch_size,
                        server_group_multiplier,
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

    fn spawn_background_enrich(
        &self,
        notes: Vec<DecryptedNote>,
        require_memos: bool,
        persistence_worker: Option<Arc<PersistenceWorker>>,
    ) {
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
                persistence_worker,
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
        persistence_worker: Option<Arc<PersistenceWorker>>,
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
            persistence_worker,
        )
        .await
    }

    /// Fetch full transactions to enrich notes (memos, Orchard nullifiers, outgoing memo recovery).
    #[allow(clippy::too_many_arguments)]
    async fn fetch_and_enrich_notes_with_context(
        client: LightClient,
        sink: StorageSink,
        wallet_id: Option<String>,
        keys: Vec<WalletKeyGroup>,
        max_parallel: usize,
        notes: &mut [DecryptedNote],
        require_memos: bool,
        persistence_worker: Option<Arc<PersistenceWorker>>,
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
                key_type: KeyType::ImportView,
                seed_derivation_index: None,
                discovery_candidate: false,
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
                    if let Ok(fvk) = IronwoodExtendedFullViewingKey::from_bytes(&ivk) {
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
            .filter(|note| note.note_type == NoteType::Ironwood)
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
                note.note_type == NoteType::Ironwood && note.nullifier.iter().all(|b| *b == 0);
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
                    NoteType::Ironwood => orchard_needs_tx += 1,
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
                                    if let Err(e) = Self::persist_enriched_note_memo(
                                        persistence_worker.as_ref(),
                                        &sink,
                                        &note.tx_hash,
                                        note.output_index as i64,
                                        NoteType::Sapling,
                                        memo,
                                    )
                                    .await
                                    {
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
                    NoteType::Ironwood => {
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
                                    if let Err(e) = Self::persist_enriched_note_memo(
                                        persistence_worker.as_ref(),
                                        &sink,
                                        &note.tx_hash,
                                        note.output_index as i64,
                                        NoteType::Ironwood,
                                        memo,
                                    )
                                    .await
                                    {
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
                match Self::recover_outgoing_memo(
                    &raw_tx_bytes,
                    work.block.unwrap_or(0),
                    sapling_ovk,
                    orchard_ovk,
                ) {
                    Ok(Some(memo)) => {
                        if let Err(error) = Self::persist_outgoing_memo(
                            persistence_worker.as_ref(),
                            &sink,
                            txid_hex,
                            memo,
                        )
                        .await
                        {
                            tracing::warn!("Outgoing memo persistence failed: {}", error);
                        }
                    }
                    Ok(None) => {}
                    Err(error) => tracing::warn!("Outgoing memo recovery failed: {}", error),
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
                    if let Ok(fvk) = IronwoodExtendedFullViewingKey::from_bytes(&ivk) {
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

    async fn persist_enriched_note_memo(
        worker: Option<&Arc<PersistenceWorker>>,
        sink: &StorageSink,
        txid: &[u8],
        output_index: i64,
        note_type: NoteType,
        memo: Vec<u8>,
    ) -> Result<()> {
        if let Some(worker) = worker {
            let sink = sink.clone();
            let txid = txid.to_vec();
            worker
                .execute(move |db| {
                    sink.update_note_memo_with_db(db, &txid, output_index, note_type, Some(&memo))
                })
                .await
        } else {
            sink.update_note_memo(txid, output_index, note_type, Some(&memo))
        }
    }

    async fn persist_outgoing_memo(
        worker: Option<&Arc<PersistenceWorker>>,
        sink: &StorageSink,
        txid_hex: String,
        memo: Vec<u8>,
    ) -> Result<()> {
        if let Some(worker) = worker {
            let sink = sink.clone();
            worker
                .execute(move |db| sink.upsert_tx_memo_with_db(db, &txid_hex, &memo))
                .await
        } else {
            sink.upsert_tx_memo(&txid_hex, &memo)
        }
    }

    fn recover_outgoing_memo(
        raw_tx_bytes: &[u8],
        height: u64,
        sapling_ovk: Option<&SaplingOutgoingViewingKey>,
        orchard_ovk: Option<&orchard::keys::OutgoingViewingKey>,
    ) -> Result<Option<Vec<u8>>> {
        if sapling_ovk.is_none() && orchard_ovk.is_none() {
            return Ok(None);
        }

        let tx = read_pirate_transaction(raw_tx_bytes)
            .map_err(|e| Error::Sync(format!("Failed to parse transaction: {}", e)))?;

        let mut memo_to_store: Option<Vec<u8>> = None;

        if let Some(ovk) = sapling_ovk {
            if let Some(bundle) = tx.sapling_bundle() {
                let zip212 = zip212_enforcement(
                    &PirateNetwork::default(),
                    BlockHeight::from_u32(height as u32),
                );
                for output in bundle.shielded_outputs() {
                    if let Some((_note, _address, memo)) =
                        try_sapling_output_recovery(ovk, output, zip212)
                    {
                        if !memo.iter().all(|b| *b == 0) {
                            memo_to_store = Some(memo.to_vec());
                            break;
                        }
                    }
                }
            }
        }

        if memo_to_store.is_none() {
            if let Some(ovk) = orchard_ovk {
                if let Some(bundle) = tx.ironwood_bundle() {
                    for action in bundle.actions() {
                        let domain = IronwoodDomain::for_action(action);
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

        Ok(memo_to_store)
    }

    /// Process block commitments into ShardTree batches and track positions.
    ///
    /// ShardTree is the single source of truth for commitment tree state.
    /// Positions are tracked via simple counters (initialized from frontier at sync start).
    #[allow(clippy::too_many_arguments)]
    async fn update_commitment_trees(
        &self,
        blocks: &[CompactBlockData],
        prepared_commitments: PreparedCommitmentBatch,
        notes: &[DecryptedNote],
        checkpoint_mode: FrontierCheckpointMode,
        warm_trees: Option<&mut SyncWarmTrees<'_>>,
        historical_prefill_state: Option<&mut HistoricalPrefillState>,
        run_db: Option<&Database>,
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<(u64, PositionMaps, bool, ShardtreePersistResult)> {
        prepared_commitments.validate_source(blocks)?;
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
            .filter(|n| n.note_type == crate::pipeline::NoteType::Ironwood)
            .map(|n| n.commitment)
            .collect();
        let has_owned_sapling = !sapling_owned.is_empty();
        let has_owned_orchard = !orchard_owned.is_empty();
        let mut shardtree_batches: Vec<ShardtreeBatch> =
            Vec::with_capacity(prepared_commitments.blocks.len());
        let batch_end_height = prepared_commitments.blocks.last().map(|block| block.height);
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

        for block in prepared_commitments.blocks {
            let mut shardtree_batch = ShardtreeBatch::new(block.height);
            {
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

                for tx in block.transactions {
                    for output in tx.sapling {
                        let pos = *sapling_pos_ref;
                        *sapling_pos_ref = sapling_pos_ref.saturating_add(1);
                        let is_owned =
                            has_owned_sapling && sapling_owned.contains(&output.commitment);
                        if is_owned {
                            if let Some(key) = TxOutputKey::new(&tx.hash, output.output_index) {
                                position_mappings_ref.sapling_by_tx.insert(key, pos);
                            }
                        }
                        if let Some(node) = output.node {
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
                            )?;
                            *count_ref += 1;
                        }
                    }

                    for action in tx.ironwood {
                        let pos = *orchard_pos_ref;
                        *orchard_pos_ref = orchard_pos_ref.saturating_add(1);
                        let is_owned =
                            has_owned_orchard && orchard_owned.contains(&action.commitment);
                        if is_owned {
                            position_mappings_ref
                                .orchard_by_commitment
                                .insert(action.commitment, pos);
                        }
                        if let Some(node) = action.node {
                            let retention = if is_owned {
                                Retention::Marked
                            } else {
                                Retention::Ephemeral
                            };
                            process_historical_leaf(
                                orchard_skip_state.as_deref_mut(),
                                pos,
                                block.height,
                                node,
                                retention,
                                HistoricalLeafSink {
                                    current_batch: &mut shardtree_batch,
                                    shardtree_batches: &mut shardtree_batches,
                                },
                                append_orchard_leaf,
                            )?;
                            *count_ref += 1;
                        }
                    }
                }
            }

            let checkpoint_height = u32::try_from(block.height).map_err(|_| {
                Error::Sync(format!(
                    "Checkpoint height {} exceeds u32::MAX",
                    block.height
                ))
            })?;
            let checkpoint_id = BlockHeight::from(checkpoint_height);
            let sapling_wallet_mark = shardtree_batch
                .sapling
                .iter()
                .any(|(_, retention)| retention.is_marked());
            let orchard_wallet_mark = shardtree_batch
                .orchard
                .iter()
                .any(|(_, retention)| retention.is_marked());
            let (sapling_checkpoint_safe, orchard_checkpoint_safe) = historical_prefill_state
                .as_deref()
                .map(|state| {
                    (
                        state.sapling_checkpoint_safe(),
                        state.orchard_checkpoint_safe(),
                    )
                })
                .unwrap_or((true, true));
            let sapling_should_checkpoint = sapling_checkpoint_safe
                && match checkpoint_mode {
                    FrontierCheckpointMode::PerBlock => true,
                    FrontierCheckpointMode::OwnedOnly => sapling_wallet_mark,
                };
            let orchard_should_checkpoint = orchard_checkpoint_safe
                && match checkpoint_mode {
                    FrontierCheckpointMode::PerBlock => true,
                    FrontierCheckpointMode::OwnedOnly => orchard_wallet_mark,
                };
            shardtree_batch.checkpoint_id =
                (sapling_should_checkpoint || orchard_should_checkpoint).then_some(checkpoint_id);
            if sapling_should_checkpoint {
                if let Some((_, retention)) = shardtree_batch.sapling.last_mut() {
                    *retention = Retention::Checkpoint {
                        id: checkpoint_id,
                        marking: if retention.is_marked() {
                            Marking::Marked
                        } else {
                            Marking::None
                        },
                    };
                } else {
                    shardtree_batch.sapling_empty_checkpoint = true;
                }
            }
            if orchard_should_checkpoint {
                if let Some((_, retention)) = shardtree_batch.orchard.last_mut() {
                    *retention = Retention::Checkpoint {
                        id: checkpoint_id,
                        marking: if retention.is_marked() {
                            Marking::Marked
                        } else {
                            Marking::None
                        },
                    };
                } else {
                    shardtree_batch.orchard_empty_checkpoint = true;
                }
            }

            shardtree_batches.push(shardtree_batch);
        }

        let verified_roots = historical_prefill_state
            .as_ref()
            .map(|state| state.pending_verified_roots())
            .unwrap_or_default();
        let persist_result = if let Some(trees) = warm_trees {
            trees.persist_batches_with_roots(
                &shardtree_batches,
                batch_end_height,
                &verified_roots,
            )?
        } else if let Some(worker) = persistence_worker {
            worker
                .persist_shardtree_batches_with_roots(
                    shardtree_batches,
                    batch_end_height,
                    verified_roots.clone(),
                )
                .await?
        } else {
            Self::persist_shardtree_batches_with_roots_for_storage(
                self.storage.as_ref(),
                &shardtree_batches,
                batch_end_height,
                run_db,
                &verified_roots,
            )?
        };
        if !verified_roots.is_empty() {
            if let Some(state) = historical_prefill_state.take() {
                state.mark_verified_roots_persisted(&verified_roots);
            }
            let grafted = verified_roots.counts();
            tracing::info!(
                "Grafted sampled historical subtree roots atomically: sapling={}, ironwood={}",
                grafted.0,
                grafted.1
            );
        }
        Ok((
            count,
            position_mappings,
            persist_result.batch_end_checkpointed,
            persist_result,
        ))
    }

    async fn flush_historical_prefill_buffers(
        &self,
        historical_prefill_state: &mut Option<HistoricalPrefillState>,
        warm_trees: &mut Option<SyncWarmTrees<'_>>,
        run_db: Option<&Database>,
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<()> {
        let Some(state) = historical_prefill_state.as_mut() else {
            return Ok(());
        };
        let mut pending_batches = Vec::new();
        merge_emitted_batches(
            &mut pending_batches,
            drain_historical_skip_state(&mut state.sapling, append_sapling_leaf),
        );
        merge_emitted_batches(
            &mut pending_batches,
            drain_historical_skip_state(&mut state.orchard, append_orchard_leaf),
        );
        if pending_batches.is_empty() {
            return Ok(());
        }

        if let Some(trees) = warm_trees.as_mut() {
            let _ = trees.persist_batches(&pending_batches, None)?;
        } else if let Some(worker) = persistence_worker {
            let _ = worker
                .persist_shardtree_batches(pending_batches, None)
                .await?;
        } else {
            let _ = Self::persist_shardtree_batches_for_storage(
                self.storage.as_ref(),
                &pending_batches,
                None,
                run_db,
            )?;
        }
        Ok(())
    }

    /// Persist commitment batches to the ShardTree (SQLite-backed).
    ///
    /// Uses upstream-style retained leaves and encodes per-block checkpoints at insert time.
    fn persist_shardtree_batches_for_storage(
        storage: Option<&StorageSink>,
        batches: &[ShardtreeBatch],
        batch_end_height: Option<u64>,
        run_db: Option<&Database>,
    ) -> Result<ShardtreePersistResult> {
        Self::persist_shardtree_batches_with_roots_for_storage(
            storage,
            batches,
            batch_end_height,
            run_db,
            &VerifiedSubtreeRoots::default(),
        )
    }

    fn persist_shardtree_batches_with_roots_for_storage(
        storage: Option<&StorageSink>,
        batches: &[ShardtreeBatch],
        batch_end_height: Option<u64>,
        run_db: Option<&Database>,
        verified_roots: &VerifiedSubtreeRoots,
    ) -> Result<ShardtreePersistResult> {
        if batches.is_empty() && verified_roots.is_empty() {
            return Ok(ShardtreePersistResult::default());
        }
        let Some(sink) = storage else {
            if !verified_roots.is_empty() {
                return Err(Error::Sync(
                    "Verified subtree roots require persistent storage".to_string(),
                ));
            }
            return Ok(ShardtreePersistResult::default());
        };

        let opened_db;
        let db = if let Some(db) = run_db {
            db
        } else {
            opened_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &opened_db
        };
        let tx = db
            .unchecked_immediate_transaction()
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
        let max_committed_heights = CommittedCheckpointHeights {
            sapling: max_existing_sapling_checkpoint,
            ironwood: max_existing_orchard_checkpoint,
        };

        let checkpoint_heavy = batches
            .iter()
            .filter(|batch| batch.checkpoint_id.is_some())
            .take(2)
            .count()
            > 1;
        let result = if checkpoint_heavy || !verified_roots.is_empty() {
            let sapling_preloads =
                sparse_preload_addresses::<_, SAPLING_SHARD_HEIGHT>(&sapling_store, "Sapling")?;
            let orchard_preloads =
                sparse_preload_addresses::<_, ORCHARD_SHARD_HEIGHT>(&orchard_store, "Orchard")?;
            let sapling_store =
                SparseCachingShardStore::with_preloaded(sapling_store, sapling_preloads).map_err(
                    |e| Error::Sync(format!("Failed to preload Sapling shard store: {}", e)),
                )?;
            let orchard_store =
                SparseCachingShardStore::with_preloaded(orchard_store, orchard_preloads).map_err(
                    |e| Error::Sync(format!("Failed to preload Orchard shard store: {}", e)),
                )?;
            let mut sapling_tree: ShardTree<
                _,
                { NOTE_COMMITMENT_TREE_DEPTH },
                SAPLING_SHARD_HEIGHT,
            > = ShardTree::new(sapling_store, SHARDTREE_PRUNING_DEPTH);
            let mut orchard_tree: ShardTree<
                _,
                { NOTE_COMMITMENT_TREE_DEPTH },
                ORCHARD_SHARD_HEIGHT,
            > = ShardTree::new(orchard_store, SHARDTREE_PRUNING_DEPTH);
            let result = apply_shardtree_batches_to_trees(
                &mut sapling_tree,
                &mut orchard_tree,
                batches,
                batch_end_height,
                max_committed_heights,
                verified_roots,
            )?;
            sapling_tree
                .into_store()
                .flush()
                .map_err(|e| Error::Sync(format!("Failed to flush Sapling shard store: {}", e)))?;
            orchard_tree
                .into_store()
                .flush()
                .map_err(|e| Error::Sync(format!("Failed to flush Orchard shard store: {}", e)))?;
            result
        } else {
            let mut sapling_tree: ShardTree<
                _,
                { NOTE_COMMITMENT_TREE_DEPTH },
                SAPLING_SHARD_HEIGHT,
            > = ShardTree::new(sapling_store, SHARDTREE_PRUNING_DEPTH);
            let mut orchard_tree: ShardTree<
                _,
                { NOTE_COMMITMENT_TREE_DEPTH },
                ORCHARD_SHARD_HEIGHT,
            > = ShardTree::new(orchard_store, SHARDTREE_PRUNING_DEPTH);
            apply_shardtree_batches_to_trees(
                &mut sapling_tree,
                &mut orchard_tree,
                batches,
                batch_end_height,
                max_committed_heights,
                verified_roots,
            )?
        };

        persist_verified_pool_roots::<SaplingNode, SAPLING_SHARD_HEIGHT>(
            &tx,
            SAPLING_TABLE_PREFIX,
            &verified_roots.sapling,
        )?;
        persist_verified_pool_roots::<MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>(
            &tx,
            ORCHARD_TABLE_PREFIX,
            &verified_roots.ironwood,
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
                crate::pipeline::NoteType::Ironwood => {
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
                sapling::Rseed::AfterZip212(rseed_bytes)
            } else {
                let rcm = Option::from(jubjub::Fr::from_bytes(&rseed_bytes))
                    .ok_or_else(|| Error::Sync("Invalid Sapling rseed bytes".to_string()))?;
                sapling::Rseed::BeforeZip212(rcm)
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
                Some(sapling::Diversifier(d))
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
                    let note_value = sapling::value::NoteValue::from_raw(note.value);
                    let sapling_note =
                        sapling::Note::from_parts(payment_address, note_value, rseed);
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

    fn apply_diversifier_indices(&self, notes: &mut [DecryptedNote]) {
        for note in notes {
            if note.diversifier_index_88.is_some() || note.note_bytes.is_empty() {
                continue;
            }

            let mut candidates = self.keys.iter().filter(|keys| {
                note.key_id
                    .is_none_or(|selected_key_id| keys.key_id == selected_key_id)
            });
            match note.note_type {
                NoteType::Sapling => {
                    let Some(address) =
                        decode_sapling_address_bytes_from_note_bytes(&note.note_bytes)
                            .and_then(|bytes| SaplingPaymentAddress::from_bytes(&bytes))
                            .map(|inner| PiratePaymentAddress { inner })
                    else {
                        continue;
                    };
                    if let Some((key_id, index, scope)) = candidates.find_map(|keys| {
                        keys.sapling_dfvk.as_ref().and_then(|dfvk| {
                            dfvk.diversifier_index(&address)
                                .map(|(index, scope)| (keys.key_id, index, scope))
                        })
                    }) {
                        note.key_id = Some(key_id);
                        note.diversifier_index_88 = Some(index);
                        note.address_scope = match scope {
                            DiversifierScope::External => AddressScope::External,
                            DiversifierScope::Internal => AddressScope::Internal,
                        };
                    }
                }
                NoteType::Ironwood => {
                    let Some(address) =
                        decode_orchard_address_bytes_from_note_bytes(&note.note_bytes)
                            .and_then(|bytes| {
                                Option::from(OrchardAddress::from_raw_address_bytes(&bytes))
                            })
                            .map(|inner| PirateIronwoodPaymentAddress { inner })
                    else {
                        continue;
                    };
                    if let Some((key_id, index, scope)) = candidates.find_map(|keys| {
                        let fvk = keys.orchard_fvk.as_ref()?;
                        [DiversifierScope::External, DiversifierScope::Internal]
                            .into_iter()
                            .find_map(|scope| {
                                fvk.diversifier_index(&address, scope)
                                    .map(|index| (keys.key_id, index, scope))
                            })
                    }) {
                        note.key_id = Some(key_id);
                        note.diversifier_index_88 = Some(index);
                        note.address_scope = match scope {
                            DiversifierScope::External => AddressScope::External,
                            DiversifierScope::Internal => AddressScope::Internal,
                        };
                    }
                }
            }
        }
    }

    async fn apply_spends(
        &mut self,
        blocks: &[CompactBlockData],
        db: Option<&Database>,
        persistence_worker: Option<&PersistenceWorker>,
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
                            let note_type = pirate_storage_sqlite::models::NoteType::Ironwood;
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
                            pirate_storage_sqlite::models::NoteType::Ironwood => {
                                has_orchard_rederive_keys
                            }
                        });
                if should_attempt_rederive {
                    let recovered = if let Some(worker) = persistence_worker {
                        let worker_sink = sink.clone();
                        let worker_keys = self.keys.clone();
                        let worker_entries = unlinked_entries.clone();
                        worker
                            .execute(move |worker_db| {
                                Self::rederive_unmatched_spends_with_context(
                                    &worker_sink,
                                    &worker_keys,
                                    &worker_entries,
                                    worker_db,
                                )
                            })
                            .await?
                    } else {
                        self.rederive_unmatched_spends(&unlinked_entries, db)?
                    };
                    match recovered {
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
                let result = if let Some(worker) = persistence_worker {
                    let worker_sink = sink.clone();
                    let worker_entries = unlinked_entries.clone();
                    worker
                        .execute(move |worker_db| {
                            let repo = Repository::new(worker_db);
                            Ok(repo.upsert_unlinked_spend_nullifiers_with_txid(
                                worker_sink.account_id,
                                &worker_entries,
                            )?)
                        })
                        .await
                } else {
                    sink.upsert_unlinked_spend_nullifiers_with_txid(&unlinked_entries)
                };
                if let Err(e) = result {
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
        let apply_result = if let Some(worker) = persistence_worker {
            let worker_sink = sink.clone();
            let worker_spend_updates = spend_updates.clone();
            let worker_fallback_entries = fallback_entries.clone();
            let worker_tx_updates = tx_updates.clone();
            worker
                .execute(move |worker_db| {
                    worker_sink.apply_spend_updates_with_txmeta_with_db(
                        worker_db,
                        &worker_spend_updates,
                        &worker_fallback_entries,
                        &worker_tx_updates,
                    )
                })
                .await
        } else {
            match db {
                Some(db) => sink.apply_spend_updates_with_txmeta_with_db(
                    db,
                    &spend_updates,
                    &fallback_entries,
                    &tx_updates,
                ),
                None => sink.apply_spend_updates_with_txmeta(
                    &spend_updates,
                    &fallback_entries,
                    &tx_updates,
                ),
            }
        };
        match apply_result {
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

    fn rederive_unmatched_sapling_spends_with_context(
        sink: &StorageSink,
        keys: &[WalletKeyGroup],
        entries: &[TypedSpendEntry],
        db: &Database,
    ) -> Result<Vec<RecoveredSpend>> {
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
        for key in keys {
            if let Some(dfvk) = key.sapling_dfvk.as_ref() {
                dfvk_by_key.insert(key.key_id, dfvk.clone());
            }
        }
        if dfvk_by_key.is_empty() {
            return Ok(Vec::new());
        }

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
            let note_value = sapling::value::NoteValue::from_raw(value);
            let sapling_note = sapling::Note::from_parts(payment_address, note_value, rseed);

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

    fn rederive_unmatched_sapling_spends(
        &self,
        entries: &[TypedSpendEntry],
        db: Option<&Database>,
    ) -> Result<Vec<RecoveredSpend>> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(Vec::new());
        };
        let opened_db;
        let db = if let Some(db) = db {
            db
        } else {
            opened_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &opened_db
        };
        Self::rederive_unmatched_sapling_spends_with_context(sink, &self.keys, entries, db)
    }

    fn rederive_unmatched_orchard_spends_with_context(
        sink: &StorageSink,
        keys: &[WalletKeyGroup],
        entries: &[TypedSpendEntry],
        db: &Database,
    ) -> Result<Vec<RecoveredSpend>> {
        let mut spend_map: HashMap<NullifierBytes, TxidBytes> = HashMap::new();
        for (note_type, nf, txid) in entries {
            if *note_type == pirate_storage_sqlite::models::NoteType::Ironwood {
                spend_map.entry(*nf).or_insert(*txid);
            }
        }
        if spend_map.is_empty() {
            return Ok(Vec::new());
        }

        let mut fvk_by_key: HashMap<i64, IronwoodExtendedFullViewingKey> = HashMap::new();
        for key in keys {
            if let Some(fvk) = key.orchard_fvk.as_ref() {
                fvk_by_key.insert(key.key_id, fvk.clone());
            }
        }
        if fvk_by_key.is_empty() {
            return Ok(Vec::new());
        }

        let repo = Repository::new(db);
        let notes = repo.get_spend_reconciliation_notes(sink.account_id)?;

        let mut recovered: Vec<RecoveredSpend> = Vec::new();
        for mut note in notes {
            if note.note_type != pirate_storage_sqlite::models::NoteType::Ironwood {
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

            let mut candidate_keys: Vec<(i64, &IronwoodExtendedFullViewingKey)> = Vec::new();
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

    fn rederive_unmatched_orchard_spends(
        &self,
        entries: &[TypedSpendEntry],
        db: Option<&Database>,
    ) -> Result<Vec<RecoveredSpend>> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(Vec::new());
        };
        let opened_db;
        let db = if let Some(db) = db {
            db
        } else {
            opened_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &opened_db
        };
        Self::rederive_unmatched_orchard_spends_with_context(sink, &self.keys, entries, db)
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
                pirate_storage_sqlite::models::NoteType::Ironwood,
                nf,
                txid,
            ));
        }
        Ok(recovered)
    }

    fn rederive_unmatched_spends_with_context(
        sink: &StorageSink,
        keys: &[WalletKeyGroup],
        entries: &[TypedSpendEntry],
        db: &Database,
    ) -> Result<Vec<TypedRecoveredSpend>> {
        let mut recovered = Vec::new();
        for (id, nf, txid) in
            Self::rederive_unmatched_sapling_spends_with_context(sink, keys, entries, db)?
        {
            recovered.push((
                id,
                pirate_storage_sqlite::models::NoteType::Sapling,
                nf,
                txid,
            ));
        }
        for (id, nf, txid) in
            Self::rederive_unmatched_orchard_spends_with_context(sink, keys, entries, db)?
        {
            recovered.push((
                id,
                pirate_storage_sqlite::models::NoteType::Ironwood,
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
        repo.resolve_orchard_anchor_from_db_state(anchors.ironwood_anchor_height)
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
        db_session: Option<&Database>,
        persistence_worker: Option<&PersistenceWorker>,
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

        if let Some(worker) = persistence_worker {
            return worker.checkpoint_shardtrees(checkpoint_id).await;
        }

        let opened_db;
        let db = if let Some(db) = db_session {
            db
        } else {
            opened_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &opened_db
        };
        Self::create_checkpoint_with_db(db, checkpoint_height)
    }

    fn create_checkpoint_with_db(db: &Database, checkpoint_height: u32) -> Result<()> {
        let checkpoint_id = BlockHeight::from(checkpoint_height);
        let tx = db
            .unchecked_immediate_transaction()
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

    async fn retain_checkpoint(
        &self,
        height: u64,
        warm_trees: Option<&mut SyncWarmTrees<'_>>,
        db_session: Option<&Database>,
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<()> {
        let Some(sink) = self.storage.as_ref() else {
            return Ok(());
        };
        let checkpoint_height = u32::try_from(height).map_err(|_| {
            Error::Sync(format!(
                "Retained checkpoint height {} exceeds u32::MAX",
                height
            ))
        })?;
        let checkpoint_id = BlockHeight::from(checkpoint_height);

        if let Some(trees) = warm_trees {
            return trees.retain_checkpoint(checkpoint_id);
        }

        if let Some(worker) = persistence_worker {
            return worker.retain_shardtree_checkpoint(checkpoint_id).await;
        }

        let opened_db;
        let db = if let Some(db) = db_session {
            db
        } else {
            opened_db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            &opened_db
        };
        Self::retain_checkpoint_with_db(db, checkpoint_height)
    }

    fn retain_checkpoint_with_db(db: &Database, checkpoint_height: u32) -> Result<()> {
        let checkpoint_id = BlockHeight::from(checkpoint_height);
        let tx = db.unchecked_immediate_transaction().map_err(|e| {
            Error::Sync(format!(
                "Failed to start retained-checkpoint transaction: {}",
                e
            ))
        })?;
        {
            let store = SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                &tx,
                SAPLING_TABLE_PREFIX,
            )
            .map_err(|e| Error::Sync(format!("Failed to open Sapling shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
            tree.ensure_retained(checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to retain Sapling checkpoint {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }
        {
            let store =
                SqliteShardStore::<_, MerkleHashOrchard, ORCHARD_SHARD_HEIGHT>::from_connection(
                    &tx,
                    ORCHARD_TABLE_PREFIX,
                )
                .map_err(|e| Error::Sync(format!("Failed to open Orchard shard store: {}", e)))?;
            let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT> =
                ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
            tree.ensure_retained(checkpoint_id).map_err(|e| {
                Error::Sync(format!(
                    "Failed to retain Orchard checkpoint {}: {}",
                    checkpoint_height, e
                ))
            })?;
        }
        tx.commit()
            .map_err(|e| Error::Sync(format!("Failed to commit retained checkpoint: {}", e)))?;
        Ok(())
    }

    /// Save sync state periodically
    #[allow(clippy::too_many_arguments)]
    async fn save_sync_state(
        &self,
        local_height: u64,
        target_height: u64,
        last_checkpoint: u64,
        canonical_blocks: &[CompactBlockData],
        include_aux_state_update: bool,
        db_session: Option<&Database>,
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<(u128, u128)> {
        if let Some(ref sink) = self.storage {
            if let Some(worker) = persistence_worker {
                let worker_sink = sink.clone();
                let worker_blocks = canonical_blocks.to_vec();
                let birthday_height = self.birthday_height;
                return worker
                    .execute(move |worker_db| {
                        Self::save_sync_state_with_db(
                            &worker_sink,
                            birthday_height,
                            local_height,
                            target_height,
                            last_checkpoint,
                            &worker_blocks,
                            include_aux_state_update,
                            worker_db,
                        )
                    })
                    .await;
            }
            let opened_db;
            let db = if let Some(db) = db_session {
                db
            } else {
                opened_db =
                    Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
                &opened_db
            };
            return Self::save_sync_state_with_db(
                sink,
                self.birthday_height,
                local_height,
                target_height,
                last_checkpoint,
                canonical_blocks,
                include_aux_state_update,
                db,
            );
        }
        Ok((0, 0))
    }

    #[allow(clippy::too_many_arguments)]
    fn save_sync_state_with_db(
        sink: &StorageSink,
        birthday_height: u32,
        local_height: u64,
        target_height: u64,
        last_checkpoint: u64,
        canonical_blocks: &[CompactBlockData],
        include_aux_state_update: bool,
        db: &Database,
    ) -> Result<(u128, u128)> {
        let progress_start = Instant::now();
        sink.save_sync_progress_with_db(
            db,
            canonical_blocks,
            local_height,
            target_height,
            last_checkpoint,
        )?;
        let progress_ms = progress_start.elapsed().as_millis();
        if !include_aux_state_update {
            return Ok((progress_ms, 0));
        }
        let aux_start = Instant::now();
        let scan_queue = ScanQueueStorage::new(db);
        let historic_start = (birthday_height as u64).max(1);
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
            let validated_for_anchor =
                current_state.spendable && current_state.validated_anchor_height >= computed_anchor;

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
        Ok((progress_ms, aux_start.elapsed().as_millis()))
    }

    /// Rollback to a specific checkpoint height.
    ///
    /// Uses only ShardTree truncation (via `truncate_above_height`). Position counters
    /// are recovered from the ShardTree's checkpoint state after truncation.
    async fn rollback_to_checkpoint(
        &mut self,
        checkpoint_height: u64,
        persistence_worker: Option<&PersistenceWorker>,
    ) -> Result<u64> {
        let Some(ref sink) = self.storage else {
            *self.sapling_tree_position.write().await = 0;
            *self.orchard_tree_position.write().await = 0;
            return Ok(checkpoint_height);
        };

        let tree_replay_height = if let Some(worker) = persistence_worker {
            worker
                .execute_invalidating_shardtrees(move |db| {
                    truncate_above_height(db, checkpoint_height).map_err(Into::into)
                })
                .await?
        } else {
            let db = Database::open_existing(&sink.db_path, &sink.key, sink.master_key.clone())?;
            truncate_above_height(&db, checkpoint_height)?
        };

        self.nullifier_cache.clear();
        self.nullifier_cache_loaded = false;
        self.tracked_wallet_txids.clear();
        self.recover_position_counters_from_shardtree().await?;

        tracing::info!(
            "Rolled back wallet data to {} and commitment trees to {} (sapling_pos={}, orchard_pos={})",
            checkpoint_height,
            tree_replay_height,
            *self.sapling_tree_position.read().await,
            *self.orchard_tree_position.read().await,
        );
        Ok(tree_replay_height)
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

        let rollback_height = self.rollback_to_checkpoint(checkpoint_height, None).await?;
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
                self.rollback_to_common_ancestor(height, None).await?;
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
    address: sapling::PaymentAddress,
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
    let ivk_ct = IronwoodIncomingViewingKey::from_bytes(ivk_bytes);
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
    let rho = de_ct(OrchardRho::from_bytes(&rho_bytes))
        .ok_or_else(|| Error::Sync("Invalid Orchard rho bytes".to_string()))?;
    let rseed = de_ct(OrchardRandomSeed::from_bytes(rseed_bytes, &rho))
        .ok_or_else(|| Error::Sync("Invalid Orchard rseed bytes".to_string()))?;
    let note_value = OrchardNoteValue::from_raw(value);
    let note = de_ct(OrchardNote::from_parts(
        address,
        note_value,
        rho,
        rseed,
        OrchardNoteVersion::V3,
    ))
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
        let mut existing_by_output: HashMap<(Vec<u8>, i64, StorageNoteType), NoteRecord> =
            HashMap::new();
        let mut address_cache: HashMap<String, i64> = HashMap::new();
        let candidate_output_keys: HashSet<(Vec<u8>, i64, StorageNoteType)> = notes
            .iter()
            .filter(|note| !note.txid.is_empty())
            .map(|note| {
                (
                    note.txid.clone(),
                    note.output_index as i64,
                    match note.note_type {
                        crate::pipeline::NoteType::Sapling => StorageNoteType::Sapling,
                        crate::pipeline::NoteType::Ironwood => StorageNoteType::Ironwood,
                    },
                )
            })
            .collect();
        if !candidate_output_keys.is_empty() {
            for existing in repo.prepare_note_upserts(self.account_id, &candidate_output_keys)? {
                let key = (
                    existing.txid.clone(),
                    existing.output_index,
                    existing.note_type,
                );
                existing_by_output.insert(key, existing);
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
                crate::pipeline::NoteType::Ironwood => {
                    decode_orchard_address_bytes_from_note_bytes(&note.note_bytes)
                        .and_then(|bytes| {
                            Option::from(OrchardAddress::from_raw_address_bytes(&bytes))
                        })
                        .and_then(|addr| {
                            PirateIronwoodPaymentAddress { inner: addr }
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
                crate::pipeline::NoteType::Ironwood => {
                    pirate_storage_sqlite::models::AddressType::Ironwood
                }
            };

            let existing_address = repo.get_address_by_string(self.account_id, &address_string)?;
            let diversifier_index = match existing_address.as_ref() {
                Some(address) => address.diversifier_index,
                None => match note.key_id {
                    Some(key_id) => repo.get_next_diversifier_index_for_scope_and_type(
                        self.account_id,
                        key_id,
                        note.address_scope,
                        address_type,
                    )?,
                    None => 0,
                },
            };

            let address_record = pirate_storage_sqlite::Address {
                id: existing_address.and_then(|address| address.id),
                key_id: note.key_id,
                account_id: self.account_id,
                diversifier_index,
                diversifier_index_88: note.diversifier_index_88,
                address: address_string.clone(),
                address_type,
                label: None,
                created_at: timestamp,
                color_tag: pirate_storage_sqlite::address_book::ColorTag::None,
                address_scope: note.address_scope,
            };
            repo.upsert_address(&address_record)?;
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
                    crate::pipeline::NoteType::Ironwood => {
                        pirate_storage_sqlite::models::NoteType::Ironwood
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
                crate::pipeline::NoteType::Ironwood => {
                    pirate_storage_sqlite::models::NoteType::Ironwood
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
            let output_key = (n.txid.clone(), n.output_index as i64, note_type);
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
                        crate::pipeline::NoteType::Ironwood => position_mappings
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
            NoteType::Ironwood => pirate_storage_sqlite::models::NoteType::Ironwood,
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
        self.update_note_memo_with_db(&db, txid, output_index, note_type, memo)
    }

    fn update_note_memo_with_db(
        &self,
        db: &Database,
        txid: &[u8],
        output_index: i64,
        note_type: NoteType,
        memo: Option<&[u8]>,
    ) -> Result<()> {
        let repo = Repository::new(db);
        let note_type = match note_type {
            NoteType::Sapling => pirate_storage_sqlite::models::NoteType::Sapling,
            NoteType::Ironwood => pirate_storage_sqlite::models::NoteType::Ironwood,
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
        self.upsert_tx_memo_with_db(&db, txid_hex, memo)
    }

    fn upsert_tx_memo_with_db(&self, db: &Database, txid_hex: &str, memo: &[u8]) -> Result<()> {
        let repo = Repository::new(db);
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

    fn save_sync_progress_with_db(
        &self,
        db: &Database,
        blocks: &[CompactBlockData],
        local_height: u64,
        target_height: u64,
        last_checkpoint_height: u64,
    ) -> Result<()> {
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
        Ok(sync_state.save_sync_progress(
            &rows,
            local_height,
            target_height,
            last_checkpoint_height,
            MAX_REORG_SEARCH_DEPTH,
        )?)
    }

    fn load_chain_block(&self, height: u64) -> Result<Option<ChainBlockRow>> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        self.load_chain_block_with_db(&db, height)
    }

    fn load_chain_block_with_db(
        &self,
        db: &Database,
        height: u64,
    ) -> Result<Option<ChainBlockRow>> {
        let sync_state = SyncStateStorage::new(db);
        Ok(sync_state.load_chain_block(height)?)
    }

    fn load_chain_blocks(&self, start_height: u64, end_height: u64) -> Result<Vec<ChainBlockRow>> {
        let db = Database::open_existing(&self.db_path, &self.key, self.master_key.clone())?;
        let sync_state = SyncStateStorage::new(&db);
        Ok(sync_state.load_chain_blocks(start_height, end_height)?)
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

#[derive(Clone, Debug, Default)]
struct PositionMaps {
    sapling_by_tx: HashMap<TxOutputKey, u64>,
    orchard_by_commitment: HashMap<[u8; 32], u64>,
}

/// Wallet keys cached for trial decryption
#[derive(Clone)]
struct WalletKeyGroup {
    key_id: i64,
    key_type: KeyType,
    seed_derivation_index: Option<u32>,
    discovery_candidate: bool,
    sapling_dfvk: Option<ExtendedFullViewingKey>,
    orchard_fvk: Option<IronwoodExtendedFullViewingKey>,
    sapling_ivk: Option<[u8; 32]>,
    orchard_ivk: Option<[u8; 64]>,
    sapling_ovk: Option<SaplingOutgoingViewingKey>,
    orchard_ovk: Option<orchard::keys::OutgoingViewingKey>,
}

#[derive(Clone, Default)]
struct TrialDecryptKeys {
    sapling_ivks: Vec<PreparedIncomingViewingKey>,
    sapling_key_ids: Vec<i64>,
    sapling_scopes: Vec<AddressScope>,
    orchard_ivks: Vec<OrchardPreparedIncomingViewingKey>,
    orchard_key_ids: Vec<i64>,
    orchard_scopes: Vec<AddressScope>,
    orchard_fvks: Vec<orchard::keys::FullViewingKey>,
}

/// Privacy-safe summary of the keys that reached the sync scanner.
///
/// Stored account-key counts are deliberately kept separate from usable key
/// groups and prepared IVKs. That distinction makes incomplete or unsupported
/// records visible without logging any key bytes, addresses, or fingerprints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SyncKeyInventory {
    account_key_count: usize,
    key_group_count: usize,
    seed_count: usize,
    seed_derived_count: usize,
    seed_discovery_candidate_count: usize,
    imported_spending_count: usize,
    imported_viewing_count: usize,
    spendable_count: usize,
    sapling_key_group_count: usize,
    ironwood_key_group_count: usize,
    sapling_imported_spending_count: usize,
    ironwood_imported_spending_count: usize,
    sapling_imported_viewing_count: usize,
    ironwood_imported_viewing_count: usize,
    sapling_imported_spending_key_group_count: usize,
    ironwood_imported_spending_key_group_count: usize,
    sapling_imported_spending_ivk_count: usize,
    ironwood_imported_spending_ivk_count: usize,
    sapling_min_birthday_height: Option<i64>,
    ironwood_min_birthday_height: Option<i64>,
    sapling_ivk_count: usize,
    ironwood_ivk_count: usize,
    sapling_external_ivk_count: usize,
    sapling_internal_ivk_count: usize,
    ironwood_external_ivk_count: usize,
    ironwood_internal_ivk_count: usize,
}

impl SyncKeyInventory {
    fn from_sources(
        account_keys: &[AccountKey],
        key_groups: &[WalletKeyGroup],
        trial_decrypt_keys: &TrialDecryptKeys,
    ) -> Self {
        let sapling_imported_spending_key_ids = key_groups
            .iter()
            .filter(|group| {
                group.key_type == KeyType::ImportSpend
                    && group.seed_derivation_index.is_none()
                    && group.sapling_dfvk.is_some()
            })
            .map(|group| group.key_id)
            .collect::<HashSet<_>>();
        let ironwood_imported_spending_key_ids = key_groups
            .iter()
            .filter(|group| {
                group.key_type == KeyType::ImportSpend
                    && group.seed_derivation_index.is_none()
                    && group.orchard_fvk.is_some()
            })
            .map(|group| group.key_id)
            .collect::<HashSet<_>>();
        let mut inventory = Self {
            account_key_count: account_keys.len(),
            key_group_count: key_groups.len(),
            sapling_key_group_count: key_groups
                .iter()
                .filter(|group| group.sapling_dfvk.is_some())
                .count(),
            ironwood_key_group_count: key_groups
                .iter()
                .filter(|group| group.orchard_fvk.is_some())
                .count(),
            sapling_imported_spending_key_group_count: sapling_imported_spending_key_ids.len(),
            ironwood_imported_spending_key_group_count: ironwood_imported_spending_key_ids.len(),
            sapling_imported_spending_ivk_count: trial_decrypt_keys
                .sapling_key_ids
                .iter()
                .filter(|key_id| sapling_imported_spending_key_ids.contains(key_id))
                .count(),
            ironwood_imported_spending_ivk_count: trial_decrypt_keys
                .orchard_key_ids
                .iter()
                .filter(|key_id| ironwood_imported_spending_key_ids.contains(key_id))
                .count(),
            sapling_ivk_count: trial_decrypt_keys.sapling_ivks.len(),
            ironwood_ivk_count: trial_decrypt_keys.orchard_ivks.len(),
            sapling_external_ivk_count: trial_decrypt_keys
                .sapling_scopes
                .iter()
                .filter(|scope| **scope == AddressScope::External)
                .count(),
            sapling_internal_ivk_count: trial_decrypt_keys
                .sapling_scopes
                .iter()
                .filter(|scope| **scope == AddressScope::Internal)
                .count(),
            ironwood_external_ivk_count: trial_decrypt_keys
                .orchard_scopes
                .iter()
                .filter(|scope| **scope == AddressScope::External)
                .count(),
            ironwood_internal_ivk_count: trial_decrypt_keys
                .orchard_scopes
                .iter()
                .filter(|scope| **scope == AddressScope::Internal)
                .count(),
            seed_derived_count: key_groups
                .iter()
                .filter(|group| group.seed_derivation_index.is_some())
                .count(),
            seed_discovery_candidate_count: key_groups
                .iter()
                .filter(|group| group.discovery_candidate)
                .count(),
            ..Self::default()
        };

        let seed_derived_key_ids = key_groups
            .iter()
            .filter(|group| group.seed_derivation_index.is_some())
            .map(|group| group.key_id)
            .collect::<HashSet<_>>();

        for key in account_keys {
            let has_sapling = key.sapling_extsk.is_some() || key.sapling_dfvk.is_some();
            let has_ironwood = key.orchard_extsk.is_some() || key.orchard_fvk.is_some();

            let is_seed_derived = key
                .id
                .is_some_and(|key_id| seed_derived_key_ids.contains(&key_id));
            if !is_seed_derived {
                match key.key_type {
                    KeyType::Seed => inventory.seed_count += 1,
                    KeyType::ImportSpend => {
                        inventory.imported_spending_count += 1;
                        inventory.sapling_imported_spending_count += usize::from(has_sapling);
                        inventory.ironwood_imported_spending_count += usize::from(has_ironwood);
                    }
                    KeyType::ImportView => {
                        inventory.imported_viewing_count += 1;
                        inventory.sapling_imported_viewing_count += usize::from(has_sapling);
                        inventory.ironwood_imported_viewing_count += usize::from(has_ironwood);
                    }
                }
            }
            if key.spendable {
                inventory.spendable_count += 1;
            }
            if has_sapling {
                inventory.sapling_min_birthday_height = Some(
                    inventory
                        .sapling_min_birthday_height
                        .map_or(key.birthday_height, |height| {
                            height.min(key.birthday_height)
                        }),
                );
            }
            if has_ironwood {
                inventory.ironwood_min_birthday_height = Some(
                    inventory
                        .ironwood_min_birthday_height
                        .map_or(key.birthday_height, |height| {
                            height.min(key.birthday_height)
                        }),
                );
            }
        }

        inventory
    }

    fn append_debug_event(self, wallet_id: &str) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let event = serde_json::json!({
            "id": "log_sync_key_inventory",
            "timestamp": timestamp,
            "location": "sync::SyncEngine::with_wallet_at_path",
            "message": "sync key inventory",
            "data": {
                "schema_version": SYNC_KEY_INVENTORY_LOG_SCHEMA_VERSION,
                "wallet_id": wallet_id,
                "account_key_count": self.account_key_count,
                "key_group_count": self.key_group_count,
                "seed_count": self.seed_count,
                "seed_derived_count": self.seed_derived_count,
                "seed_discovery_candidate_count": self.seed_discovery_candidate_count,
                "imported_spending_count": self.imported_spending_count,
                "imported_viewing_count": self.imported_viewing_count,
                "spendable_count": self.spendable_count,
                "sapling_key_group_count": self.sapling_key_group_count,
                "ironwood_key_group_count": self.ironwood_key_group_count,
                "sapling_imported_spending_count": self.sapling_imported_spending_count,
                "ironwood_imported_spending_count": self.ironwood_imported_spending_count,
                "sapling_imported_viewing_count": self.sapling_imported_viewing_count,
                "ironwood_imported_viewing_count": self.ironwood_imported_viewing_count,
                "sapling_imported_spending_key_group_count": self.sapling_imported_spending_key_group_count,
                "ironwood_imported_spending_key_group_count": self.ironwood_imported_spending_key_group_count,
                "sapling_imported_spending_ivk_count": self.sapling_imported_spending_ivk_count,
                "ironwood_imported_spending_ivk_count": self.ironwood_imported_spending_ivk_count,
                "sapling_min_birthday_height": self.sapling_min_birthday_height,
                "ironwood_min_birthday_height": self.ironwood_min_birthday_height,
                "sapling_ivk_count": self.sapling_ivk_count,
                "ironwood_ivk_count": self.ironwood_ivk_count,
                "sapling_external_ivk_count": self.sapling_external_ivk_count,
                "sapling_internal_ivk_count": self.sapling_internal_ivk_count,
                "ironwood_external_ivk_count": self.ironwood_external_ivk_count,
                "ironwood_internal_ivk_count": self.ironwood_internal_ivk_count,
            },
            "sessionId": "debug-session",
            "runId": "run1",
            "hypothesisId": "K",
        });
        append_debug_log_line(&event.to_string());
    }
}

impl TrialDecryptKeys {
    fn from_key_groups(keys: &[WalletKeyGroup]) -> Self {
        let mut prepared = Self::default();
        prepared.sapling_ivks.reserve(keys.len().saturating_mul(2));
        prepared
            .sapling_key_ids
            .reserve(keys.len().saturating_mul(2));
        prepared
            .sapling_scopes
            .reserve(keys.len().saturating_mul(2));
        prepared.orchard_ivks.reserve(keys.len().saturating_mul(2));
        prepared
            .orchard_key_ids
            .reserve(keys.len().saturating_mul(2));
        prepared
            .orchard_scopes
            .reserve(keys.len().saturating_mul(2));
        prepared.orchard_fvks.reserve(keys.len().saturating_mul(2));

        for key in keys {
            if let Some(ivk_bytes) = key.sapling_ivk {
                if let Some(ivk_fr) = Option::from(jubjub::Fr::from_bytes(&ivk_bytes)) {
                    let sapling_ivk = SaplingIvk(ivk_fr);
                    prepared
                        .sapling_ivks
                        .push(PreparedIncomingViewingKey::new(&sapling_ivk));
                    prepared.sapling_key_ids.push(key.key_id);
                    prepared.sapling_scopes.push(AddressScope::External);
                }
            }

            // The legacy wallet used each ZIP-32 account's external address
            // directly (including for change), so preparing an internal IVK
            // would double discovery work without adding historical coverage.
            if key.seed_derivation_index.is_none() || !key.discovery_candidate {
                if let Some(dfvk) = key.sapling_dfvk.as_ref() {
                    let internal_ivk_bytes = dfvk.to_internal_ivk_bytes();
                    if let Some(ivk_fr) = Option::from(jubjub::Fr::from_bytes(&internal_ivk_bytes))
                    {
                        let sapling_ivk = SaplingIvk(ivk_fr);
                        prepared
                            .sapling_ivks
                            .push(PreparedIncomingViewingKey::new(&sapling_ivk));
                        prepared.sapling_key_ids.push(key.key_id);
                        prepared.sapling_scopes.push(AddressScope::Internal);
                    }
                }
            }

            if let (Some(ivk_bytes), Some(fvk)) = (key.orchard_ivk, key.orchard_fvk.as_ref()) {
                let ivk_ct = IronwoodIncomingViewingKey::from_bytes(&ivk_bytes);
                if bool::from(ivk_ct.is_some()) {
                    let ivk = ivk_ct.unwrap();
                    prepared
                        .orchard_ivks
                        .push(OrchardPreparedIncomingViewingKey::new(&ivk));
                    prepared.orchard_key_ids.push(key.key_id);
                    prepared.orchard_scopes.push(AddressScope::External);
                    prepared.orchard_fvks.push(fvk.inner.clone());
                }
            }

            if let Some(fvk) = key.orchard_fvk.as_ref() {
                let internal_ivk_bytes = fvk.to_internal_ivk_bytes();
                let ivk_ct = IronwoodIncomingViewingKey::from_bytes(&internal_ivk_bytes);
                if bool::from(ivk_ct.is_some()) {
                    let ivk = ivk_ct.unwrap();
                    prepared
                        .orchard_ivks
                        .push(OrchardPreparedIncomingViewingKey::new(&ivk));
                    prepared.orchard_key_ids.push(key.key_id);
                    prepared.orchard_scopes.push(AddressScope::Internal);
                    prepared.orchard_fvks.push(fvk.inner.clone());
                }
            }
        }

        prepared
    }
}

#[derive(Clone, Debug)]
struct SaplingOutputMeta {
    height: u64,
    tx_index: usize,
    output_index: usize,
    tx_hash_index: usize,
}

#[derive(Clone, Debug)]
struct OrchardOutputMeta {
    height: u64,
    tx_index: usize,
    output_index: usize,
    tx_hash_index: usize,
    commitment: [u8; 32],
}

#[derive(Clone, Debug)]
struct SaplingBatchOutput {
    epk: [u8; 32],
    cmu: [u8; 32],
    ciphertext: [u8; 52],
}

impl ShieldedOutput<SaplingDomain, COMPACT_NOTE_SIZE> for SaplingBatchOutput {
    fn ephemeral_key(&self) -> EphemeralKeyBytes {
        EphemeralKeyBytes(self.epk)
    }

    fn cmstar_bytes(
        &self,
    ) -> <SaplingDomain as zcash_note_encryption::Domain>::ExtractedCommitmentBytes {
        self.cmu
    }

    fn enc_ciphertext(&self) -> &[u8; COMPACT_NOTE_SIZE] {
        &self.ciphertext
    }
}

fn sapling_rseed_to_bytes(note: &sapling::Note) -> (u8, [u8; 32]) {
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
    pool_wall: Duration,
    worker_active: Duration,
    task_count: u64,
}

#[derive(Debug, Default, Clone)]
struct TrialDecryptTelemetry {
    pool_wall: Duration,
    worker_active: Duration,
    task_count: u64,
}

impl TrialDecryptTelemetry {
    fn merge_stage(&mut self, stage: &DecryptBackendTelemetry, _note_type: NoteType) {
        self.pool_wall += stage.pool_wall;
        self.worker_active += stage.worker_active;
        self.task_count = self.task_count.saturating_add(stage.task_count);
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
    task_multiplier: usize,
}

struct MeasuredDecrypt<T> {
    value: T,
    worker_active: Duration,
    task_count: u64,
}

fn try_compact_note_decryption_parallel_measured<D, Output>(
    pool: &rayon::ThreadPool,
    ivks: &[D::IncomingViewingKey],
    outputs: &[(D, Output)],
    max_parallel: usize,
    task_multiplier: usize,
) -> MeasuredDecrypt<Vec<CompactDecryptResult<D>>>
where
    D: zcash_note_encryption::BatchDomain + Sync,
    Output: ShieldedOutput<D, COMPACT_NOTE_SIZE> + Sync,
    D::IncomingViewingKey: Sync,
    D::Note: Send,
    D::Recipient: Send,
{
    if ivks.is_empty() {
        return MeasuredDecrypt {
            value: (0..outputs.len()).map(|_| None).collect(),
            worker_active: Duration::ZERO,
            task_count: 0,
        };
    }

    let outputs_len = outputs.len();
    if outputs_len == 0 {
        return MeasuredDecrypt {
            value: Vec::new(),
            worker_active: Duration::ZERO,
            task_count: 0,
        };
    }

    let max_parallel = max_parallel.max(1);
    if max_parallel == 1 || outputs_len < MIN_PARALLEL_OUTPUTS {
        let worker_started = Instant::now();
        let value = note_batch::try_compact_note_decryption(ivks, outputs);
        return MeasuredDecrypt {
            value,
            worker_active: worker_started.elapsed(),
            task_count: 1,
        };
    }

    // Lookahead decryption shares this pool with current-batch ShardTree
    // construction. More, shorter ordered tasks give later critical-path tree
    // work scheduling opportunities without adding threads or mutable state.
    let target_tasks = max_parallel.saturating_mul(task_multiplier.max(1));
    let mut chunk_size = outputs_len.div_ceil(target_tasks);
    if chunk_size < MIN_PARALLEL_DECRYPT_CHUNK {
        chunk_size = MIN_PARALLEL_DECRYPT_CHUNK;
    }
    let chunk_count = outputs_len.div_ceil(chunk_size);
    if chunk_count <= 1 {
        let worker_started = Instant::now();
        let value = note_batch::try_compact_note_decryption(ivks, outputs);
        return MeasuredDecrypt {
            value,
            worker_active: worker_started.elapsed(),
            task_count: 1,
        };
    }

    let chunks = pool.install(|| {
        outputs
            .par_chunks(chunk_size)
            .map(|chunk| {
                let worker_started = Instant::now();
                let results = note_batch::try_compact_note_decryption(ivks, chunk);
                (results, worker_started.elapsed())
            })
            .collect::<Vec<_>>()
    });
    let worker_active = chunks.iter().map(|(_, active)| *active).sum::<Duration>();
    let task_count = chunks.len() as u64;
    let value = chunks
        .into_iter()
        .flat_map(|(results, _)| results)
        .collect();
    MeasuredDecrypt {
        value,
        worker_active,
        task_count,
    }
}

#[cfg(test)]
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
    try_compact_note_decryption_parallel_measured(pool, ivks, outputs, max_parallel, 1).value
}

fn try_compact_note_decryption_backend<D, Output>(
    pool: &rayon::ThreadPool,
    ivks: &[D::IncomingViewingKey],
    outputs: &[(D, Output)],
    max_parallel: usize,
    task_multiplier: usize,
) -> (Vec<CompactDecryptResult<D>>, DecryptBackendTelemetry)
where
    D: zcash_note_encryption::BatchDomain + Sync,
    Output: ShieldedOutput<D, COMPACT_NOTE_SIZE> + Sync,
    D::IncomingViewingKey: Sync,
    D::Note: Send,
    D::Recipient: Send,
{
    let started = Instant::now();
    let measured = try_compact_note_decryption_parallel_measured(
        pool,
        ivks,
        outputs,
        max_parallel,
        task_multiplier,
    );
    let telemetry = DecryptBackendTelemetry {
        pool_wall: started.elapsed(),
        worker_active: measured.worker_active,
        task_count: measured.task_count,
    };
    (measured.value, telemetry)
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
        task_multiplier,
    } = inputs;

    let sapling_output_capacity = if sapling_ivks.is_empty() {
        0
    } else {
        blocks
            .iter()
            .flat_map(|block| &block.transactions)
            .map(|tx| tx.outputs.len())
            .sum()
    };
    let orchard_output_capacity = if orchard_ivks.is_empty() {
        0
    } else {
        blocks
            .iter()
            .flat_map(|block| &block.transactions)
            .map(|tx| tx.actions.len())
            .sum()
    };
    let transaction_capacity = if sapling_ivks.is_empty() && orchard_ivks.is_empty() {
        0
    } else {
        blocks.iter().map(|block| block.transactions.len()).sum()
    };
    let mut sapling_outputs: Vec<(SaplingDomain, SaplingBatchOutput)> =
        Vec::with_capacity(sapling_output_capacity);
    let mut sapling_meta: Vec<SaplingOutputMeta> = Vec::with_capacity(sapling_output_capacity);
    let mut orchard_outputs: Vec<(IronwoodDomain, CompactAction)> =
        Vec::with_capacity(orchard_output_capacity);
    let mut orchard_meta: Vec<OrchardOutputMeta> = Vec::with_capacity(orchard_output_capacity);
    let mut tx_hashes: Vec<Vec<u8>> = Vec::with_capacity(transaction_capacity);
    let network = PirateNetwork::default();

    for block in blocks {
        let height = block.height;
        let sapling_zip212 = (!sapling_ivks.is_empty())
            .then(|| zip212_enforcement(&network, BlockHeight::from_u32(height as u32)));
        for (tx_idx, tx) in block.transactions.iter().enumerate() {
            let tx_index = tx.index.unwrap_or(tx_idx as u64) as usize;
            let has_decryptable_pool = (!sapling_ivks.is_empty() && !tx.outputs.is_empty())
                || (!orchard_ivks.is_empty() && !tx.actions.is_empty());
            let tx_hash_index = has_decryptable_pool.then(|| {
                let index = tx_hashes.len();
                tx_hashes.push(tx.hash.clone());
                index
            });

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

                    let domain = SaplingDomain::new(
                        sapling_zip212.expect("Sapling outputs require a ZIP 212 policy"),
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
                        tx_hash_index: tx_hash_index
                            .expect("Sapling outputs require a cached transaction hash"),
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
                    let domain = IronwoodDomain::for_compact_action(&compact_action);
                    orchard_outputs.push((domain, compact_action));
                    orchard_meta.push(OrchardOutputMeta {
                        height,
                        tx_index,
                        output_index: action_idx,
                        tx_hash_index: tx_hash_index
                            .expect("Orchard actions require a cached transaction hash"),
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
            task_multiplier,
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
                note_rec.set_tx_hash(tx_hashes[meta.tx_hash_index].clone());
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
            task_multiplier,
        );
        telemetry.merge_stage(&orchard_telemetry, NoteType::Ironwood);

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

                let mut note_rec = DecryptedNote::new_ironwood(OrchardDecryptedNoteInit {
                    height: meta.height,
                    tx_index: meta.tx_index,
                    output_index: meta.output_index,
                    value,
                    commitment,
                    nullifier: [0u8; 32],
                    encrypted_memo: Vec::new(),
                    position: Some(0),
                });
                note_rec.set_tx_hash(tx_hashes[meta.tx_hash_index].clone());
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
        let ivk_ct = IronwoodIncomingViewingKey::from_bytes(ivk_bytes);
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
        task_multiplier: 1,
    })?;
    Ok(batch.notes)
}

// DecryptedNote is imported from pipeline module - no need to redefine here

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn server_info_validation_reserves_extra_time_and_a_fresh_channel_for_i2p() {
        assert_eq!(
            server_info_validation_policy(TransportMode::I2p),
            ServerInfoValidationPolicy {
                attempt_timeout: Duration::from_secs(45),
                max_attempts: 2,
            }
        );
        assert_eq!(
            server_info_validation_policy(TransportMode::Direct),
            ServerInfoValidationPolicy {
                attempt_timeout: Duration::from_secs(5),
                max_attempts: 1,
            }
        );
        assert_eq!(
            server_info_validation_policy(TransportMode::Tor),
            ServerInfoValidationPolicy {
                attempt_timeout: Duration::from_secs(15),
                max_attempts: 1,
            }
        );
        assert_eq!(
            server_info_validation_policy(TransportMode::Socks5),
            ServerInfoValidationPolicy {
                attempt_timeout: Duration::from_secs(15),
                max_attempts: 1,
            }
        );
    }

    fn shardtree_test_database(
        passphrase: &str,
        salt_byte: u8,
    ) -> (tempfile::NamedTempFile, Database, StorageSink) {
        let file = tempfile::NamedTempFile::new().unwrap();
        let key = EncryptionKey::from_passphrase(passphrase, &[salt_byte; 32]).unwrap();
        let master_key =
            MasterKey::generate(pirate_storage_sqlite::EncryptionAlgorithm::ChaCha20Poly1305);
        let db = Database::open(file.path(), &key, master_key.clone()).unwrap();
        let sink = StorageSink {
            db_path: file.path().to_path_buf(),
            key: EncryptionKey::from_bytes(*key.as_bytes()),
            master_key,
            account_id: 1,
            address_network_type: NetworkType::Mainnet,
        };
        (file, db, sink)
    }

    fn persistence_test_batches() -> Vec<ShardtreeBatch> {
        persistence_test_batches_with_count(4)
    }

    fn persistence_test_batches_with_count(count: u32) -> Vec<ShardtreeBatch> {
        (0..count)
            .map(|offset| {
                let height = 100 + offset;
                let marking = if offset == 0 {
                    Marking::Marked
                } else {
                    Marking::None
                };
                let retention = Retention::Checkpoint {
                    id: BlockHeight::from(height),
                    marking,
                };
                let mut batch = ShardtreeBatch::new(u64::from(height));
                batch.checkpoint_id = Some(BlockHeight::from(height));
                shardtree_support::append_sapling_leaf(
                    &mut batch,
                    u64::from(offset),
                    SaplingNode::empty_leaf(),
                    retention,
                );
                shardtree_support::append_orchard_leaf(
                    &mut batch,
                    u64::from(offset),
                    MerkleHashOrchard::empty_leaf(),
                    retention,
                );
                batch
            })
            .collect()
    }

    fn empty_compact_block(height: u64) -> CompactBlockData {
        CompactBlockData {
            proto_version: 1,
            height,
            hash: vec![0; 32],
            prev_hash: vec![0; 32],
            time: 0,
            header: Vec::new(),
            transactions: Vec::new(),
        }
    }

    fn linked_compact_blocks(start: u64, end: u64) -> Vec<CompactBlockData> {
        let mut previous_hash = vec![0x5a; 32];
        (start..=end)
            .map(|height| {
                let mut block = empty_compact_block(height);
                block.prev_hash = previous_hash.clone();
                block.hash = vec![(height & 0xff) as u8; 32];
                previous_hash = block.hash.clone();
                block
            })
            .collect()
    }

    fn test_spending_keys(mnemonic: &str) -> (ExtendedSpendingKey, IronwoodExtendedSpendingKey) {
        let sapling =
            ExtendedSpendingKey::from_mnemonic_with_account(mnemonic, NetworkType::Mainnet, 0)
                .unwrap();
        let seed = ExtendedSpendingKey::seed_bytes_from_mnemonic(mnemonic).unwrap();
        let ironwood = IronwoodExtendedSpendingKey::master(&seed)
            .unwrap()
            .derive_account(133, 0)
            .unwrap();
        (sapling, ironwood)
    }

    #[test]
    fn sync_key_inventory_distinguishes_imported_pools_and_trial_ivks() {
        const MNEMONIC: &str =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        const OTHER_MNEMONIC: &str =
            "legal winner thank year wave sausage worth useful legal winner thank yellow";
        let (sapling, ironwood) = test_spending_keys(MNEMONIC);
        let (imported_sapling, imported_ironwood) = test_spending_keys(OTHER_MNEMONIC);
        let keys = vec![
            AccountKey {
                id: Some(1),
                account_id: 1,
                key_type: KeyType::Seed,
                key_scope: KeyScope::Account,
                label: None,
                birthday_height: 1,
                created_at: 1,
                spendable: true,
                sapling_extsk: Some(sapling.to_bytes()),
                sapling_dfvk: None,
                orchard_extsk: Some(ironwood.to_bytes()),
                orchard_fvk: None,
                encrypted_mnemonic: None,
            },
            AccountKey {
                id: Some(2),
                account_id: 1,
                key_type: KeyType::ImportSpend,
                key_scope: KeyScope::Account,
                label: None,
                birthday_height: 1,
                created_at: 2,
                spendable: true,
                sapling_extsk: Some(imported_sapling.to_bytes()),
                sapling_dfvk: None,
                orchard_extsk: None,
                orchard_fvk: None,
                encrypted_mnemonic: None,
            },
            AccountKey {
                id: Some(3),
                account_id: 1,
                key_type: KeyType::ImportView,
                key_scope: KeyScope::Account,
                label: None,
                birthday_height: 1,
                created_at: 3,
                spendable: false,
                sapling_extsk: None,
                sapling_dfvk: None,
                orchard_extsk: None,
                orchard_fvk: Some(imported_ironwood.to_extended_fvk().to_bytes()),
                encrypted_mnemonic: None,
            },
        ];
        let key_groups = keys
            .iter()
            .filter_map(|key| build_key_group_from_account_key(key, None).unwrap())
            .collect::<Vec<_>>();
        let trial_decrypt_keys = TrialDecryptKeys::from_key_groups(&key_groups);

        let inventory = SyncKeyInventory::from_sources(&keys, &key_groups, &trial_decrypt_keys);

        assert_eq!(inventory.account_key_count, 3);
        assert_eq!(inventory.key_group_count, 3);
        assert_eq!(inventory.seed_count, 1);
        assert_eq!(inventory.imported_spending_count, 1);
        assert_eq!(inventory.imported_viewing_count, 1);
        assert_eq!(inventory.spendable_count, 2);
        assert_eq!(inventory.sapling_key_group_count, 2);
        assert_eq!(inventory.ironwood_key_group_count, 2);
        assert_eq!(inventory.sapling_imported_spending_count, 1);
        assert_eq!(inventory.ironwood_imported_spending_count, 0);
        assert_eq!(inventory.sapling_imported_viewing_count, 0);
        assert_eq!(inventory.ironwood_imported_viewing_count, 1);
        assert_eq!(inventory.sapling_imported_spending_key_group_count, 1);
        assert_eq!(inventory.ironwood_imported_spending_key_group_count, 0);
        assert_eq!(inventory.sapling_imported_spending_ivk_count, 2);
        assert_eq!(inventory.ironwood_imported_spending_ivk_count, 0);
        assert_eq!(inventory.sapling_min_birthday_height, Some(1));
        assert_eq!(inventory.ironwood_min_birthday_height, Some(1));
        assert_eq!(inventory.sapling_ivk_count, 4);
        assert_eq!(inventory.ironwood_ivk_count, 4);
        assert_eq!(inventory.sapling_external_ivk_count, 2);
        assert_eq!(inventory.sapling_internal_ivk_count, 2);
        assert_eq!(inventory.ironwood_external_ivk_count, 2);
        assert_eq!(inventory.ironwood_internal_ivk_count, 2);
    }

    #[test]
    fn seed_derived_sapling_accounts_use_external_scope_without_counting_as_imports() {
        const MNEMONIC: &str =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let account_one =
            ExtendedSpendingKey::from_mnemonic_with_account(MNEMONIC, NetworkType::Mainnet, 1)
                .unwrap();
        let key = AccountKey {
            id: Some(9),
            account_id: 1,
            key_type: KeyType::ImportSpend,
            key_scope: KeyScope::Account,
            label: Some("Seed account 1".to_string()),
            birthday_height: 1,
            created_at: 1,
            spendable: true,
            sapling_extsk: Some(account_one.to_bytes()),
            sapling_dfvk: Some(account_one.to_extended_fvk().to_bytes()),
            orchard_extsk: None,
            orchard_fvk: None,
            encrypted_mnemonic: None,
        };
        let group = build_key_group_from_account_key(&key, Some((1, true)))
            .unwrap()
            .expect("seed-derived key group");
        let groups = vec![group];
        let prepared = TrialDecryptKeys::from_key_groups(&groups);
        let inventory =
            SyncKeyInventory::from_sources(std::slice::from_ref(&key), &groups, &prepared);

        assert_eq!(prepared.sapling_ivks.len(), 1);
        assert_eq!(prepared.sapling_scopes, vec![AddressScope::External]);
        assert_eq!(inventory.seed_derived_count, 1);
        assert_eq!(inventory.seed_discovery_candidate_count, 1);
        assert_eq!(inventory.imported_spending_count, 0);
        assert_eq!(inventory.sapling_imported_spending_ivk_count, 0);

        let durable_group = build_key_group_from_account_key(&key, Some((1, false)))
            .unwrap()
            .expect("durable seed-derived key group");
        let durable = TrialDecryptKeys::from_key_groups(&[durable_group]);
        assert_eq!(durable.sapling_ivks.len(), 2);
        assert_eq!(
            durable.sapling_scopes,
            vec![AddressScope::External, AddressScope::Internal]
        );
    }

    #[test]
    fn spending_keys_override_stale_cached_viewing_keys() {
        const MNEMONIC: &str =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        const OTHER_MNEMONIC: &str =
            "legal winner thank year wave sausage worth useful legal winner thank yellow";
        let (sapling, ironwood) = test_spending_keys(MNEMONIC);
        let (other_sapling, other_ironwood) = test_spending_keys(OTHER_MNEMONIC);
        let key = AccountKey {
            id: Some(7),
            account_id: 1,
            key_type: KeyType::Seed,
            key_scope: KeyScope::Account,
            label: Some("Seed".to_string()),
            birthday_height: 1,
            created_at: 1,
            spendable: true,
            sapling_extsk: Some(sapling.to_bytes()),
            sapling_dfvk: Some(other_sapling.to_extended_fvk().to_bytes()),
            orchard_extsk: Some(ironwood.to_bytes()),
            orchard_fvk: Some(other_ironwood.to_extended_fvk().to_bytes()),
            encrypted_mnemonic: None,
        };

        let group = build_key_group_from_account_key(&key, None)
            .unwrap()
            .expect("spendable key group");

        assert_eq!(
            group.sapling_dfvk.unwrap().to_bytes(),
            sapling.to_extended_fvk().to_bytes()
        );
        assert_eq!(
            group.orchard_fvk.unwrap().to_bytes(),
            ironwood.to_extended_fvk().to_bytes()
        );
    }

    #[test]
    fn wallet_startup_replays_reconciled_keys_without_deleting_addresses() {
        const MNEMONIC: &str =
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        const OTHER_MNEMONIC: &str =
            "legal winner thank year wave sausage worth useful legal winner thank yellow";
        let file = tempfile::NamedTempFile::new().unwrap();
        let key = EncryptionKey::from_passphrase("seed-repair", &[0x51; 32]).unwrap();
        let master_key =
            MasterKey::generate(pirate_storage_sqlite::EncryptionAlgorithm::ChaCha20Poly1305);
        let db = Database::open(file.path(), &key, master_key.clone()).unwrap();
        let repo = Repository::new(&db);
        let account_id = repo
            .insert_account(&pirate_storage_sqlite::Account {
                id: None,
                name: "Seed repair".to_string(),
                created_at: 1,
            })
            .unwrap();
        let (sapling, ironwood) = test_spending_keys(MNEMONIC);
        let (other_sapling, other_ironwood) = test_spending_keys(OTHER_MNEMONIC);
        let secret = pirate_storage_sqlite::WalletSecret {
            wallet_id: "seed-repair-wallet".to_string(),
            account_id,
            extsk: sapling.to_bytes(),
            dfvk: Some(sapling.to_extended_fvk().to_bytes()),
            orchard_extsk: Some(ironwood.to_bytes()),
            sapling_ivk: None,
            orchard_ivk: None,
            encrypted_mnemonic: Some(MNEMONIC.as_bytes().to_vec()),
            mnemonic_language: None,
            created_at: 1,
        };
        let encrypted_secret = repo.encrypt_wallet_secret_fields(&secret).unwrap();
        repo.upsert_wallet_secret(&encrypted_secret).unwrap();
        let stale_key = AccountKey {
            id: None,
            account_id,
            key_type: KeyType::Seed,
            key_scope: KeyScope::Account,
            label: Some("Seed".to_string()),
            birthday_height: 20,
            created_at: 1,
            spendable: true,
            sapling_extsk: Some(secret.extsk.clone()),
            sapling_dfvk: Some(other_sapling.to_extended_fvk().to_bytes()),
            orchard_extsk: secret.orchard_extsk.clone(),
            orchard_fvk: Some(other_ironwood.to_extended_fvk().to_bytes()),
            encrypted_mnemonic: secret.encrypted_mnemonic.clone(),
        };
        let encrypted_key = repo.encrypt_account_key_fields(&stale_key).unwrap();
        let key_id = repo.upsert_account_key(&encrypted_key).unwrap();
        repo.upsert_address(&pirate_storage_sqlite::Address {
            id: None,
            key_id: Some(key_id),
            account_id,
            diversifier_index: 0,
            diversifier_index_88: None,
            address: "pirate1-preserved-address".to_string(),
            address_type: pirate_storage_sqlite::AddressType::Ironwood,
            label: Some("Preserved".to_string()),
            created_at: 1,
            color_tag: pirate_storage_sqlite::ColorTag::None,
            address_scope: pirate_storage_sqlite::AddressScope::External,
        })
        .unwrap();
        SyncStateStorage::new(&db).reset_sync_state(100).unwrap();
        drop(repo);
        drop(db);

        let engine = SyncEngine::new("http://127.0.0.1:8067".to_string(), 20)
            .with_wallet_at_path(
                secret.wallet_id.clone(),
                file.path().to_path_buf(),
                EncryptionKey::from_bytes(*key.as_bytes()),
                master_key.clone(),
                NetworkType::Mainnet,
                NetworkType::Mainnet,
            )
            .unwrap();
        assert_eq!(engine.keys.len(), 1);
        drop(engine);

        let reopened = Database::open(file.path(), &key, master_key).unwrap();
        let reopened_repo = Repository::new(&reopened);
        assert_eq!(
            SyncStateStorage::new(&reopened)
                .load_sync_state()
                .unwrap()
                .local_height,
            0
        );
        assert!(!reopened_repo.seed_key_scan_replay_required().unwrap());
        let addresses = reopened_repo.get_all_addresses(account_id).unwrap();
        assert_eq!(addresses.len(), 1);
        assert_eq!(addresses[0].label.as_deref(), Some("Preserved"));
    }

    fn canonical_test_commitment(value: u8) -> Vec<u8> {
        let mut commitment = vec![0u8; 32];
        commitment[0] = value;
        commitment
    }

    fn compact_tx_with_commitments(
        index: u64,
        hash_byte: u8,
        sapling_commitment: u8,
        ironwood_commitment: u8,
    ) -> crate::client::CompactTx {
        crate::client::CompactTx {
            index: Some(index),
            hash: vec![hash_byte; 32],
            fee: None,
            spends: Vec::new(),
            outputs: vec![crate::client::CompactSaplingOutput {
                cmu: canonical_test_commitment(sapling_commitment),
                ephemeral_key: Vec::new(),
                ciphertext: Vec::new(),
            }],
            actions: vec![crate::client::CompactIronwoodAction {
                nullifier: Vec::new(),
                cmx: canonical_test_commitment(ironwood_commitment),
                ephemeral_key: Vec::new(),
                enc_ciphertext: Vec::new(),
                out_ciphertext: Vec::new(),
            }],
        }
    }

    #[test]
    fn immutable_commitment_preparation_preserves_consensus_transaction_order() {
        let mut block = empty_compact_block(500);
        block.hash = vec![0xa5; 32];
        block.transactions = vec![
            compact_tx_with_commitments(2, 0x22, 2, 12),
            compact_tx_with_commitments(1, 0x11, 1, 11),
        ];
        block.transactions[1]
            .outputs
            .push(crate::client::CompactSaplingOutput {
                cmu: vec![0xff; 31],
                ephemeral_key: Vec::new(),
                ciphertext: Vec::new(),
            });

        let prepared = prepare_commitment_batch(std::slice::from_ref(&block));

        prepared
            .validate_source(std::slice::from_ref(&block))
            .unwrap();
        assert_eq!(prepared.sapling_count, 2);
        assert_eq!(prepared.ironwood_count, 2);
        assert_eq!(prepared.blocks[0].transactions[0].hash, vec![0x11; 32]);
        assert_eq!(prepared.blocks[0].transactions[1].hash, vec![0x22; 32]);
        assert_eq!(
            prepared.blocks[0].transactions[0].sapling[0].commitment[0],
            1
        );
        assert_eq!(
            prepared.blocks[0].transactions[1].ironwood[0].commitment[0],
            12
        );
        assert!(prepared.blocks[0].transactions[0].sapling[0].node.is_some());
        assert!(prepared.blocks[0].transactions[0].ironwood[0]
            .node
            .is_some());

        let mut mismatched = block.clone();
        mismatched.hash[0] ^= 0xff;
        assert!(prepared
            .validate_source(std::slice::from_ref(&mismatched))
            .is_err());
    }

    #[tokio::test]
    async fn prepared_commitment_finalization_preserves_positions_for_both_pools() {
        let mut block = empty_compact_block(500);
        block.hash = vec![0xa5; 32];
        block.transactions = vec![
            compact_tx_with_commitments(2, 0x22, 2, 12),
            compact_tx_with_commitments(1, 0x11, 1, 11),
        ];
        let prepared = prepare_commitment_batch(std::slice::from_ref(&block));
        let mut sapling_note = DecryptedNote::new(500, 1, 0, 1, [2; 32], [0; 32], Vec::new());
        sapling_note.commitment = canonical_test_commitment(2).try_into().unwrap();
        let ironwood_note = DecryptedNote::new_ironwood(OrchardDecryptedNoteInit {
            height: 500,
            tx_index: 0,
            output_index: 0,
            value: 1,
            commitment: canonical_test_commitment(11).try_into().unwrap(),
            nullifier: [0; 32],
            encrypted_memo: Vec::new(),
            position: None,
        });
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 1);

        let (count, mappings, _, _) = engine
            .update_commitment_trees(
                std::slice::from_ref(&block),
                prepared,
                &[sapling_note, ironwood_note],
                FrontierCheckpointMode::OwnedOnly,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(count, 4);
        assert_eq!(*engine.sapling_tree_position.read().await, 2);
        assert_eq!(*engine.orchard_tree_position.read().await, 2);
        assert_eq!(
            mappings.sapling_by_tx.get(&TxOutputKey {
                txid: [0x22; 32],
                index: 0,
            }),
            Some(&1)
        );
        let owned_ironwood_commitment: [u8; 32] = canonical_test_commitment(11).try_into().unwrap();
        assert_eq!(
            mappings
                .orchard_by_commitment
                .get(&owned_ironwood_commitment),
            Some(&0)
        );
    }

    #[tokio::test]
    async fn mismatched_prepared_commitments_cannot_advance_frontier_state() {
        let mut prepared_block = empty_compact_block(500);
        prepared_block.hash = vec![0xa5; 32];
        prepared_block.transactions = vec![compact_tx_with_commitments(0, 0x11, 1, 11)];
        let prepared = prepare_commitment_batch(std::slice::from_ref(&prepared_block));
        let mut validated_block = prepared_block;
        validated_block.hash[0] ^= 0xff;
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 1);

        let result = engine
            .update_commitment_trees(
                std::slice::from_ref(&validated_block),
                prepared,
                &[],
                FrontierCheckpointMode::OwnedOnly,
                None,
                None,
                None,
                None,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(*engine.sapling_tree_position.read().await, 0);
        assert_eq!(*engine.orchard_tree_position.read().await, 0);
    }

    #[test]
    fn canonical_block_window_covers_the_full_reorg_search_depth() {
        let blocks = (10_000..18_000)
            .map(empty_compact_block)
            .collect::<Vec<_>>();

        let retained = canonical_block_window(&blocks);

        assert_eq!(retained.len(), MAX_REORG_SEARCH_DEPTH as usize + 1);
        assert_eq!(retained.first().unwrap().height, 15_999);
        assert_eq!(retained.last().unwrap().height, 17_999);
    }

    #[test]
    fn canonical_block_window_keeps_short_batches_intact() {
        let blocks = (20_000..20_500)
            .map(empty_compact_block)
            .collect::<Vec<_>>();

        assert_eq!(canonical_block_window(&blocks).len(), blocks.len());
    }

    #[test]
    fn reorg_backoff_reaches_the_search_floor_logarithmically() {
        let divergent = 50_000;
        let stop = divergent - MAX_REORG_SEARCH_DEPTH;
        let mut distance = 1u64;
        let mut probes = Vec::new();
        loop {
            let probe = reorg_backoff_probe(divergent, stop, distance);
            probes.push(probe);
            if probe == stop {
                break;
            }
            distance = distance.saturating_mul(2);
        }

        assert_eq!(probes.last(), Some(&stop));
        assert!(probes.len() <= 12, "unexpected probe count: {probes:?}");
    }

    #[test]
    fn resume_validation_deadlines_are_transport_aware() {
        assert_eq!(
            resume_chain_network_timeout(TransportMode::Direct),
            Duration::from_secs(30)
        );
        assert!(
            resume_chain_network_timeout(TransportMode::Tor)
                < resume_chain_network_timeout(TransportMode::I2p)
        );
    }

    #[test]
    fn compact_block_range_requires_complete_linked_sequence() {
        let blocks = linked_compact_blocks(42, 45);

        SyncEngine::validate_compact_block_range(42, 45, &blocks).unwrap();
    }

    #[test]
    fn compact_block_range_rejects_missing_or_reordered_heights() {
        let mut blocks = linked_compact_blocks(42, 45);
        blocks.remove(1);
        assert!(SyncEngine::validate_compact_block_range(42, 45, &blocks).is_err());

        let mut blocks = linked_compact_blocks(42, 45);
        blocks.swap(1, 2);
        assert!(SyncEngine::validate_compact_block_range(42, 45, &blocks).is_err());
    }

    #[test]
    fn compact_block_range_rejects_disconnected_or_malformed_hashes() {
        let mut disconnected = linked_compact_blocks(42, 45);
        disconnected[2].prev_hash = vec![0xff; 32];
        assert!(SyncEngine::validate_compact_block_range(42, 45, &disconnected).is_err());

        let mut malformed = linked_compact_blocks(42, 45);
        malformed[1].hash.truncate(31);
        assert!(SyncEngine::validate_compact_block_range(42, 45, &malformed).is_err());
    }

    #[test]
    fn compact_block_range_rejects_malformed_shielded_payloads() {
        let mut blocks = linked_compact_blocks(42, 45);
        blocks[1].transactions = vec![compact_tx_with_commitments(0, 0x11, 1, 11)];
        blocks[1].transactions[0].outputs[0].cmu.truncate(31);

        assert!(SyncEngine::validate_compact_block_range(42, 45, &blocks).is_err());
    }

    #[tokio::test]
    async fn disconnected_cached_range_is_evicted_before_network_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlockCache::for_test(dir.path().join("blocks.db")).unwrap();
        let mut disconnected = linked_compact_blocks(42, 45);
        disconnected[2].prev_hash = vec![0xff; 32];
        cache.store_blocks(&disconnected).unwrap();
        let client = LightClient::new("http://127.0.0.1:1".to_string());

        assert!(!SyncEngine::cached_blocks_are_canonical(
            &client,
            &cache,
            42,
            45,
            &disconnected,
            Some(ValidatedCacheRange { start: 42, end: 45 }),
        )
        .await
        .unwrap());
        assert!(cache
            .load_range_for_upgrade(42, 45)
            .unwrap()
            .blocks
            .is_empty());
    }

    #[test]
    fn batch_boundary_uses_the_previous_in_memory_hash() {
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 1);
        let blocks = linked_compact_blocks(42, 45);

        assert!(engine
            .validate_batch_boundary(42, &blocks, None, Some(&[0x5a; 32]))
            .unwrap());
        assert!(!engine
            .validate_batch_boundary(42, &blocks, None, Some(&[0xff; 32]))
            .unwrap());
    }

    #[test]
    fn historical_mark_positions_are_reduced_to_pool_subtree_hints() {
        let subtree_leaves = 1u64 << SAPLING_SHARD_HEIGHT;
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 1)
            .with_historical_mark_positions(
                [subtree_leaves + 7, 3 * subtree_leaves + 1],
                [2 * subtree_leaves + 9],
            );

        assert_eq!(
            engine.historical_sapling_mark_subtrees,
            HashSet::from([1, 3])
        );
        assert_eq!(engine.historical_ironwood_mark_subtrees, HashSet::from([2]));
    }

    #[test]
    fn validated_cache_range_is_bounded_on_both_sides() {
        let range = ValidatedCacheRange {
            start: 100,
            end: 200,
        };

        assert!(range.contains(100, 200));
        assert!(range.contains(125, 175));
        assert!(!range.contains(99, 175));
        assert!(!range.contains(125, 201));
    }

    #[test]
    fn bounded_latest_sync_returns_at_one_validated_tip_snapshot() {
        assert_eq!(select_sync_target(100, None, 150, false), 150);
        assert_eq!(select_sync_target(160, None, 150, false), 160);
        assert_eq!(select_sync_target(100, None, 150, true), 150);
        assert_eq!(select_sync_target(100, Some(125), 150, false), 125);
    }

    #[test]
    fn normal_resume_repairs_a_matching_metadata_gap() {
        assert_eq!(
            matching_resume_height(
                2_400_000,
                156_854,
                true,
                ResumeChainPolicy::RepairMetadataGap,
            ),
            156_855
        );
    }

    #[test]
    fn historical_rescan_preserves_its_requested_start_across_a_matching_gap() {
        assert_eq!(
            matching_resume_height(
                2_400_000,
                156_854,
                true,
                ResumeChainPolicy::PreserveHistoricalBootstrap,
            ),
            2_400_000
        );
    }

    #[test]
    fn contiguous_metadata_never_changes_the_requested_start() {
        for policy in [
            ResumeChainPolicy::RepairMetadataGap,
            ResumeChainPolicy::PreserveHistoricalBootstrap,
        ] {
            assert_eq!(
                matching_resume_height(2_400_000, 2_399_999, false, policy),
                2_400_000
            );
        }
    }

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.checkpoint_interval, 10_000);
        assert_eq!(config.batch_size, 4_000);
        assert_eq!(config.max_batch_size, 4_000);
        assert_eq!(config.target_batch_bytes, 128_000_000);
        assert!(config.lazy_memo_decode);
    }

    #[test]
    fn wallet_scan_omits_blocks_before_birthday() {
        let blocks = (98..=102).map(empty_compact_block).collect::<Vec<_>>();

        let relevant = wallet_relevant_blocks(&blocks, 100);

        assert_eq!(
            relevant
                .iter()
                .map(|block| block.height)
                .collect::<Vec<_>>(),
            vec![100, 101, 102]
        );
    }

    #[test]
    fn wallet_scan_can_skip_an_entire_replay_batch() {
        let blocks = (98..=102).map(empty_compact_block).collect::<Vec<_>>();

        assert!(wallet_relevant_blocks(&blocks, 200).is_empty());
    }

    #[test]
    fn direct_tree_state_fetch_uses_one_long_initial_request() {
        let client = crate::client::LightClient::with_config(crate::client::LightClientConfig {
            endpoint: "http://127.0.0.1:9067".to_string(),
            transport: TransportMode::Direct,
            ..crate::client::LightClientConfig::default()
        });
        let engine = SyncEngine::with_client_and_config(client, 3_500_000, SyncConfig::default());
        let profile = engine.tree_state_retry_profile();

        assert_eq!(profile.max_attempts, 1);
        assert_eq!(profile.base_timeout, Duration::from_secs(120));
        assert_eq!(profile.max_timeout, Duration::from_secs(120));
        assert!(!profile.enable_hash_fallback);
    }

    #[tokio::test]
    async fn birthday_seed_rejects_a_tree_state_for_another_height() {
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 3_500_000);
        let result = engine
            .seed_shardtrees_from_tree_state(
                3_499_999,
                TreeState {
                    network: "main".to_string(),
                    height: 3_499_998,
                    hash: String::new(),
                    time: 0,
                    sapling_tree: String::new(),
                    sapling_frontier: String::new(),
                    ironwood_tree: String::new(),
                },
            )
            .await;

        assert!(result.unwrap_err().to_string().contains("expected 3499999"));
    }

    #[tokio::test]
    async fn pre_activation_ironwood_root_is_not_treated_as_a_frontier() {
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 101);
        let result = engine
            .seed_shardtrees_from_tree_state(
                100,
                TreeState {
                    network: "main".to_string(),
                    height: 100,
                    hash: String::new(),
                    time: 0,
                    sapling_tree: String::new(),
                    sapling_frontier: String::new(),
                    ironwood_tree: "00".repeat(32),
                },
            )
            .await;

        assert!(result.is_ok());
    }

    #[test]
    fn fetch_latency_prevents_an_unmeasured_jump_to_the_maximum_batch() {
        let cap = batch_cap_for_target_latency(100, 64, Duration::from_millis(669), 100, 4_000);

        assert_eq!(cap, 200);
    }

    #[test]
    fn fetch_latency_reduces_slow_batches_to_the_configured_floor() {
        let cap =
            batch_cap_for_target_latency(4_000, 4_000, Duration::from_millis(240_857), 100, 4_000);

        assert_eq!(cap, 100);
    }

    #[test]
    fn fetch_latency_doubles_fast_batches_until_the_configured_ceiling() {
        assert_eq!(
            batch_cap_for_target_latency(500, 500, Duration::from_millis(100), 100, 4_000,),
            1_000
        );
        assert_eq!(
            batch_cap_for_target_latency(3_000, 3_000, Duration::from_millis(100), 100, 4_000,),
            4_000
        );
    }

    #[test]
    fn cache_hits_do_not_inflate_the_network_batch_cap() {
        assert_eq!(
            network_batch_cap_after_fetch(
                100,
                BlockFetchSource::Cache,
                4_000,
                Duration::from_millis(10),
                100,
                4_000,
            ),
            100
        );
    }

    #[test]
    fn initial_network_cap_uses_the_heavy_block_byte_budget() {
        let config = SyncConfig {
            min_batch_size: 100,
            max_batch_size: 6_000,
            target_batch_bytes: 96_000_000,
            max_batch_bytes: 128_000_000,
            max_batch_memory_bytes: Some(500_000_000),
            heavy_block_threshold_bytes: 500_000,
            ..SyncConfig::default()
        };

        assert_eq!(initial_network_batch_cap(&config), 192);
    }

    #[test]
    fn initial_network_cap_preserves_the_mobile_floor() {
        let config = SyncConfig {
            min_batch_size: 25,
            max_batch_size: 2_000,
            target_batch_bytes: 8_000_000,
            max_batch_bytes: 16_000_000,
            max_batch_memory_bytes: Some(96_000_000),
            heavy_block_threshold_bytes: 500_000,
            ..SyncConfig::default()
        };

        assert_eq!(initial_network_batch_cap(&config), 25);
    }

    #[test]
    fn initial_network_cap_respects_memory_and_block_limits() {
        let memory_limited = SyncConfig {
            min_batch_size: 10,
            max_batch_size: 10_000,
            target_batch_bytes: 100_000_000,
            max_batch_bytes: 100_000_000,
            max_batch_memory_bytes: Some(20_000_000),
            heavy_block_threshold_bytes: 500_000,
            ..SyncConfig::default()
        };
        let block_limited = SyncConfig {
            max_batch_size: 30,
            ..memory_limited.clone()
        };

        assert_eq!(initial_network_batch_cap(&memory_limited), 40);
        assert_eq!(initial_network_batch_cap(&block_limited), 30);
    }

    #[test]
    fn cached_rescan_planning_is_independent_of_the_device_profile_block_cap() {
        let constrained_profile = SyncConfig {
            max_batch_size: 2_000,
            batch_size: 2_000,
            max_parallel_decrypt: 2,
            max_batch_memory_bytes: Some(48_000_000),
            ..SyncConfig::default()
        };

        assert_eq!(cached_batch_block_cap(), 16_000);
        assert_eq!(
            prefetched_batch_encoded_byte_cap(&constrained_profile),
            48_000_000
        );
    }

    #[test]
    fn cached_rescan_decoder_uses_the_smallest_exact_byte_budget() {
        let config = SyncConfig {
            max_batch_bytes: 128_000_000,
            max_batch_memory_bytes: Some(96_000_000),
            prefetch_queue_max_bytes: 64_000_000,
            ..SyncConfig::default()
        };

        assert_eq!(prefetched_batch_encoded_byte_cap(&config), 64_000_000);
    }

    #[test]
    fn interrupted_tree_replay_resumes_below_the_wallet_birthday() {
        assert_eq!(
            resumable_tree_replay_checkpoint(152_854, 3_499_999, 152_855, Some(152_854)),
            Some(152_854)
        );
        assert_eq!(
            resumable_tree_replay_checkpoint(170_118, 3_499_999, 152_855, Some(170_118)),
            Some(170_118)
        );
        assert_eq!(
            resumable_tree_replay_checkpoint(170_118, 3_499_999, 152_855, Some(160_918)),
            Some(160_918)
        );
    }

    #[test]
    fn normal_wallet_cursor_is_not_mistaken_for_a_partial_tree_replay() {
        assert_eq!(
            resumable_tree_replay_checkpoint(4_000_000, 3_499_999, 152_855, Some(3_499_999)),
            None
        );
        assert_eq!(
            resumable_tree_replay_checkpoint(0, 3_499_999, 152_855, None),
            None
        );
    }

    #[test]
    fn tree_replay_stops_prefetch_at_the_requested_frontier() {
        assert_eq!(
            tree_replay_prefetch_end(Some(3_499_999), 3_498_000, 4_100_000),
            3_499_999
        );
        assert_eq!(
            tree_replay_prefetch_end(Some(3_499_999), 3_500_000, 4_100_000),
            4_100_000
        );
        assert_eq!(
            tree_replay_prefetch_end(None, 152_855, 4_100_000),
            4_100_000
        );
    }

    #[test]
    fn tree_replay_checkpoints_periodically_and_at_the_requested_frontier() {
        assert!(tree_replay_checkpoint_due(
            Some(3_499_999),
            200_000,
            true,
            false
        ));
        assert!(tree_replay_checkpoint_due(
            Some(3_499_999),
            3_499_999,
            false,
            false
        ));
        assert!(!tree_replay_checkpoint_due(
            Some(3_499_999),
            3_499_999,
            true,
            true
        ));
        assert!(!tree_replay_checkpoint_due(None, 200_000, true, false));
    }

    #[tokio::test]
    async fn test_sync_engine_creation() {
        let engine = SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
        assert_eq!(engine.birthday_height(), 3_800_000);
    }

    #[tokio::test]
    async fn test_compute_batch_end_treats_server_hint_as_density_hint() {
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
                true,
            )
            .await
            .unwrap();

        assert_eq!(desired_blocks, 4_000);
        assert_eq!(batch_end, 103_999);
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
                true,
            )
            .await
            .unwrap();

        assert_eq!(desired_blocks, 500);
        assert_eq!(batch_end, 100_499);
    }

    #[tokio::test]
    async fn pending_server_batch_hint_does_not_block_batch_planning() {
        let engine = SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
        let task = ServerBatchHintTask {
            start: 100_000,
            handle: tokio::spawn(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Some(100_199)
            }),
        };
        let started = Instant::now();

        let (value, pending) = engine.resolve_server_batch_hint_task(task).await.unwrap();

        assert_eq!(value, None);
        assert!(started.elapsed() < Duration::from_millis(100));
        let pending = pending.expect("unfinished hint must remain pending");
        pending.handle.abort();
    }

    #[tokio::test]
    async fn test_compute_batch_end_lets_memory_cap_override_minimum() {
        let config = SyncConfig {
            use_server_batch_recommendations: false,
            min_batch_size: 100,
            max_batch_size: 2_000,
            max_batch_memory_bytes: Some(64_000_000),
            ..SyncConfig::default()
        };
        let engine = SyncEngine::with_config(
            "https://lightd.piratechain.com:443".to_string(),
            3_800_000,
            config,
        );
        let mut server_group_end_hint = None;
        let mut pending_server_group_hint = None;
        let tuning = BatchTuning {
            target_bytes: 128_000_000,
            avg_block_size_estimate: 2_000_000,
            max_batch_blocks: 2_000,
        };

        let (batch_end, desired_blocks) = engine
            .compute_batch_end(
                100_000,
                110_000,
                tuning,
                &mut server_group_end_hint,
                &mut pending_server_group_hint,
                true,
            )
            .await
            .unwrap();

        assert_eq!(desired_blocks, 32);
        assert_eq!(batch_end, 100_031);
    }

    #[tokio::test]
    async fn test_birthday_height_update() {
        let mut engine =
            SyncEngine::new("https://lightd.piratechain.com:443".to_string(), 3_800_000);
        engine.set_birthday_height(4_000_000);
        assert_eq!(engine.birthday_height(), 4_000_000);
    }

    #[tokio::test]
    async fn optional_subtree_root_task_never_waits_for_an_unfinished_request() {
        let mut task = HistoricalPrefillTask {
            handle: Some(tokio::spawn(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(RemoteHistoricalSubtreeRoots::default())
            })),
        };

        let started = Instant::now();
        assert!(task.take_ready().await.is_none());
        assert!(started.elapsed() < Duration::from_millis(100));
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

    fn sapling_benchmark_output(recipient_scalar: u64, seed: u8) -> SaplingBatchOutput {
        let recipient_ivk = SaplingIvk(jubjub::Fr::from(recipient_scalar));
        let address = recipient_ivk
            .to_payment_address(sapling::Diversifier([0; 11]))
            .expect("zero diversifier is valid");
        let note = address.create_note(
            sapling::value::NoteValue::from_raw(1),
            Rseed::AfterZip212([seed; 32]),
        );
        let cmu = note.cmu().to_bytes();
        let mut rng = StdRng::seed_from_u64(u64::from(seed));
        let encryption =
            sapling::note_encryption::sapling_note_encryption(None, note, [0; 512], &mut rng);
        let epk = <SaplingDomain as zcash_note_encryption::Domain>::epk_bytes(encryption.epk()).0;
        let ciphertext = encryption.encrypt_note_plaintext()[..COMPACT_NOTE_SIZE]
            .try_into()
            .unwrap();
        SaplingBatchOutput {
            epk,
            cmu,
            ciphertext,
        }
    }

    fn sapling_decrypt_with_chunk_floor(
        pool: &rayon::ThreadPool,
        ivks: &[PreparedIncomingViewingKey],
        outputs: &[(SaplingDomain, SaplingBatchOutput)],
        max_parallel: usize,
        chunk_floor: usize,
    ) -> Vec<CompactDecryptResult<SaplingDomain>> {
        let chunk_size = outputs.len().div_ceil(max_parallel).max(chunk_floor);
        if chunk_size >= outputs.len() {
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

    fn best_sapling_decrypt_sample(
        mut run: impl FnMut() -> Vec<CompactDecryptResult<SaplingDomain>>,
    ) -> (Duration, Vec<bool>) {
        (0..3)
            .map(|_| {
                let started = Instant::now();
                let results = run();
                (
                    started.elapsed(),
                    results.iter().map(Option::is_some).collect::<Vec<_>>(),
                )
            })
            .min_by_key(|(elapsed, _)| *elapsed)
            .expect("three benchmark samples")
    }

    fn sapling_decrypt_signatures(
        results: Vec<CompactDecryptResult<SaplingDomain>>,
    ) -> Vec<Option<(u64, [u8; 11], usize)>> {
        results
            .into_iter()
            .map(|result| {
                result.map(|((note, address), ivk_index)| {
                    (note.value().inner(), address.diversifier().0, ivk_index)
                })
            })
            .collect()
    }

    fn stage_scheduler_tree_pressure(pool: &rayon::ThreadPool, work_items: usize) -> Duration {
        let started = Instant::now();
        pool.install(|| {
            (0..work_items).into_par_iter().for_each(|_| {
                let mut node = SaplingNode::empty_leaf();
                for level in 0..8 {
                    node = SaplingNode::combine(
                        incrementalmerkletree::Level::new(level),
                        &node,
                        &node,
                    );
                }
                std::hint::black_box(node);
            });
        });
        started.elapsed()
    }

    fn stage_scheduler_contention_sample(
        pool: &rayon::ThreadPool,
        ivks: &[PreparedIncomingViewingKey],
        outputs: &[(SaplingDomain, SaplingBatchOutput)],
        max_parallel: usize,
        task_multiplier: usize,
        tree_work_items: usize,
    ) -> (Duration, Duration, Duration, u64, Vec<bool>) {
        let overall_started = Instant::now();
        let ((decrypt_elapsed, decrypt), tree_elapsed) = std::thread::scope(|scope| {
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let decrypt_barrier = Arc::clone(&barrier);
            let decrypt = scope.spawn(move || {
                decrypt_barrier.wait();
                let started = Instant::now();
                let result = try_compact_note_decryption_parallel_measured(
                    pool,
                    ivks,
                    outputs,
                    max_parallel,
                    task_multiplier,
                );
                (started.elapsed(), result)
            });
            let tree = scope.spawn(move || {
                barrier.wait();
                // Current-batch persistence becomes runnable shortly after
                // lookahead decryption has occupied the shared pool.
                std::thread::sleep(Duration::from_millis(1));
                stage_scheduler_tree_pressure(pool, tree_work_items)
            });
            (decrypt.join().unwrap(), tree.join().unwrap())
        });
        (
            overall_started.elapsed(),
            decrypt_elapsed,
            tree_elapsed,
            decrypt.task_count,
            decrypt.value.iter().map(Option::is_some).collect(),
        )
    }

    #[test]
    fn parallel_sapling_decryption_matches_sequential_across_chunk_boundaries() {
        let miss = sapling_benchmark_output(11, 7);
        let hit = sapling_benchmark_output(2, 9);
        let mut outputs = (0..513)
            .map(|_| {
                (
                    SaplingDomain::new(sapling::note_encryption::Zip212Enforcement::On),
                    miss.clone(),
                )
            })
            .collect::<Vec<_>>();
        for index in [0usize, 16, 63, 64, 95, 96, 255, 256, 512] {
            outputs[index].1 = hit.clone();
        }
        let ivks = [
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(2u64))),
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(3u64))),
        ];
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();

        for output_count in [1usize, 63, 64, 95, 96, 97, 127, 128, 255, 256, 257, 513] {
            let sample = &outputs[..output_count];
            let expected =
                sapling_decrypt_signatures(note_batch::try_compact_note_decryption(&ivks, sample));
            let actual = sapling_decrypt_signatures(try_compact_note_decryption_parallel(
                &pool, &ivks, sample, 32,
            ));
            assert_eq!(actual, expected, "output_count={output_count}");
        }
    }

    #[test]
    fn stage_aware_decryption_preserves_order_and_note_results() {
        let miss = sapling_benchmark_output(11, 7);
        let hit = sapling_benchmark_output(2, 9);
        let mut outputs = (0..513)
            .map(|_| {
                (
                    SaplingDomain::new(sapling::note_encryption::Zip212Enforcement::On),
                    miss.clone(),
                )
            })
            .collect::<Vec<_>>();
        for index in [0usize, 63, 64, 255, 256, 512] {
            outputs[index].1 = hit.clone();
        }
        let ivks = [
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(2u64))),
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(3u64))),
        ];
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();

        let baseline = try_compact_note_decryption_parallel_measured(&pool, &ivks, &outputs, 4, 1);
        let stage_aware = try_compact_note_decryption_parallel_measured(
            &pool,
            &ivks,
            &outputs,
            4,
            LOOKAHEAD_DECRYPT_TASK_MULTIPLIER,
        );

        assert!(stage_aware.task_count > baseline.task_count);
        assert_eq!(
            sapling_decrypt_signatures(stage_aware.value),
            sapling_decrypt_signatures(baseline.value)
        );
    }

    /// Manual release-mode benchmark for the Sapling trial-decryption miss path.
    ///
    /// Run with:
    /// `cargo test -p pirate-sync-lightd --lib benchmark_sapling_trial_decrypt_chunking --release -- --ignored --nocapture`
    #[test]
    #[ignore = "manual performance harness"]
    fn benchmark_sapling_trial_decrypt_chunking() {
        let representative_output_count = 2_413;
        let max_output_count = 6_000;
        let miss = sapling_benchmark_output(11, 7);
        let hit = sapling_benchmark_output(2, 9);
        let mut outputs = (0..max_output_count)
            .map(|_| {
                (
                    SaplingDomain::new(sapling::note_encryption::Zip212Enforcement::On),
                    miss.clone(),
                )
            })
            .collect::<Vec<_>>();
        outputs[16].1 = hit;
        let ivks = [
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(2u64))),
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(3u64))),
        ];
        let representative_outputs = &outputs[..representative_output_count];

        for threads in [4usize, 8, 16] {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let (sequential, expected_hits) = best_sapling_decrypt_sample(|| {
                note_batch::try_compact_note_decryption(&ivks, representative_outputs)
            });
            assert_eq!(expected_hits.iter().filter(|hit| **hit).count(), 1);
            eprintln!(
                "sapling trial-decrypt benchmark: threads={threads}, ivks={}, outputs={representative_output_count}, sequential={:.3} ms",
                ivks.len(),
                sequential.as_secs_f64() * 1_000.0,
            );
            for chunk_floor in [64usize, 128, 192, 256, 384, 512, 1_024] {
                let (elapsed, actual_hits) = best_sapling_decrypt_sample(|| {
                    sapling_decrypt_with_chunk_floor(
                        &pool,
                        &ivks,
                        representative_outputs,
                        32,
                        chunk_floor,
                    )
                });
                assert_eq!(actual_hits, expected_hits);
                eprintln!(
                    "parallel chunk_floor={chunk_floor:>4}: {:>8.3} ms ({:.2}x sequential)",
                    elapsed.as_secs_f64() * 1_000.0,
                    sequential.as_secs_f64() / elapsed.as_secs_f64(),
                );
            }

            eprintln!("sapling trial-decrypt crossover: threads={threads}");
            for output_count in [
                32usize, 64, 96, 128, 192, 256, 384, 512, 1_024, 2_413, 6_000,
            ] {
                let sample = &outputs[..output_count];
                let (sequential, sequential_hits) = best_sapling_decrypt_sample(|| {
                    note_batch::try_compact_note_decryption(&ivks, sample)
                });
                let (current, current_hits) = best_sapling_decrypt_sample(|| {
                    sapling_decrypt_with_chunk_floor(&pool, &ivks, sample, 32, 256)
                });
                let (candidate, candidate_hits) = best_sapling_decrypt_sample(|| {
                    sapling_decrypt_with_chunk_floor(&pool, &ivks, sample, 32, 64)
                });
                assert_eq!(current_hits, sequential_hits);
                assert_eq!(candidate_hits, sequential_hits);
                eprintln!(
                    "outputs={output_count:>4}: sequential={:>7.3} ms, current256={:>7.3} ms, candidate64={:>7.3} ms, candidate/current={:.3}",
                    sequential.as_secs_f64() * 1_000.0,
                    current.as_secs_f64() * 1_000.0,
                    candidate.as_secs_f64() * 1_000.0,
                    candidate.as_secs_f64() / current.as_secs_f64(),
                );
            }
        }
    }

    /// Manual release-mode A/B benchmark for shared-pool scheduling.
    ///
    /// Run with:
    /// `cargo test -p pirate-sync-lightd --lib benchmark_stage_aware_cpu_scheduler --release -- --ignored --nocapture`
    #[test]
    #[ignore = "manual performance harness"]
    fn benchmark_stage_aware_cpu_scheduler() {
        let output_count = 2_413;
        let miss = sapling_benchmark_output(11, 7);
        let hit = sapling_benchmark_output(2, 9);
        let mut outputs = (0..output_count)
            .map(|_| {
                (
                    SaplingDomain::new(sapling::note_encryption::Zip212Enforcement::On),
                    miss.clone(),
                )
            })
            .collect::<Vec<_>>();
        outputs[16].1 = hit;
        let ivks = [
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(2u64))),
            PreparedIncomingViewingKey::new(&SaplingIvk(jubjub::Fr::from(3u64))),
        ];
        let threads = num_cpus::get().clamp(2, 16);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let expected = note_batch::try_compact_note_decryption(&ivks, &outputs)
            .iter()
            .map(Option::is_some)
            .collect::<Vec<_>>();

        let cases = [
            ("baseline", threads, 1usize),
            ("reserve_1", threads.saturating_sub(1).max(1), 1),
            ("reserve_2", threads.saturating_sub(2).max(1), 1),
            ("reserve_4", threads.saturating_sub(4).max(1), 1),
            ("sliced_2x", threads, 2),
            ("sliced_4x", threads, LOOKAHEAD_DECRYPT_TASK_MULTIPLIER),
        ];
        for (label, max_parallel, multiplier) in cases {
            let best = (0..3)
                .map(|_| {
                    stage_scheduler_contention_sample(
                        &pool,
                        &ivks,
                        &outputs,
                        max_parallel,
                        multiplier,
                        2_048,
                    )
                })
                .min_by_key(|sample| sample.0)
                .unwrap();
            assert_eq!(best.4, expected);
            eprintln!(
                "stage scheduler: case={}, threads={}, decrypt_parallel={}, multiplier={}, tasks={}, pipeline_ms={:.3}, decrypt_ms={:.3}, tree_ready_ms={:.3}",
                label,
                threads,
                max_parallel,
                multiplier,
                best.3,
                best.0.as_secs_f64() * 1_000.0,
                best.1.as_secs_f64() * 1_000.0,
                best.2.as_secs_f64() * 1_000.0,
            );
        }
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

        let result = SyncEngine::fetch_blocks_with_retry_inner(
            client, 10, 20, cancel, None, None, 1_000_000,
        )
        .await;
        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[tokio::test]
    async fn test_fetch_blocks_with_retry_empty_range() {
        let client = LightClient::new("http://127.0.0.1:1".to_string());
        let cancel = CancelToken::new();

        let blocks = SyncEngine::fetch_blocks_with_retry_inner(
            client, 20, 10, cancel, None, None, 1_000_000,
        )
        .await
        .unwrap();
        assert!(blocks.blocks.is_empty());
    }

    fn fetched_test_batch(blocks: Vec<CompactBlockData>) -> FetchedBlockBatch {
        let shielded_work_items = blocks
            .iter()
            .map(|block| block.shielded_work_items(1, 1))
            .sum();
        FetchedBlockBatch {
            encoded_bytes: blocks.len() as u64,
            shielded_work_items,
            requested_blocks: blocks.len() as u64,
            requested_bytes: u64::MAX,
            requested_work_items: u64::MAX,
            blocks,
            source: BlockFetchSource::Cache,
            elapsed: Duration::ZERO,
            network_elapsed: Duration::ZERO,
            cache_write_elapsed: Duration::ZERO,
            spool_reservations: Vec::new(),
        }
    }

    #[tokio::test]
    async fn one_batch_lookahead_matches_sequential_trial_decryption() {
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 1);
        let blocks = linked_compact_blocks(42, 45);
        let sequential = engine.trial_decrypt_batch(&blocks).await.unwrap().0;
        let (sender, receiver) = mpsc::channel(1);
        sender
            .send(Ok(fetched_test_batch(blocks.clone())))
            .await
            .unwrap();
        drop(sender);
        let producer = tokio::spawn(async {});
        let mut queue = VecDeque::from([PrefetchTask {
            start: 42,
            end: 45,
            payload: Some(PrefetchPayload::Fetch {
                receiver,
                handle: producer,
            }),
        }]);

        engine.start_one_batch_decrypt_lookahead(&mut queue);
        let mut task = queue.pop_front().unwrap();
        let received = engine.receive_prefetch_batch(&mut task).await.unwrap();
        received
            .prepared_commitments
            .as_ref()
            .expect("prepared commitments")
            .validate_source(&received.fetched.blocks)
            .unwrap();
        let prepared = received.prepared_notes.expect("prepared notes").0;

        assert_eq!(
            sequential
                .iter()
                .map(|note| (&note.tx_hash, note.output_index, note.value))
                .collect::<Vec<_>>(),
            prepared
                .iter()
                .map(|note| (&note.tx_hash, note.output_index, note.value))
                .collect::<Vec<_>>()
        );
        assert_eq!(received.fetched.blocks.len(), blocks.len());
        SyncEngine::abort_prefetch_task(&mut task);
    }

    #[tokio::test]
    async fn cancelling_lookahead_releases_its_bounded_batch() {
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 1);
        let watermarks = PrefetchWatermarks::new(64, 32);
        let reservation = watermarks.reserve(32, &CancelToken::new()).await.unwrap();
        let mut fetched = fetched_test_batch(linked_compact_blocks(42, 42));
        fetched.spool_reservations.push(reservation);
        let (sender, receiver) = mpsc::channel(1);
        sender.send(Ok(fetched)).await.unwrap();
        let producer = tokio::spawn(async move {
            let _sender = sender;
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        let mut queue = VecDeque::from([PrefetchTask {
            start: 42,
            end: 42,
            payload: Some(PrefetchPayload::Fetch {
                receiver,
                handle: producer,
            }),
        }]);

        engine.start_one_batch_decrypt_lookahead(&mut queue);
        let mut task = queue.pop_front().unwrap();
        SyncEngine::abort_prefetch_task(&mut task);

        tokio::time::timeout(Duration::from_secs(1), async {
            while watermarks.queued_bytes() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("lookahead byte reservation should be released");
    }

    #[tokio::test]
    async fn prepared_lookahead_still_requires_reorg_boundary_validation() {
        let engine = SyncEngine::new("http://127.0.0.1:9067".to_string(), 1);
        let blocks = linked_compact_blocks(42, 45);
        let (sender, receiver) = mpsc::channel(1);
        sender.send(Ok(fetched_test_batch(blocks))).await.unwrap();
        drop(sender);
        let mut queue = VecDeque::from([PrefetchTask {
            start: 42,
            end: 45,
            payload: Some(PrefetchPayload::Fetch {
                receiver,
                handle: tokio::spawn(async {}),
            }),
        }]);
        engine.start_one_batch_decrypt_lookahead(&mut queue);
        let mut task = queue.pop_front().unwrap();
        let received = engine.receive_prefetch_batch(&mut task).await.unwrap();

        received
            .prepared_commitments
            .as_ref()
            .expect("prepared commitments")
            .validate_source(&received.fetched.blocks)
            .unwrap();
        assert!(!engine
            .validate_batch_boundary(42, &received.fetched.blocks, None, Some(&[0xff; 32]),)
            .unwrap());
        SyncEngine::abort_prefetch_task(&mut task);
    }

    #[test]
    fn rescanning_a_spent_note_reuses_its_raw_storage_row() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let key = EncryptionKey::from_passphrase("note-upsert-test", &[4u8; 32]).unwrap();
        let master_key =
            MasterKey::generate(pirate_storage_sqlite::EncryptionAlgorithm::ChaCha20Poly1305);
        let db = Database::open(file.path(), &key, master_key.clone()).unwrap();
        let repo = Repository::new(&db);
        let account_id = repo
            .insert_account(&pirate_storage_sqlite::Account {
                id: None,
                name: "Rescan persistence".to_string(),
                created_at: 1,
            })
            .unwrap();
        let sink = StorageSink {
            db_path: file.path().to_path_buf(),
            key: EncryptionKey::from_bytes(*key.as_bytes()),
            master_key,
            account_id,
            address_network_type: NetworkType::Mainnet,
        };
        let nullifier = [0x31; 32];
        let spending_txid = [0x52; 32];
        let txid = vec![0x73; 32];
        let mut note =
            DecryptedNote::new(500, 0, 2, 100_000_000, [0x24; 32], nullifier, Vec::new());
        note.set_tx_hash(txid.clone());
        note.position = Some(10);

        sink.persist_notes_with_db(
            &db,
            std::slice::from_ref(&note),
            &HashMap::new(),
            &HashMap::new(),
            &PositionMaps::default(),
        )
        .unwrap();
        assert!(repo
            .mark_note_spent_by_nullifier_with_txid(account_id, &nullifier, &spending_txid)
            .unwrap());

        sink.persist_notes_with_db(
            &db,
            std::slice::from_ref(&note),
            &HashMap::new(),
            &HashMap::new(),
            &PositionMaps::default(),
        )
        .unwrap();

        let raw_rows: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
            .unwrap();
        let stored = repo
            .get_note_by_txid_and_index_with_type(
                account_id,
                &txid,
                2,
                Some(StorageNoteType::Sapling),
            )
            .unwrap()
            .expect("rescanned note");
        assert_eq!(raw_rows, 1);
        assert!(stored.spent);
        assert_eq!(stored.spent_txid, Some(spending_txid.to_vec()));
        assert!(repo.get_unspent_notes(account_id).unwrap().is_empty());
    }

    #[tokio::test]
    async fn persistence_worker_applies_immutable_jobs_in_submission_order() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let key = EncryptionKey::from_passphrase("writer-test", &[9u8; 32]).unwrap();
        let master_key =
            MasterKey::generate(pirate_storage_sqlite::EncryptionAlgorithm::ChaCha20Poly1305);
        let db = Database::open(file.path(), &key, master_key.clone()).unwrap();
        drop(db);
        let sink = StorageSink {
            db_path: file.path().to_path_buf(),
            key: EncryptionKey::from_bytes(*key.as_bytes()),
            master_key,
            account_id: 1,
            address_network_type: NetworkType::Mainnet,
        };
        let worker =
            PersistenceWorker::start(sink, DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES).unwrap();

        worker
            .execute(move |db| {
                db.conn().execute(
                    "INSERT INTO migration_state (key, value, updated_at) VALUES ('writer_order', 'first', '1') ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    [],
                ).map_err(|error| Error::Storage(error.to_string()))?;
                Ok(())
            })
            .await
            .unwrap();
        worker
            .execute(|db| {
                db.conn().execute(
                    "UPDATE migration_state SET value = 'second', updated_at = '2' WHERE key = 'writer_order'",
                    [],
                ).map_err(|error| Error::Storage(error.to_string()))?;
                Ok(())
            })
            .await
            .unwrap();
        let value = worker
            .execute(|db| {
                db.conn()
                    .query_row(
                        "SELECT value FROM migration_state WHERE key = 'writer_order'",
                        [],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(|error| Error::Storage(error.to_string()))
            })
            .await
            .unwrap();

        assert_eq!(value, "second");
    }

    #[tokio::test]
    async fn verified_roots_bridge_sparse_cache_subtree_boundaries_atomically() {
        let (_file, db, sink) = shardtree_test_database("verified-root-boundary", 20);
        let first_root_index = 249u64;
        let pending_root_index = 253u64;
        let local_shard_index = 254u64;
        let existing_sapling = (0..4u32)
            .map(|offset| {
                PersistedSubtreeRoot::new(
                    BlockHeight::from(1_900 + offset),
                    SaplingNode::empty_root(incrementalmerkletree::Level::new((offset + 1) as u8)),
                )
            })
            .collect::<Vec<_>>();
        let existing_ironwood = (0..4u32)
            .map(|offset| {
                PersistedSubtreeRoot::new(
                    BlockHeight::from(1_900 + offset),
                    MerkleHashOrchard::empty_root(incrementalmerkletree::Level::new(
                        (offset + 1) as u8,
                    )),
                )
            })
            .collect::<Vec<_>>();
        let tx = db.unchecked_immediate_transaction().unwrap();
        put_shard_roots::<SaplingNode, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT>(
            &tx,
            SAPLING_TABLE_PREFIX,
            first_root_index,
            &existing_sapling,
        )
        .unwrap();
        put_shard_roots::<MerkleHashOrchard, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT>(
            &tx,
            ORCHARD_TABLE_PREFIX,
            first_root_index,
            &existing_ironwood,
        )
        .unwrap();
        tx.commit().unwrap();
        drop(db);

        let verified_roots = VerifiedSubtreeRoots {
            sapling: vec![shardtree_support::VerifiedSubtreeRoot {
                index: pending_root_index,
                end_height: 1_999,
                root: SaplingNode::empty_root(incrementalmerkletree::Level::new(5)),
            }],
            ironwood: vec![shardtree_support::VerifiedSubtreeRoot {
                index: pending_root_index,
                end_height: 1_999,
                root: MerkleHashOrchard::empty_root(incrementalmerkletree::Level::new(5)),
            }],
        };
        let checkpoint_id = BlockHeight::from(2_000u32);
        let mut batch = ShardtreeBatch::new(2_000);
        batch.checkpoint_id = Some(checkpoint_id);
        shardtree_support::append_sapling_leaf(
            &mut batch,
            local_shard_index << SAPLING_SHARD_HEIGHT,
            SaplingNode::empty_leaf(),
            Retention::Checkpoint {
                id: checkpoint_id,
                marking: Marking::None,
            },
        );
        shardtree_support::append_orchard_leaf(
            &mut batch,
            local_shard_index << ORCHARD_SHARD_HEIGHT,
            MerkleHashOrchard::empty_leaf(),
            Retention::Checkpoint {
                id: checkpoint_id,
                marking: Marking::None,
            },
        );

        let worker =
            PersistenceWorker::start(sink, DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES).unwrap();
        worker
            .persist_shardtree_batches_with_roots(vec![batch], Some(2_000), verified_roots)
            .await
            .unwrap();

        let ranges = worker
            .execute(move |db| {
                let read_range = |table_prefix: &str| {
                    db.conn().query_row(
                        &format!(
                            "SELECT MIN(shard_index), MAX(shard_index), COUNT(*) FROM {}_tree_shards",
                            table_prefix
                        ),
                        [],
                        |row| {
                            Ok((
                                row.get::<_, Option<i64>>(0)?,
                                row.get::<_, Option<i64>>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                };
                let sapling = read_range(SAPLING_TABLE_PREFIX)
                    .map_err(|error| Error::Storage(error.to_string()))?;
                let ironwood = read_range(ORCHARD_TABLE_PREFIX)
                    .map_err(|error| Error::Storage(error.to_string()))?;
                let sapling_root_height = db
                    .conn()
                    .query_row(
                        "SELECT subtree_end_height FROM sapling_tree_shards WHERE shard_index = ?1",
                        [pending_root_index as i64],
                        |row| row.get::<_, Option<i64>>(0),
                    )
                    .map_err(|error| Error::Storage(error.to_string()))?;
                let ironwood_root_height = db
                    .conn()
                    .query_row(
                        "SELECT subtree_end_height FROM orchard_tree_shards WHERE shard_index = ?1",
                        [pending_root_index as i64],
                        |row| row.get::<_, Option<i64>>(0),
                    )
                    .map_err(|error| Error::Storage(error.to_string()))?;
                Ok((sapling, ironwood, sapling_root_height, ironwood_root_height))
            })
            .await
            .unwrap();

        assert_eq!(ranges.0, (Some(249), Some(254), 6));
        assert_eq!(ranges.1, (Some(249), Some(254), 6));
        assert_eq!(ranges.2, Some(1_999));
        assert_eq!(ranges.3, Some(1_999));
    }

    #[tokio::test]
    async fn marked_leaf_survives_grafted_roots_and_dense_tip_checkpoints() {
        let (_file, db, sink) = shardtree_test_database("grafted-root-mark", 21);
        let pending_root_index = 4u64;
        let local_shard_index = 5u64;
        let existing_sapling = (0..4u32)
            .map(|offset| {
                PersistedSubtreeRoot::new(
                    BlockHeight::from(1_900 + offset),
                    SaplingNode::empty_root(incrementalmerkletree::Level::new((offset + 1) as u8)),
                )
            })
            .collect::<Vec<_>>();
        let existing_ironwood = (0..4u32)
            .map(|offset| {
                PersistedSubtreeRoot::new(
                    BlockHeight::from(1_900 + offset),
                    MerkleHashOrchard::empty_root(incrementalmerkletree::Level::new(
                        (offset + 1) as u8,
                    )),
                )
            })
            .collect::<Vec<_>>();
        let tx = db.unchecked_immediate_transaction().unwrap();
        put_shard_roots::<SaplingNode, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT>(
            &tx,
            SAPLING_TABLE_PREFIX,
            0,
            &existing_sapling,
        )
        .unwrap();
        put_shard_roots::<MerkleHashOrchard, { NOTE_COMMITMENT_TREE_DEPTH }, ORCHARD_SHARD_HEIGHT>(
            &tx,
            ORCHARD_TABLE_PREFIX,
            0,
            &existing_ironwood,
        )
        .unwrap();
        tx.commit().unwrap();
        drop(db);

        let verified_roots = VerifiedSubtreeRoots {
            sapling: vec![shardtree_support::VerifiedSubtreeRoot {
                index: pending_root_index,
                end_height: 1_999,
                root: SaplingNode::empty_root(incrementalmerkletree::Level::new(5)),
            }],
            ironwood: vec![shardtree_support::VerifiedSubtreeRoot {
                index: pending_root_index,
                end_height: 1_999,
                root: MerkleHashOrchard::empty_root(incrementalmerkletree::Level::new(5)),
            }],
        };
        let checkpoint_id = BlockHeight::from(2_000u32);
        let mut marked_batch = ShardtreeBatch::new(2_000);
        marked_batch.checkpoint_id = Some(checkpoint_id);
        shardtree_support::append_sapling_leaf(
            &mut marked_batch,
            local_shard_index << SAPLING_SHARD_HEIGHT,
            SaplingNode::empty_leaf(),
            Retention::Checkpoint {
                id: checkpoint_id,
                marking: Marking::Marked,
            },
        );
        shardtree_support::append_orchard_leaf(
            &mut marked_batch,
            local_shard_index << ORCHARD_SHARD_HEIGHT,
            MerkleHashOrchard::empty_leaf(),
            Retention::Checkpoint {
                id: checkpoint_id,
                marking: Marking::None,
            },
        );

        let worker =
            PersistenceWorker::start(sink, DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES).unwrap();
        worker
            .persist_shardtree_batches_with_roots(vec![marked_batch], Some(2_000), verified_roots)
            .await
            .unwrap();

        let dense_tip_batches = (1..=1_005u32)
            .map(|offset| {
                let height = 2_000 + offset;
                let retention = Retention::Checkpoint {
                    id: BlockHeight::from(height),
                    marking: Marking::None,
                };
                let mut batch = ShardtreeBatch::new(u64::from(height));
                batch.checkpoint_id = Some(BlockHeight::from(height));
                shardtree_support::append_sapling_leaf(
                    &mut batch,
                    (local_shard_index << SAPLING_SHARD_HEIGHT) + u64::from(offset),
                    SaplingNode::empty_leaf(),
                    retention,
                );
                shardtree_support::append_orchard_leaf(
                    &mut batch,
                    (local_shard_index << ORCHARD_SHARD_HEIGHT) + u64::from(offset),
                    MerkleHashOrchard::empty_leaf(),
                    retention,
                );
                batch
            })
            .collect::<Vec<_>>();
        for chunk in dense_tip_batches.chunks(200) {
            worker
                .persist_shardtree_batches(chunk.to_vec(), chunk.last().map(|batch| batch.height))
                .await
                .unwrap();
        }

        let marked_position = Position::from(local_shard_index << SAPLING_SHARD_HEIGHT);
        let witness_available = worker
            .execute(move |db| {
                let store =
                    SqliteShardStore::<_, SaplingNode, SAPLING_SHARD_HEIGHT>::from_connection(
                        db.conn(),
                        SAPLING_TABLE_PREFIX,
                    )
                    .map_err(|error| Error::Storage(error.to_string()))?;
                let mut tree: ShardTree<_, { NOTE_COMMITMENT_TREE_DEPTH }, SAPLING_SHARD_HEIGHT> =
                    ShardTree::new(store, SHARDTREE_PRUNING_DEPTH);
                tree.witness_at_checkpoint_depth_caching(marked_position, 0)
                    .map(|witness| witness.is_some())
                    .map_err(|error| Error::Storage(error.to_string()))
            })
            .await
            .unwrap();

        assert!(
            witness_available,
            "a marked leaf after a grafted root must remain witnessable after checkpoint pruning"
        );
    }

    #[test]
    fn worker_sparse_cache_matches_fresh_store_persistence_for_both_pools() {
        let (_baseline_file, baseline_db, baseline_sink) =
            shardtree_test_database("baseline-cache-test", 10);
        let (_candidate_file, candidate_db, _candidate_sink) =
            shardtree_test_database("candidate-cache-test", 11);
        let batches = persistence_test_batches();

        SyncEngine::persist_shardtree_batches_for_storage(
            Some(&baseline_sink),
            &batches[..2],
            Some(101),
            Some(&baseline_db),
        )
        .unwrap();
        SyncEngine::persist_shardtree_batches_for_storage(
            Some(&baseline_sink),
            &batches[2..],
            Some(103),
            Some(&baseline_db),
        )
        .unwrap();

        let mut cached = PersistenceShardTrees::load(
            candidate_db.conn(),
            DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES,
        )
        .unwrap();
        let (_, first, evicted) = cached
            .persist_batches(&candidate_db, &batches[..2], Some(101))
            .unwrap();
        assert!(!evicted);
        assert!(!first.cache_reused);
        assert!(first.sapling.preload_discovery > Duration::ZERO);
        assert!(first.ironwood.preload_discovery > Duration::ZERO);

        let (_, second, evicted) = cached
            .persist_batches(&candidate_db, &batches[2..], Some(103))
            .unwrap();
        assert!(!evicted);
        assert!(second.cache_reused);
        assert_eq!(second.sapling.preload_discovery, Duration::ZERO);
        assert_eq!(second.ironwood.preload_discovery, Duration::ZERO);
        assert_eq!(second.sapling.commitment_count, 2);
        assert_eq!(second.ironwood.commitment_count, 2);
        assert!(second.sapling.dirty_shards > 0);
        assert!(second.ironwood.dirty_shards > 0);
        assert!(second.sapling.dirty_encoded_bytes > 0);
        assert!(second.ironwood.dirty_encoded_bytes > 0);
        assert!(second.sapling.peak_cache_bytes > 0);
        assert!(second.ironwood.peak_cache_bytes > 0);

        SyncEngine::create_checkpoint_with_db(&baseline_db, 103).unwrap();
        SyncEngine::retain_checkpoint_with_db(&baseline_db, 103).unwrap();
        let (checkpoint_telemetry, evicted) = cached
            .checkpoint_tip(&candidate_db, BlockHeight::from(103u32))
            .unwrap();
        assert!(!evicted);
        assert!(checkpoint_telemetry.cache_reused);
        let (retain_telemetry, evicted) = cached
            .retain_checkpoint(&candidate_db, BlockHeight::from(103u32))
            .unwrap();
        assert!(!evicted);
        assert!(retain_telemetry.cache_reused);

        let baseline =
            pirate_storage_sqlite::SemanticOracleSnapshot::capture(&baseline_db, 1).unwrap();
        let candidate =
            pirate_storage_sqlite::SemanticOracleSnapshot::capture(&candidate_db, 1).unwrap();
        baseline.ensure_equivalent(&candidate).unwrap();
    }

    #[test]
    fn sparse_cache_memory_limit_evicts_only_after_a_successful_commit() {
        let (_file, db, _sink) = shardtree_test_database("cache-memory-limit", 19);
        let batches = persistence_test_batches();
        let mut cached = PersistenceShardTrees::load(db.conn(), 1).unwrap();

        let (_, telemetry, evict) = cached
            .persist_batches(&db, &batches[..2], Some(101))
            .unwrap();

        assert!(evict);
        assert!(telemetry.cache_evicted_after_commit);
        assert!(telemetry.sapling.cache_evictions > 0);
        assert!(telemetry.ironwood.cache_evictions > 0);
        let max_checkpoint: Option<u32> = db
            .conn()
            .query_row(
                "SELECT MAX(checkpoint_id) FROM sapling_tree_checkpoints",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(max_checkpoint, Some(101));
    }

    #[tokio::test]
    async fn persistence_worker_reloads_cache_after_failed_commit() {
        let (_file, db, sink) = shardtree_test_database("failed-commit-cache-test", 12);
        db.conn()
            .execute_batch(
                r#"
                PRAGMA foreign_keys = ON;
                CREATE TABLE cache_commit_parent (id INTEGER PRIMARY KEY);
                CREATE TABLE cache_commit_child (
                    parent_id INTEGER NOT NULL,
                    FOREIGN KEY(parent_id) REFERENCES cache_commit_parent(id)
                        DEFERRABLE INITIALLY DEFERRED
                );
                CREATE TRIGGER fail_cached_shardtree_commit
                AFTER INSERT ON sapling_tree_shards
                BEGIN
                    INSERT INTO cache_commit_child(parent_id) VALUES (1);
                END;
                "#,
            )
            .unwrap();
        drop(db);
        let worker =
            PersistenceWorker::start(sink, DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES).unwrap();
        worker
            .execute(|db| {
                db.conn()
                    .execute_batch("PRAGMA foreign_keys = ON;")
                    .map_err(|error| Error::Storage(error.to_string()))?;
                Ok(())
            })
            .await
            .unwrap();
        let batches = persistence_test_batches();

        let failed = worker
            .persist_shardtree_batches(batches[..2].to_vec(), Some(101))
            .await;
        assert!(failed.is_err());
        worker
            .execute(|db| {
                db.conn()
                    .execute_batch(
                        "DROP TRIGGER fail_cached_shardtree_commit; DELETE FROM cache_commit_child;",
                    )
                    .map_err(|error| Error::Storage(error.to_string()))?;
                Ok(())
            })
            .await
            .unwrap();
        worker
            .persist_shardtree_batches(batches[..2].to_vec(), Some(101))
            .await
            .unwrap();
        let max_checkpoint = worker
            .execute(|db| {
                db.conn()
                    .query_row(
                        "SELECT MAX(checkpoint_id) FROM sapling_tree_checkpoints",
                        [],
                        |row| row.get::<_, Option<u32>>(0),
                    )
                    .map_err(|error| Error::Storage(error.to_string()))
            })
            .await
            .unwrap();
        assert_eq!(max_checkpoint, Some(101));
    }

    #[tokio::test]
    async fn rollback_invalidates_worker_cache_before_replay() {
        let (_baseline_file, baseline_db, baseline_sink) =
            shardtree_test_database("rollback-baseline", 13);
        let (_candidate_file, candidate_db, candidate_sink) =
            shardtree_test_database("rollback-candidate", 14);
        let batches = persistence_test_batches();
        SyncEngine::persist_shardtree_batches_for_storage(
            Some(&baseline_sink),
            &batches,
            Some(103),
            Some(&baseline_db),
        )
        .unwrap();
        assert_eq!(truncate_above_height(&baseline_db, 102).unwrap(), 102);
        SyncEngine::persist_shardtree_batches_for_storage(
            Some(&baseline_sink),
            &batches[3..],
            Some(103),
            Some(&baseline_db),
        )
        .unwrap();
        drop(candidate_db);
        let worker =
            PersistenceWorker::start(candidate_sink, DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES)
                .unwrap();
        worker
            .persist_shardtree_batches(batches.clone(), Some(103))
            .await
            .unwrap();
        let replay_height = worker
            .execute_invalidating_shardtrees(|db| {
                truncate_above_height(db, 102).map_err(Into::into)
            })
            .await
            .unwrap();
        assert_eq!(replay_height, 102);
        worker
            .persist_shardtree_batches(batches[3..].to_vec(), Some(103))
            .await
            .unwrap();
        let candidate = worker
            .execute(|db| {
                pirate_storage_sqlite::SemanticOracleSnapshot::capture(db, 1)
                    .map_err(|error| Error::Storage(error.to_string()))
            })
            .await
            .unwrap();
        let baseline =
            pirate_storage_sqlite::SemanticOracleSnapshot::capture(&baseline_db, 1).unwrap();
        baseline.ensure_equivalent(&candidate).unwrap();
    }

    #[tokio::test]
    async fn cancelled_persistence_response_discards_the_committed_cache() {
        let (_baseline_file, baseline_db, baseline_sink) =
            shardtree_test_database("cancel-baseline", 15);
        let (_candidate_file, candidate_db, candidate_sink) =
            shardtree_test_database("cancel-candidate", 16);
        let batches = persistence_test_batches();
        let mut replacement = batches[1].clone();
        replacement.sapling[0].0 = SaplingNode::empty_root(incrementalmerkletree::Level::new(1));
        replacement.orchard[0].0 =
            MerkleHashOrchard::empty_root(incrementalmerkletree::Level::new(1));

        SyncEngine::persist_shardtree_batches_for_storage(
            Some(&baseline_sink),
            &batches[..2],
            Some(101),
            Some(&baseline_db),
        )
        .unwrap();
        assert_eq!(truncate_above_height(&baseline_db, 100).unwrap(), 100);
        SyncEngine::persist_shardtree_batches_for_storage(
            Some(&baseline_sink),
            std::slice::from_ref(&replacement),
            Some(101),
            Some(&baseline_db),
        )
        .unwrap();

        drop(candidate_db);
        let worker = Arc::new(
            PersistenceWorker::start(candidate_sink, DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES)
                .unwrap(),
        );
        let (entered_sender, entered_receiver) = oneshot::channel();
        let (release_sender, release_receiver) = std_mpsc::sync_channel(1);
        let blocking_worker = Arc::clone(&worker);
        let blocker = tokio::spawn(async move {
            blocking_worker
                .execute(move |_| {
                    let _ = entered_sender.send(());
                    let _ = release_receiver.recv();
                    Ok(())
                })
                .await
        });
        entered_receiver.await.unwrap();

        let cancelled_worker = Arc::clone(&worker);
        let cancelled_batches = batches[..2].to_vec();
        let cancelled = tokio::spawn(async move {
            cancelled_worker
                .persist_shardtree_batches(cancelled_batches, Some(101))
                .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancelled.abort();
        let _ = cancelled.await;
        release_sender.send(()).unwrap();
        blocker.await.unwrap().unwrap();

        let committed_height = worker
            .execute(|db| {
                db.conn()
                    .query_row(
                        "SELECT MAX(checkpoint_id) FROM sapling_tree_checkpoints",
                        [],
                        |row| row.get::<_, Option<u32>>(0),
                    )
                    .map_err(|error| Error::Storage(error.to_string()))
            })
            .await
            .unwrap();
        assert_eq!(committed_height, Some(101));
        worker
            .execute(|db| truncate_above_height(db, 100).map_err(Into::into))
            .await
            .unwrap();
        worker
            .persist_shardtree_batches(vec![replacement], Some(101))
            .await
            .unwrap();

        let candidate = worker
            .execute(|db| {
                pirate_storage_sqlite::SemanticOracleSnapshot::capture(db, 1)
                    .map_err(|error| Error::Storage(error.to_string()))
            })
            .await
            .unwrap();
        let baseline =
            pirate_storage_sqlite::SemanticOracleSnapshot::capture(&baseline_db, 1).unwrap();
        baseline.ensure_equivalent(&candidate).unwrap();
    }

    #[test]
    #[ignore = "manual persistence microbenchmark"]
    fn benchmark_long_lived_sparse_cache_against_per_batch_reload() {
        let (_baseline_file, baseline_db, baseline_sink) =
            shardtree_test_database("cache-benchmark-baseline", 17);
        let (_candidate_file, candidate_db, _candidate_sink) =
            shardtree_test_database("cache-benchmark-candidate", 18);
        let batches = persistence_test_batches_with_count(20);

        let baseline_start = Instant::now();
        for chunk in batches.chunks(2) {
            SyncEngine::persist_shardtree_batches_for_storage(
                Some(&baseline_sink),
                chunk,
                chunk.last().map(|batch| batch.height),
                Some(&baseline_db),
            )
            .unwrap();
        }
        let baseline_elapsed = baseline_start.elapsed();

        let candidate_start = Instant::now();
        let mut cached = PersistenceShardTrees::load(
            candidate_db.conn(),
            DEFAULT_PERSISTENCE_SHARDTREE_CACHE_BYTES,
        )
        .unwrap();
        for chunk in batches.chunks(2) {
            let (_, _, evicted) = cached
                .persist_batches(&candidate_db, chunk, chunk.last().map(|batch| batch.height))
                .unwrap();
            assert!(!evicted);
        }
        let candidate_elapsed = candidate_start.elapsed();

        let baseline =
            pirate_storage_sqlite::SemanticOracleSnapshot::capture(&baseline_db, 1).unwrap();
        let candidate =
            pirate_storage_sqlite::SemanticOracleSnapshot::capture(&candidate_db, 1).unwrap();
        baseline.ensure_equivalent(&candidate).unwrap();
        eprintln!(
            "fresh_per_batch_us={} long_lived_sparse_us={} speedup={:.2}x",
            baseline_elapsed.as_micros(),
            candidate_elapsed.as_micros(),
            baseline_elapsed.as_secs_f64() / candidate_elapsed.as_secs_f64()
        );
    }
}
